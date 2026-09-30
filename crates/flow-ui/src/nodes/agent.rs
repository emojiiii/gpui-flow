//! Agent node body, composed from the reusable generation surface.
use super::{GenerationNode, common};
use crate::{FlowTheme, NodeRenderContext};
use gpui::{prelude::*, *};
use gpui_flow_core::{DataType, Node, NodeId, Point as ModelPoint, Port};
use serde_json::json;
#[derive(IntoElement)]
pub struct AgentNode {
    pub body: GenerationNode,
}
impl AgentNode {
    pub fn new(prompt: impl Into<SharedString>) -> Self {
        Self {
            body: GenerationNode::new(prompt).id("agent-node"),
        }
    }
    pub fn from_node(node: &Node) -> Self {
        Self {
            body: GenerationNode::from_node(node),
        }
    }
    pub fn id(mut self, value: impl Into<ElementId>) -> Self {
        self.body = self.body.id(value);
        self
    }
    pub fn provider(mut self, value: impl Into<SharedString>) -> Self {
        self.body = self.body.provider(value);
        self
    }
    pub fn model(mut self, value: impl Into<SharedString>) -> Self {
        self.body = self.body.model(value);
        self
    }
    pub fn progress(mut self, value: f32) -> Self {
        self.body = self.body.progress(value);
        self
    }
    pub fn status(mut self, value: impl Into<SharedString>) -> Self {
        self.body = self.body.status(value);
        self
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.body = self.body.theme(value);
        self
    }
    pub fn zoom(mut self, value: f32) -> Self {
        self.body = self.body.zoom(value);
        self
    }
    pub fn on_run(
        mut self,
        callback: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.body = self.body.on_run(callback);
        self
    }
}
impl RenderOnce for AgentNode {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.body
    }
}
pub fn agent_node_body(node: &Node, context: NodeRenderContext) -> AnyElement {
    AgentNode::from_node(node)
        .theme(context.theme)
        .zoom(context.zoom)
        .into_any_element()
}
pub(super) fn model(id: NodeId, position: ModelPoint) -> Node {
    let mut node = common::model("agent", "Agent / 智能体", id, position);
    node.data =
        json!({"prompt":"Transform the connected inputs","provider":"mock","model":"offline-demo"});
    node.ports = vec![
        Port::input("in", "Context", DataType::Any),
        Port::output("out", "Result", DataType::Any),
    ];
    node
}
