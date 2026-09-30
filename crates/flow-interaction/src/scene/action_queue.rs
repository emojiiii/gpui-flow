//! Bounded deferred actions let listeners request edits without re-entering the
//! document during capture/bubble dispatch. Instance identity scopes each action.
use super::SceneCommand;
use gpui_flow_engine::ElementHandle;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
#[derive(Clone)]
pub struct SceneActionSender {
    sender: SyncSender<(ElementHandle, SceneCommand)>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionQueueError {
    Full,
    Closed,
}
impl SceneActionSender {
    pub fn enqueue(
        &self,
        origin: ElementHandle,
        command: SceneCommand,
    ) -> Result<(), ActionQueueError> {
        self.sender
            .try_send((origin, command))
            .map_err(|e| match e {
                TrySendError::Full(_) => ActionQueueError::Full,
                TrySendError::Disconnected(_) => ActionQueueError::Closed,
            })
    }
}
pub(super) struct SceneActionQueue {
    pub sender: SceneActionSender,
    pub receiver: Receiver<(ElementHandle, SceneCommand)>,
}
impl Default for SceneActionQueue {
    fn default() -> Self {
        let (sender, receiver) = sync_channel(256);
        Self {
            sender: SceneActionSender { sender },
            receiver,
        }
    }
}
