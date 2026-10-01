use gpui_flow_core::{EdgeId, GroupId, NodeId, Point, PortRef};
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub meta: bool,
    pub space: bool,
}
impl Modifiers {
    pub fn command(self) -> bool {
        self.control || self.meta
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerButton {
    Left,
    Middle,
    Right,
}
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub enum HitTarget {
    #[default]
    Background,
    Node(NodeId),
    Edge(EdgeId),
    Group(GroupId),
    Port(PortRef),
    ResizeHandle(NodeId),
    Control,
    /// Retained-scene object ID for a SceneEditor-backed plugin host.
    Object(String),
    ObjectResize(String),
    /// True local geometry endpoint of one built-in straight Line.
    LineEndpoint {
        id: String,
        endpoint: crate::scene::LineEndpoint,
    },
    /// One center-pivot rotation handle for the complete selection, including
    /// a sole object. No arbitrary root owns the shared tree-event target.
    SelectionRotateHandle,
    /// Shared world-axis box for the complete current selection; no arbitrary
    /// primary object owns its input or capture.
    SelectionResizeHandle {
        handle: gpui_flow_editor::ResizeHandle,
    },
    /// A named single-selection transform handle. The legacy ObjectResize
    /// retains its bottom-right behavior for existing headless integrations.
    ObjectResizeHandle {
        id: String,
        handle: gpui_flow_editor::ResizeHandle,
    },
}
/// Position is in canvas-local screen pixels. The current viewport is the only
/// source of world coordinates, preventing inconsistent screen/world payloads.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PointerInput {
    pub position: Point,
    pub button: Option<PointerButton>,
    pub modifiers: Modifiers,
    pub hit: HitTarget,
    pub pointer_id: u64,
}
impl PointerInput {
    pub fn left(position: Point, hit: HitTarget) -> Self {
        Self {
            position,
            hit,
            button: Some(PointerButton::Left),
            ..Default::default()
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum KeyCode {
    Space,
    Escape,
    Delete,
    Backspace,
    Character(char),
    Other(String),
}
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasEvent {
    PointerDown(PointerInput),
    PointerMove(PointerInput),
    PointerUp(PointerInput),
    /// Cursor left the canvas. Clear hover paths, retaining an active press or
    /// capture until release/cancel so a drag can continue outside the view.
    PointerLeave,
    PointerCancel,
    FocusLost,
    Wheel {
        position: Point,
        delta: Point,
        modifiers: Modifiers,
    },
    KeyDown {
        key: KeyCode,
        modifiers: Modifiers,
    },
    KeyUp {
        key: KeyCode,
        modifiers: Modifiers,
    },
}
impl CanvasEvent {
    pub fn pointer(&self) -> Option<&PointerInput> {
        match self {
            Self::PointerDown(p) | Self::PointerMove(p) | Self::PointerUp(p) => Some(p),
            _ => None,
        }
    }
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum CanvasCursor {
    #[default]
    Default,
    Crosshair,
    Grab,
    Grabbing,
    Resize,
    ResizeHorizontal,
    ResizeVertical,
    ResizeOtherDiagonal,
    NotAllowed,
}
