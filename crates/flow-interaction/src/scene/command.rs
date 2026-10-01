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
    SetObjectData {
        id: DisplayId,
        data: Value,
    },
    AddObject {
        object: DisplayObject,
        parent: Option<DisplayId>,
        index: Option<usize>,
    },
    ImportScene(Scene),
    LoadScene(Scene),
    PasteScene {
        scene: Scene,
        offset: Point,
    },
    Undo,
    Redo,
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
            Self::CapturePointer { .. } | Self::ReleasePointer | Self::CancelGesture => {
                return None;
            }
            Self::Select { .. }
            | Self::ClearSelection
            | Self::SelectAll
            | Self::SelectRect { .. } => Capability::Selection,
            Self::Pan { .. } | Self::Zoom { .. } | Self::Fit { .. } => Capability::Navigation,
            _ => Capability::Edit,
        })
    }
}
