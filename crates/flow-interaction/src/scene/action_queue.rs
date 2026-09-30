//! Bounded deferred actions avoid re-entering the document during listeners.
use super::SceneCommand;
use gpui_flow_engine::{ElementHandle, EngineError, InstanceState, SceneEngine};
use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};

const CAPACITY: usize = 256;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mount {
    handle: ElementHandle,
    epoch: u64,
}
impl Mount {
    fn new(engine: &SceneEngine, handle: ElementHandle) -> Result<Self, EngineError> {
        engine.object(handle)?;
        let info = engine.instance(handle)?;
        if info.state != InstanceState::Attached {
            return Err(EngineError::DetachedHandle);
        }
        Ok(Self {
            handle,
            epoch: info.attachment_epoch,
        })
    }
    fn validate(self, engine: &SceneEngine) -> Result<(), EngineError> {
        if Self::new(engine, self.handle)? != self {
            return Err(EngineError::InvalidHandle);
        }
        Ok(())
    }
}
/// A deferred focus request scoped to the current mounts of both endpoints.
/// Prepare outside a listener with the engine, then enqueue without borrowing it.
/// After either endpoint remounts, prepare a fresh request; old requests cannot
/// transfer authority across a detach/reattach boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SceneFocusRequest {
    origin: Mount,
    target: Option<Mount>,
}
impl SceneFocusRequest {
    pub fn new(
        engine: &SceneEngine,
        origin: ElementHandle,
        target: Option<ElementHandle>,
    ) -> Result<Self, EngineError> {
        Ok(Self {
            origin: Mount::new(engine, origin)?,
            target: target.map(|h| Mount::new(engine, h)).transpose()?,
        })
    }
    pub(super) fn origin(self) -> ElementHandle {
        self.origin.handle
    }
    pub(super) fn target(self) -> Option<ElementHandle> {
        self.target.map(|m| m.handle)
    }
    pub(super) fn validate(self, engine: &SceneEngine) -> Result<(), EngineError> {
        self.origin.validate(engine)?;
        if let Some(target) = self.target {
            target.validate(engine)?;
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct SceneActionSender {
    shared: Arc<Mutex<Shared>>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionQueueError {
    Full,
    Closed,
    FocusBudgetExhausted,
}
impl SceneActionSender {
    pub fn enqueue(
        &self,
        origin: ElementHandle,
        command: SceneCommand,
    ) -> Result<(), ActionQueueError> {
        self.push(origin, SceneAction::Command(Box::new(command)))
    }
    pub fn enqueue_focus(&self, request: SceneFocusRequest) -> Result<(), ActionQueueError> {
        self.push(request.origin(), SceneAction::Focus(request))
    }
    fn push(&self, origin: ElementHandle, action: SceneAction) -> Result<(), ActionQueueError> {
        let mut queue = self.shared.lock().expect("action queue lock");
        if !queue.open {
            return Err(ActionQueueError::Closed);
        }
        if queue.focus_blocked && matches!(action, SceneAction::Focus(_)) {
            return Err(ActionQueueError::FocusBudgetExhausted);
        }
        if queue.items.len() >= CAPACITY {
            return Err(ActionQueueError::Full);
        }
        queue.items.push_back((origin, action));
        Ok(())
    }
}
pub(super) enum SceneAction {
    Command(Box<SceneCommand>),
    Focus(SceneFocusRequest),
}
struct Shared {
    items: VecDeque<(ElementHandle, SceneAction)>,
    open: bool,
    focus_blocked: bool,
}
pub(super) struct SceneActionQueue {
    pub sender: SceneActionSender,
    shared: Arc<Mutex<Shared>>,
}
impl Default for SceneActionQueue {
    fn default() -> Self {
        let shared = Arc::new(Mutex::new(Shared {
            items: VecDeque::new(),
            open: true,
            focus_blocked: false,
        }));
        Self {
            sender: SceneActionSender {
                shared: shared.clone(),
            },
            shared,
        }
    }
}
impl SceneActionQueue {
    pub fn begin_operation(&mut self) {
        self.shared.lock().expect("action queue lock").focus_blocked = false;
    }
    pub fn next(&mut self) -> Option<(ElementHandle, SceneAction)> {
        self.shared
            .lock()
            .expect("action queue lock")
            .items
            .pop_front()
    }
    /// Atomically retain every ordinary command in FIFO order and drop all
    /// remaining focus redirects, even in a full mixed queue. Concurrent focus
    /// producers are rejected until the next router operation starts.
    pub fn discard_pending_focus(&mut self) -> usize {
        let mut queue = self.shared.lock().expect("action queue lock");
        queue.focus_blocked = true;
        let before = queue.items.len();
        queue
            .items
            .retain(|(_, action)| matches!(action, SceneAction::Command(_)));
        before - queue.items.len()
    }
}
impl Drop for SceneActionQueue {
    fn drop(&mut self) {
        let mut queue = self.shared.lock().expect("action queue lock");
        queue.open = false;
        queue.items.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_scene::{DisplayObject, Scene, display};
    fn request() -> (ElementHandle, SceneFocusRequest) {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("a", display::Rect::new(10., 10.)),
                None,
                None,
            )
            .unwrap();
        let engine = SceneEngine::new(scene).unwrap();
        let a = engine.handle(&"a".into()).unwrap();
        (a, SceneFocusRequest::new(&engine, a, Some(a)).unwrap())
    }
    fn command(i: usize) -> SceneCommand {
        SceneCommand::SetObjectData {
            id: "a".into(),
            data: serde_json::json!(i),
        }
    }
    #[test]
    fn full_mixed_queue_discards_every_focus_preserves_accepted_commands_and_bounds_storage() {
        let (origin, request) = request();
        let mut queue = SceneActionQueue::default();
        let sender = queue.sender.clone();
        for i in 0..128 {
            sender.enqueue(origin, command(i)).unwrap();
            sender.enqueue_focus(request).unwrap();
        }
        assert_eq!(
            sender.enqueue(origin, command(999)),
            Err(ActionQueueError::Full)
        );
        assert_eq!(queue.discard_pending_focus(), 128);
        for i in 128..256 {
            sender.enqueue(origin, command(i)).unwrap();
        }
        assert_eq!(
            sender.enqueue_focus(request),
            Err(ActionQueueError::FocusBudgetExhausted)
        );
        assert_eq!(
            sender.enqueue(origin, command(999)),
            Err(ActionQueueError::Full)
        );
        for i in 0..256 {
            match queue.next().unwrap().1 {
                SceneAction::Command(actual) => assert_eq!(*actual, command(i)),
                SceneAction::Focus(_) => panic!("stale focus"),
            }
        }
        assert!(queue.next().is_none());
        queue.begin_operation();
        sender.enqueue_focus(request).unwrap();
        assert!(matches!(queue.next().unwrap().1, SceneAction::Focus(_)));
        drop(queue);
        assert_eq!(sender.enqueue_focus(request), Err(ActionQueueError::Closed));
    }
    #[test]
    fn concurrent_focus_sender_cannot_refill_queue_after_budget_closes_focus_gate() {
        let (origin, request) = request();
        let mut queue = SceneActionQueue::default();
        let sender = queue.sender.clone();
        for i in 0..127 {
            sender.enqueue(origin, command(i)).unwrap();
            sender.enqueue_focus(request).unwrap();
        }
        let barrier = Arc::new(std::sync::Barrier::new(2));
        let worker_barrier = barrier.clone();
        let worker_sender = sender.clone();
        let worker = std::thread::spawn(move || {
            worker_barrier.wait();
            worker_sender.enqueue_focus(request)
        });
        barrier.wait();
        queue.discard_pending_focus();
        assert!(matches!(
            worker.join().unwrap(),
            Ok(()) | Err(ActionQueueError::FocusBudgetExhausted)
        ));
        assert_eq!(
            sender.enqueue_focus(request),
            Err(ActionQueueError::FocusBudgetExhausted)
        );
        for i in 0..127 {
            match queue.next().unwrap().1 {
                SceneAction::Command(actual) => assert_eq!(*actual, command(i)),
                SceneAction::Focus(_) => panic!("focus refilled"),
            }
        }
        assert!(queue.next().is_none());
        sender.enqueue(origin, command(127)).unwrap();
        assert!(matches!(queue.next().unwrap().1, SceneAction::Command(_)));
    }
}
