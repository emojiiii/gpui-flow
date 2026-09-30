//! Text/prompt node payload and independently renderable body.
use super::common;
use crate::{FlowTheme, NodeRenderContext};
use gpui::{prelude::*, *};
use gpui_flow_core::{DataType, Node, NodeId, Point as ModelPoint, Port};
use serde_json::json;
#[derive(IntoElement)]
pub struct TextNode {
    pub text: SharedString,
    pub theme: FlowTheme,
    pub zoom: f32,
}
impl TextNode {
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
impl RenderOnce for TextNode {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        common::body(self.theme, self.zoom).child(self.text)
    }
}
pub fn text_node_body(node: &Node, context: NodeRenderContext) -> AnyElement {
    TextNode::from_node(node)
        .theme(context.theme)
        .zoom(context.zoom)
        .into_any_element()
}
pub(super) fn model(id: NodeId, position: ModelPoint) -> Node {
    let mut node = common::model("text", "Prompt / 文本", id, position);
    node.data = json!({"text":"A quiet landscape, soft light, cinematic composition"});
    node.ports = vec![Port::output("out", "Text", DataType::String)];
    node
}
