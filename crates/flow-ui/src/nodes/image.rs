//! Local image asset preview. Loading/decoding uses GPUI's image element.
use super::common;
use crate::{FlowTheme, NodeRenderContext};
use gpui::{prelude::*, *};
use gpui_flow_core::{DataType, Node, NodeId, Point as ModelPoint, Port};
use serde_json::json;
use std::path::PathBuf;
#[derive(IntoElement)]
pub struct ImageNode {
    pub source: Option<PathBuf>,
    pub alt: SharedString,
    pub caption: SharedString,
    pub theme: FlowTheme,
    pub zoom: f32,
    pub preview_height: f32,
}
impl ImageNode {
    pub fn new(source: impl Into<PathBuf>) -> Self {
        Self::empty().source(source)
    }
    pub fn empty() -> Self {
        Self {
            source: None,
            alt: "Image preview".into(),
            caption: "".into(),
            theme: FlowTheme::default(),
            zoom: 1.,
            preview_height: 80.,
        }
    }
    pub fn from_node(node: &Node) -> Self {
        let source = common::string(node, "source", "");
        let mut image = Self::empty()
            .alt(common::string(node, "alt", "Image preview"))
            .caption(common::string(node, "caption", ""));
        if !source.is_empty() {
            image.source = Some(source.into());
        }
        image
    }
    pub fn source(mut self, value: impl Into<PathBuf>) -> Self {
        self.source = Some(value.into());
        self
    }
    pub fn alt(mut self, value: impl Into<SharedString>) -> Self {
        self.alt = value.into();
        self
    }
    pub fn caption(mut self, value: impl Into<SharedString>) -> Self {
        self.caption = value.into();
        self
    }
    pub fn preview_height(mut self, value: f32) -> Self {
        self.preview_height = value.max(0.);
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
}
impl RenderOnce for ImageNode {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let z = crate::components::valid_zoom(self.zoom);
        let body = common::body(self.theme, z);
        let mut body = if let Some(source) = self.source {
            body.child(
                img(source)
                    .w_full()
                    .h(px(self.preview_height * z))
                    .object_fit(ObjectFit::Contain),
            )
        } else {
            body.child(common::placeholder(
                self.alt,
                self.preview_height,
                super::kind_color("image"),
                z,
            ))
        };
        if !self.caption.is_empty() {
            body = body.child(self.caption);
        }
        body
    }
}
pub fn image_node_body(node: &Node, context: NodeRenderContext) -> AnyElement {
    ImageNode::from_node(node)
        .theme(context.theme)
        .zoom(context.zoom)
        .into_any_element()
}
pub(super) fn model(id: NodeId, position: ModelPoint) -> Node {
    let mut node = common::model("image", "Image / 图片", id, position);
    node.data = json!({"source":"","alt":"Image preview","caption":"Connect your own image asset"});
    node.ports = vec![
        Port::input("in", "Asset", DataType::Named("image".into())),
        Port::output("out", "Asset", DataType::Named("image".into())),
    ];
    node
}
