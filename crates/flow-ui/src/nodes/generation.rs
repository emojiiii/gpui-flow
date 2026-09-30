//! Generation body with provider metadata and optional host-controlled execution.
use super::common;
use crate::{ControlButton, FlowTheme, NodeRenderContext, badge, progress_bar};
use gpui::{prelude::*, *};
use gpui_flow_core::{DataType, Node, NodeId, Point as ModelPoint, Port};
use serde_json::json;
type RunCallback = Box<dyn Fn(&ClickEvent, &mut Window, &mut App)>;
#[derive(IntoElement)]
pub struct GenerationNode {
    pub id: ElementId,
    pub prompt: SharedString,
    pub provider: SharedString,
    pub model: SharedString,
    pub progress: Option<f32>,
    pub status: Option<SharedString>,
    pub theme: FlowTheme,
    pub zoom: f32,
    on_run: Option<RunCallback>,
}
impl GenerationNode {
    pub fn new(prompt: impl Into<SharedString>) -> Self {
        Self {
            id: "generation-node".into(),
            prompt: prompt.into(),
            provider: "".into(),
            model: "".into(),
            progress: None,
            status: None,
            theme: FlowTheme::default(),
            zoom: 1.,
            on_run: None,
        }
    }
    pub fn from_node(node: &Node) -> Self {
        Self::new(common::string(node, "prompt", ""))
            .id(SharedString::from(node.id.to_string()))
            .provider(common::string(node, "provider", ""))
            .model(common::string(node, "model", ""))
    }
    pub fn id(mut self, value: impl Into<ElementId>) -> Self {
        self.id = value.into();
        self
    }
    pub fn provider(mut self, value: impl Into<SharedString>) -> Self {
        self.provider = value.into();
        self
    }
    pub fn model(mut self, value: impl Into<SharedString>) -> Self {
        self.model = value.into();
        self
    }
    pub fn progress(mut self, value: f32) -> Self {
        self.progress = Some(value);
        self
    }
    pub fn status(mut self, value: impl Into<SharedString>) -> Self {
        self.status = Some(value.into());
        self
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    pub fn zoom(mut self, value: f32) -> Self {
        self.zoom = value;
        self
    }
    /// The host owns execution, cancellation and credentials. This body makes no requests.
    pub fn on_run(
        mut self,
        callback: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_run = Some(Box::new(callback));
        self
    }
}
impl RenderOnce for GenerationNode {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut body = common::body(self.theme, self.zoom);
        if !self.provider.is_empty() {
            body = body.child(badge(
                if self.provider.as_ref() == "mock" {
                    "MOCK / OFFLINE".into()
                } else {
                    self.provider
                },
                super::kind_color("generation"),
            ));
        }
        if !self.model.is_empty() {
            body = body.child(self.model);
        }
        body = body.child(self.prompt);
        if let Some(progress) = self.progress {
            body = body.child(progress_bar(progress, self.theme));
        }
        if let Some(status) = self.status {
            body = body.child(status);
        }
        if let Some(callback) = self.on_run {
            body = body.child(
                ControlButton::new("run-node", "Run")
                    .theme(self.theme)
                    .on_click(callback),
            );
        }
        div().id(self.id).child(body)
    }
}
pub fn generation_node_body(node: &Node, context: NodeRenderContext) -> AnyElement {
    GenerationNode::from_node(node)
        .theme(context.theme)
        .zoom(context.zoom)
        .into_any_element()
}
pub(super) fn model(id: NodeId, position: ModelPoint) -> Node {
    let mut node = common::model("generation", "Generate / 生成", id, position);
    node.data =
        json!({"prompt":"Transform the connected inputs","provider":"mock","model":"offline-demo"});
    node.ports = vec![
        Port::input("prompt", "Prompt", DataType::String),
        Port::input("reference", "Reference", DataType::Named("image".into())),
        Port::output("out", "Image", DataType::Named("image".into())),
    ];
    node
}
