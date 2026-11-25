use crate::CanvasCursor;
use gpui_flow_core::*;
use serde_json::Value;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Capability {
    Edit,
    Navigation,
    Selection,
    ClipboardRead,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AccessPolicy {
    pub editable: bool,
    pub navigation: bool,
    pub selection: bool,
    pub clipboard_read: bool,
}
impl Default for AccessPolicy {
    fn default() -> Self {
        Self::editable()
    }
}
impl AccessPolicy {
    pub const fn editable() -> Self {
        Self {
            editable: true,
            navigation: true,
            selection: true,
            clipboard_read: true,
        }
    }
    pub const fn read_only() -> Self {
        Self {
            editable: false,
            ..Self::editable()
        }
    }
    pub const fn locked() -> Self {
        Self {
            editable: false,
            navigation: false,
            selection: false,
            clipboard_read: false,
        }
    }
    pub fn allows(self, capability: Capability) -> bool {
        match capability {
            Capability::Edit => self.editable,
            Capability::Navigation => self.navigation,
            Capability::Selection => self.selection,
            Capability::ClipboardRead => self.clipboard_read,
        }
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SelectionMode {
    Replace,
    Add,
    Toggle,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}
#[derive(Debug, Clone, PartialEq)]
pub struct SnapGuide {
    pub axis: Axis,
    pub position: f32,
}
#[derive(Debug, Clone, PartialEq)]
pub enum CanvasCommand {
    CapturePointer {
        pointer_id: u64,
    },
    ReleasePointer,
    BeginGesture {
        label: String,
    },
    CommitGesture,
    CancelGesture,
    SelectNode {
        node: NodeId,
        mode: SelectionMode,
    },
    SelectEdge {
        edge: EdgeId,
        mode: SelectionMode,
    },
    SelectGroup {
        group: GroupId,
        mode: SelectionMode,
    },
    SelectAll,
    ClearSelection,
    SelectRect {
        rect: Rect,
        additive: bool,
        fully_contained: bool,
    },
    SetPositions {
        nodes: BTreeMap<NodeId, Point>,
        groups: BTreeMap<GroupId, Point>,
    },
    ResizeNode {
        node: NodeId,
        size: Size,
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
    PreviewConnection {
        from: PortRef,
        position: Point,
        hovered: Option<PortRef>,
    },
    CompleteConnection {
        from: PortRef,
    },
    ClearConnection,
    AddNode(Node),
    RemoveNode {
        node: NodeId,
    },
    DeleteSelection,
    GroupSelection {
        title: String,
        padding: f32,
    },
    UngroupSelection,
    CopySelection,
    Paste {
        clipboard: Clipboard,
        offset: Point,
    },
    Duplicate {
        offset: Point,
    },
    Undo,
    Redo,
    SetNodeData {
        node: NodeId,
        data: Value,
    },
    LoadDocument(FlowDocument),
    ImportDocument(FlowDocument),
    ImportGraph(Graph),
}
impl CanvasCommand {
    pub fn capability(&self) -> Option<Capability> {
        Some(match self {
            Self::CapturePointer { .. }
            | Self::ReleasePointer
            | Self::CancelGesture
            | Self::ClearConnection => return None,
            Self::SelectNode { .. }
            | Self::SelectEdge { .. }
            | Self::SelectGroup { .. }
            | Self::SelectAll
            | Self::ClearSelection
            | Self::SelectRect { .. } => Capability::Selection,
            Self::Pan { .. } | Self::Zoom { .. } | Self::Fit { .. } => Capability::Navigation,
            Self::CopySelection => Capability::ClipboardRead,
            _ => Capability::Edit,
        })
    }
}
#[derive(Debug, Clone)]
pub enum CanvasEffect {
    DocumentChanged,
    SelectionChanged,
    ViewportChanged,
    Clipboard(Clipboard),
    ConnectionChanged,
    ConnectionRejected(String),
    Marquee(Option<Rect>),
    Guides(Vec<SnapGuide>),
    Cursor(CanvasCursor),
    RequestPaste,
    RequestCopyDocument,
    RequestFit,
    ToggleGrid,
    RequestSave,
    Repaint,
}
