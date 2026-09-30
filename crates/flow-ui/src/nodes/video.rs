//! Video node: attach a MediaView for decoded frames/audio, or use metadata/poster callbacks.
use super::common;
use crate::{ControlButton, FlowTheme, MediaView, NodeRenderContext};
use gpui::{prelude::*, *};
use gpui_flow_core::{DataType, Node, NodeId, Point as ModelPoint, Port};
use serde_json::json;
use std::path::PathBuf;
type OpenCallback = Box<dyn Fn(&str, &mut Window, &mut App)>;
#[derive(IntoElement)]
pub struct VideoNode {
    pub id: ElementId,
    pub source: SharedString,
    pub caption: SharedString,
    pub duration: Option<f64>,
    pub poster: Option<PathBuf>,
    pub theme: FlowTheme,
    pub zoom: f32,
    on_open: Option<OpenCallback>,
    player: Option<Entity<MediaView>>,
}
impl VideoNode {
    pub fn new(source: impl Into<SharedString>) -> Self {
        Self {
            id: "video-node".into(),
            source: source.into(),
            caption: "Set a local video source to enable playback".into(),
            duration: None,
            poster: None,
            theme: FlowTheme::default(),
            zoom: 1.,
            on_open: None,
            player: None,
        }
    }
    pub fn from_node(node: &Node) -> Self {
        let mut result = Self::new(common::string(node, "source", ""))
            .id(SharedString::from(node.id.to_string()))
            .caption(common::string(
                node,
                "caption",
                "Set a local video source to enable playback",
            ));
        result.duration = common::number(node, "duration");
        if let Some(poster) = node
            .data
            .get("poster")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            result.poster = Some(poster.into());
        }
        result
    }
    /// Attach a real backend controller through the independently reusable GPUI adapter.
    pub fn player(mut self, player: Entity<MediaView>) -> Self {
        self.player = Some(player);
        self
    }
    pub fn id(mut self, value: impl Into<ElementId>) -> Self {
        self.id = value.into();
        self
    }
    pub fn caption(mut self, value: impl Into<SharedString>) -> Self {
        self.caption = value.into();
        self
    }
    pub fn duration(mut self, seconds: f64) -> Self {
        self.duration = seconds.is_finite().then_some(seconds.max(0.));
        self
    }
    pub fn poster(mut self, value: impl Into<PathBuf>) -> Self {
        self.poster = Some(value.into());
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
    /// Called on explicit activation; your host opens a decoder, player, or preview view.
    pub fn on_open(mut self, callback: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_open = Some(Box::new(callback));
        self
    }
}
impl RenderOnce for VideoNode {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        if let Some(player) = self.player {
            return div().id(self.id).child(player).into_any_element();
        }
        let z = crate::components::valid_zoom(self.zoom);
        let mut body = common::body(self.theme, z);
        body = if let Some(poster) = self.poster {
            body.child(
                img(poster)
                    .w_full()
                    .h(px(65. * z))
                    .object_fit(ObjectFit::Contain),
            )
        } else {
            body.child(common::placeholder(
                "▶  VIDEO · metadata",
                65.,
                super::kind_color("video"),
                z,
            ))
        };
        if let Some(duration) = self.duration {
            body = body.child(format!("{duration:.1} seconds"));
        }
        if !self.caption.is_empty() {
            body = body.child(self.caption);
        }
        if let Some(callback) = self.on_open {
            let source = self.source;
            body = body.child(
                ControlButton::new("open-video", "Open video")
                    .theme(self.theme)
                    .disabled(source.is_empty())
                    .on_click(move |_, window, cx| callback(source.as_ref(), window, cx)),
            );
        }
        div().id(self.id).child(body).into_any_element()
    }
}
pub fn video_node_body(node: &Node, context: NodeRenderContext) -> AnyElement {
    VideoNode::from_node(node)
        .theme(context.theme)
        .zoom(context.zoom)
        .into_any_element()
}
pub(super) fn model(id: NodeId, position: ModelPoint) -> Node {
    let mut node = common::model("video", "Video / 视频", id, position);
    node.size = gpui_flow_core::Size::new(300., 290.);
    node.data = json!({"source":"","caption":"Set a local video source to enable playback"});
    node.ports = vec![
        Port::input("in", "Asset", DataType::Named("video".into())),
        Port::output("out", "Asset", DataType::Named("video".into())),
    ];
    node
}
