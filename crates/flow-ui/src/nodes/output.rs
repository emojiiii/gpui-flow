//! Output body with a replaceable result slot.
use super::common;
use crate::{FlowTheme, NodeRenderContext};
use gpui::{prelude::*, *};
use gpui_flow_core::{DataType, Node, NodeId, Point as ModelPoint, Port};
use serde_json::json;
#[derive(IntoElement)]
pub struct OutputNode {
    pub text: SharedString,
    pub theme: FlowTheme,
    pub zoom: f32,
    content: Option<AnyElement>,
}
impl OutputNode {
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            theme: FlowTheme::default(),
            zoom: 1.,
            content: None,
        }
    }
    pub fn from_node(node: &Node) -> Self {
        Self::new(common::string(node, "text", "Custom node"))
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    pub fn zoom(mut self, value: f32) -> Self {
        self.zoom = value;
        self
    }
    pub fn content(mut self, value: impl IntoElement) -> Self {
        self.content = Some(value.into_any_element());
        self
    }
}
impl RenderOnce for OutputNode {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        common::body(self.theme, self.zoom)
            .child(self.text)
            .children(self.content)
    }
}
pub fn output_node_body(node: &Node, context: NodeRenderContext) -> AnyElement {
    OutputNode::from_node(node)
        .theme(context.theme)
        .zoom(context.zoom)
        .into_any_element()
}
pub(super) fn model(id: NodeId, position: ModelPoint) -> Node {
    let mut node = common::model("output", "Output / 输出", id, position);
    node.data = json!({"text":"Collect generated results here"});
    node.ports = vec![Port::input("in", "Result", DataType::Any).with_capacity(None)];
    node
}
