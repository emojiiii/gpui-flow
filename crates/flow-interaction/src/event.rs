use gpui_flow_core::{EdgeId, GroupId, NodeId, Point, PortRef};
pub use gpui_flow_scene::PointerDevice;
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
impl PointerButton {
    pub fn mask(self) -> u16 {
        match self {
            Self::Left => 1,
            Self::Right => 2,
            Self::Middle => 4,
        }
    }
}
/// Pointer IDs are scoped to a device kind. Touch IDs may never cancel a mouse
/// with the same numeric ID.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct PointerIdentity {
    pub device: PointerDevice,
    pub pointer_id: u64,
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
    /// Fixed source-frame inner Image editor.
    ImageCrop {
        id: String,
        part: gpui_flow_editor::ImageCropPart,
    },
    /// True local geometry endpoint of one built-in straight Line.
    LineEndpoint {
        id: String,
        endpoint: crate::scene::LineEndpoint,
    },
    /// A bounded Path control, local-shear lever or editor pivot.
    GeometryControl {
        id: String,
        control: crate::scene::GeometryControl,
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
    pub device: PointerDevice,
    /// Current held-button mask (primary=1, secondary=2, auxiliary=4).
    /// None retains legacy inference from button and event phase.
    pub buttons: Option<u16>,
    /// Hardware-supplied normalized force; None means unavailable.
    pub pressure: Option<f32>,
    /// Monotonic milliseconds in the router's host clock domain. None keeps
    /// legacy untimed clicks but cannot arm long-press/double-tap recognition.
    pub timestamp_millis: Option<u64>,
}
impl PointerInput {
    pub fn identity(&self) -> PointerIdentity {
        PointerIdentity {
            device: self.device,
            pointer_id: self.pointer_id,
        }
    }
    pub fn held_buttons(&self, released: bool) -> u16 {
        self.buttons.unwrap_or_else(|| {
            if released {
                0
            } else {
                self.button.map_or(0, PointerButton::mask)
            }
        })
    }
    /// A primary touch contact. The host must supply timestamps and optional
    /// hardware pressure, and set buttons=Some(0) on release.
    pub fn touch(pointer_id: u64, position: Point, hit: HitTarget) -> Self {
        Self {
            pointer_id,
            device: PointerDevice::Touch,
            buttons: Some(1),
            ..Self::left(position, hit)
        }
    }
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
    /// Cancel one contact without disturbing another pointer's capture or keys.
    PointerCancelFor(PointerIdentity),
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
