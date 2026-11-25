use crate::{Capability, SelectionMode};
use gpui_flow_core::{Point, Rect, Size};
use gpui_flow_scene::{Affine, DisplayId, DisplayObject, Scene};
use serde_json::Value;
#[derive(Debug, Clone, PartialEq)]
#[expect(
    clippy::large_enum_variant,
    reason = "Owned display objects keep the public command API allocation-free; command queues are short-lived"
)]
pub enum SceneCommand {
    CapturePointer {
        pointer_id: u64,
    },
    /// Capture tied to one live generation/mount. Invalidating this identity
    /// releases input without restoring an obsolete gesture snapshot.
    CapturePointerForTarget {
        pointer_id: u64,
        target: gpui_flow_engine::ElementHandle,
    },
    /// All selection roots, guarded together with their engine revision. The
    /// revision also guards every descendant and ancestor dependency in O(1).
    CapturePointerForSelection {
        pointer_id: u64,
        targets: Vec<gpui_flow_engine::ElementHandle>,
    },
    ReleasePointer,
    BeginGesture,
    CommitGesture,
    CancelGesture,
    Select {
        id: DisplayId,
        mode: SelectionMode,
    },
    ClearSelection,
    SelectAll,
    SelectRect {
        rect: Rect,
        additive: bool,
    },
    TranslateSelection {
        delta: Point,
    },
    MoveSelectionTo {
        anchor: Point,
    },
    SetTransform {
        id: DisplayId,
        transform: Affine,
    },
    /// Named-handle transform. Middleware may normalize the matrix but cannot
    /// retarget this live identity or bypass the bounded native-subtree check.
    SetResizeTransform {
        target: gpui_flow_engine::ElementHandle,
        transform: Affine,
    },
    /// Intrinsic built-in geometry/content patch. Target, kind and Group child
    /// membership are validated after middleware and reactive layout.
    SetBuiltinResize {
        target: gpui_flow_engine::ElementHandle,
        resize: gpui_flow_editor::BuiltinResize,
    },
    /// Changes only a fixed Text box and anchoring translation. Linear terms,
    /// target identity and command kind are preserved through middleware.
    SetTextBoxResize {
        target: gpui_flow_engine::ElementHandle,
        size: Size,
        transform: Affine,
    },
    /// Intrinsic Text font, box, padding and anchoring translation. The actual
    /// text content and dimensionless line-height remain current and unchanged.
    SetTextFontResize {
        target: gpui_flow_engine::ElementHandle,
        resize: gpui_flow_editor::TextFontResize,
    },
    /// Exactly one immutable ordered membership set. Middleware may normalize
    /// matrices, but cannot drop, add, duplicate, reorder or retarget members.
    SetResizeTransforms {
        transforms: Vec<(gpui_flow_engine::ElementHandle, Affine)>,
    },
    /// Uses the same atomic frozen-root batch as resize, with the stricter
    /// vector-only rotation subtree contract. Single selections are allowed.
    SetRotationTransforms {
        transforms: Vec<(gpui_flow_engine::ElementHandle, Affine)>,
    },
    /// Changes only one intrinsic Line coordinate. Middleware cannot change
    /// its live target, endpoint, or command kind.
    SetLineEndpoint {
        target: gpui_flow_engine::ElementHandle,
        endpoint: super::LineEndpoint,
        position: Point,
    },
    /// A single typed geometry field. Middleware may normalize its value but
    /// cannot retarget the instance or switch the control/axis/mode.
    SetGeometry {
        target: gpui_flow_engine::ElementHandle,
        edit: super::GeometryEdit,
    },
    /// Typed crop-only write, guarded against target changes through middleware.
    SetImageCrop {
        target: gpui_flow_engine::ElementHandle,
        crop: Option<gpui_flow_scene::Bounds>,
    },
    RotateSelection {
        degrees: f32,
    },
    ScaleSelection {
        factor: f32,
    },
    DeleteSelection,
    SetProperty {
        id: DisplayId,
        key: gpui_flow_engine::PropertyKey,
        value: Value,
    },
    /// Token-owned property write, subject to the same policy and middleware
    /// as SetProperty. Runtime handles/tokens never enter serialized documents.
    SetSessionProperty {
        token: gpui_flow_editor::EditSessionToken,
        handle: gpui_flow_engine::ElementHandle,
        key: gpui_flow_engine::PropertyKey,
        value: Value,
    },
    ResizeCustomSession {
        token: gpui_flow_editor::EditSessionToken,
        handle: gpui_flow_engine::ElementHandle,
        size: Size,
        anchor: Point,
    },
    SetObjectData {
        id: DisplayId,
        data: Value,
    },
    AddObject {
        object: DisplayObject,
        parent: Option<DisplayId>,
        index: Option<usize>,
    },
    /// Remove one object and its subtree without changing unrelated selection.
    RemoveObject {
        id: DisplayId,
    },
    ImportScene(Scene),
    LoadScene(Scene),
    PasteScene {
        scene: Scene,
        offset: Point,
    },
    Undo,
    Redo,
    /// Atomic normalized navigation result, also used by bounds middleware.
    SetViewport {
        viewport: gpui_flow_core::Viewport,
    },
    Pan {
        delta: Point,
    },
    Zoom {
        position: Point,
        zoom: f32,
    },
    Fit {
        canvas: Size,
        padding: f32,
    },
}
impl SceneCommand {
    pub fn capability(&self) -> Option<Capability> {
        Some(match self {
            Self::CapturePointer { .. }
            | Self::CapturePointerForTarget { .. }
            | Self::CapturePointerForSelection { .. }
            | Self::ReleasePointer
            | Self::CancelGesture => {
                return None;
            }
            Self::Select { .. }
            | Self::ClearSelection
            | Self::SelectAll
            | Self::SelectRect { .. } => Capability::Selection,
            Self::Pan { .. } | Self::Zoom { .. } | Self::Fit { .. } | Self::SetViewport { .. } => {
                Capability::Navigation
            }
            _ => Capability::Edit,
        })
    }
}
