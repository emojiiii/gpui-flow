//! Standalone note body; has no graph ports by default.
use super::common;
use crate::{FlowTheme, NodeRenderContext};
use gpui::{prelude::*, *};
use gpui_flow_core::{Node, NodeId, Point as ModelPoint};
use serde_json::json;
#[derive(IntoElement)]
pub struct NoteNode {
    pub text: SharedString,
    pub theme: FlowTheme,
    pub zoom: f32,
}
impl NoteNode {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            theme: FlowTheme::default(),
            zoom: 1.,
        }
    }
    pub fn from_node(node: &Node) -> Self {
        Self::new(common::string(node, "text", ""))
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    pub fn zoom(mut self, value: f32) -> Self {
        self.zoom = value;
        self
    }
}
impl RenderOnce for NoteNode {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        common::body(self.theme, self.zoom)
            .text_color(rgb(super::kind_color("note")))
            .child(self.text)
    }
}
pub fn note_node_body(node: &Node, context: NodeRenderContext) -> AnyElement {
    NoteNode::from_node(node)
        .theme(context.theme)
        .zoom(context.zoom)
        .into_any_element()
}
pub(super) fn model(id: NodeId, position: ModelPoint) -> Node {
    let mut node = common::model("note", "Note / 便签", id, position);
    node.data =
        json!({"text":"Drag nodes · connect typed ports\nAll generation in this demo is MOCK"});
    node
}
