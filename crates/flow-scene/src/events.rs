use crate::{DisplayId, Point, Scene, SceneError};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(tag = "type", content = "name", rename_all = "snake_case")]
pub enum EventKind {
    PointerDown,
    PointerMove,
    PointerUp,
    PointerCancel,
    Click,
    DoubleClick,
    PointerEnter,
    PointerLeave,
    Wheel,
    KeyDown,
    KeyUp,
    Custom(String),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventPhase {
    Capture,
    Target,
    Bubble,
}
#[derive(Debug, Clone)]
pub struct SceneEvent {
    pub kind: EventKind,
    pub target: DisplayId,
    pub current_target: DisplayId,
    pub phase: EventPhase,
    pub world_position: Option<Point>,
    pub local_position: Option<Point>,
    pub data: Value,
    pub bubbles: bool,
    pub cancelable: bool,
    default_prevented: bool,
    propagation_stopped: bool,
    immediate_stopped: bool,
}
impl SceneEvent {
    pub fn new(kind: EventKind, target: impl Into<DisplayId>) -> Self {
        let bubbles = !matches!(kind, EventKind::PointerEnter | EventKind::PointerLeave);
        let target = target.into();
        Self {
            kind,
            target: target.clone(),
            current_target: target,
            phase: EventPhase::Target,
            world_position: None,
            local_position: None,
            data: Value::Null,
            bubbles,
            cancelable: true,
            default_prevented: false,
            propagation_stopped: false,
            immediate_stopped: false,
        }
    }
    pub fn at(mut self, world_position: Point) -> Self {
        self.world_position = Some(world_position);
        self
    }
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
    pub fn prevent_default(&mut self) {
        if self.cancelable {
            self.default_prevented = true;
        }
    }
    pub fn default_prevented(&self) -> bool {
        self.default_prevented
    }
    pub fn stop_propagation(&mut self) {
        self.propagation_stopped = true;
    }
    pub fn stop_immediate_propagation(&mut self) {
        self.immediate_stopped = true;
        self.propagation_stopped = true;
    }
    pub fn propagation_stopped(&self) -> bool {
        self.propagation_stopped
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ListenerOptions {
    pub capture: bool,
    pub once: bool,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ListenerId(u64);
struct Listener {
    node: DisplayId,
    kind: EventKind,
    options: ListenerOptions,
    callback: std::boxed::Box<dyn FnMut(&mut SceneEvent)>,
}
#[derive(Default)]
pub struct EventDispatcher {
    next_id: u64,
    listeners: BTreeMap<ListenerId, Listener>,
}
#[derive(Debug, Clone)]
pub struct DispatchResult {
    pub event: SceneEvent,
    pub invoked: usize,
}
impl EventDispatcher {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn on(
        &mut self,
        node: impl Into<DisplayId>,
        kind: EventKind,
        options: ListenerOptions,
        handler: impl FnMut(&mut SceneEvent) + 'static,
    ) -> ListenerId {
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("event listener identifier space exhausted");
        let id = ListenerId(self.next_id);
        self.listeners.insert(
            id,
            Listener {
                node: node.into(),
                kind,
                options,
                callback: std::boxed::Box::new(handler),
            },
        );
        id
    }
    pub fn off(&mut self, id: ListenerId) -> bool {
        self.listeners.remove(&id).is_some()
    }
    pub fn remove_node(&mut self, node: &DisplayId) {
        self.listeners.retain(|_, listener| &listener.node != node);
    }
    pub fn retain_existing(&mut self, scene: &Scene) {
        self.listeners
            .retain(|_, listener| scene.get(&listener.node).is_some());
    }
    pub fn clear(&mut self) {
        self.listeners.clear();
    }
    pub fn len(&self) -> usize {
        self.listeners.len()
    }
    pub fn is_empty(&self) -> bool {
        self.listeners.is_empty()
    }
    /// Dispatch along a fixed ancestor path. Capture and bubble listeners both
    /// run at the target (phase Target), with capture listeners first. Returning
    /// false from a callback has no special meaning; use explicit stop methods.
    pub fn dispatch(
        &mut self,
        scene: &Scene,
        mut event: SceneEvent,
    ) -> Result<DispatchResult, SceneError> {
        scene.validate()?;
        let path = scene.ancestor_path(&event.target)?;
        if event.world_position.is_some_and(|p| !p.is_finite()) {
            return Err(SceneError::InvalidGeometry(
                "event position must be finite".into(),
            ));
        }
        // A SceneEvent may be reused by callers; routing starts fresh while an
        // already-prevented default remains prevented.
        event.propagation_stopped = false;
        event.immediate_stopped = false;
        let kind = event.kind.clone();
        let target = event.target.clone();
        let mut invoked = 0;
        for id in path.iter().take(path.len().saturating_sub(1)) {
            self.invoke(
                scene,
                &kind,
                &target,
                id,
                EventPhase::Capture,
                true,
                &mut event,
                &mut invoked,
            );
            if event.propagation_stopped {
                return Ok(DispatchResult { event, invoked });
            }
        }
        self.invoke(
            scene,
            &kind,
            &target,
            &target,
            EventPhase::Target,
            true,
            &mut event,
            &mut invoked,
        );
        if !event.immediate_stopped {
            self.invoke(
                scene,
                &kind,
                &target,
                &target,
                EventPhase::Target,
                false,
                &mut event,
                &mut invoked,
            );
        }
        if event.bubbles && !event.propagation_stopped {
            for id in path.iter().rev().skip(1) {
                self.invoke(
                    scene,
                    &kind,
                    &target,
                    id,
                    EventPhase::Bubble,
                    false,
                    &mut event,
                    &mut invoked,
                );
                if event.propagation_stopped {
                    break;
                }
            }
        }
        Ok(DispatchResult { event, invoked })
    }
    #[allow(clippy::too_many_arguments)]
    fn invoke(
        &mut self,
        scene: &Scene,
        kind: &EventKind,
        target: &DisplayId,
        current: &DisplayId,
        phase: EventPhase,
        capture: bool,
        event: &mut SceneEvent,
        invoked: &mut usize,
    ) {
        let ids: Vec<_> = self
            .listeners
            .iter()
            .filter(|(_, listener)| {
                &listener.node == current
                    && &listener.kind == kind
                    && listener.options.capture == capture
            })
            .map(|(id, _)| *id)
            .collect();
        for id in ids {
            event.kind = kind.clone();
            event.target = target.clone();
            event.current_target = current.clone();
            event.phase = phase;
            event.local_position = event
                .world_position
                .and_then(|world| scene.world_to_local(current, world).ok());
            let once = self.listeners[&id].options.once;
            if once {
                if let Some(mut listener) = self.listeners.remove(&id) {
                    (listener.callback)(event);
                }
            } else if let Some(listener) = self.listeners.get_mut(&id) {
                (listener.callback)(event);
            }
            *invoked += 1;
            // Routing identity is determined by dispatch, even if a listener
            // accidentally writes these public observation fields.
            event.kind = kind.clone();
            event.target = target.clone();
            event.current_target = current.clone();
            event.phase = phase;
            if event.immediate_stopped {
                break;
            }
        }
    }
}
