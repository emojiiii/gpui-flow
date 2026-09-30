use gpui_flow_scene::DisplayId;
/// Opaque engine-scoped identity. Never serialize this into a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ElementHandle {
    pub(crate) engine: u64,
    pub(crate) slot: usize,
    pub(crate) generation: u64,
}
impl ElementHandle {
    pub fn generation(self) -> u64 {
        self.generation
    }
    pub fn engine_id(self) -> u64 {
        self.engine
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstanceState {
    Attached,
    Detached,
    Destroyed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstanceInfo {
    pub handle: ElementHandle,
    pub id: DisplayId,
    pub state: InstanceState,
    /// Changes on reattachment, even though the live handle is retained. Input
    /// sessions must not resume across an unmount/mount between input events.
    pub attachment_epoch: u64,
}
/// A managed detached forest. Its immutable document remains owned by its engine.
/// Dropping this token does not destroy elements; the engine will clean them up
/// on destruction, or the owner can explicitly call `destroy_detached`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetachedSubtree {
    pub(crate) engine: u64,
    pub(crate) token: u64,
    pub(crate) root: DisplayId,
}
impl DetachedSubtree {
    pub fn root_id(&self) -> &DisplayId {
        &self.root
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ListenerId {
    pub(crate) engine: u64,
    pub(crate) raw: gpui_flow_scene::ListenerId,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ObserverId {
    pub(crate) engine: u64,
    pub(crate) sequence: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct DisposerId {
    pub(crate) engine: u64,
    pub(crate) sequence: u64,
}
