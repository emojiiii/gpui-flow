//! Audio metadata. Waveform samples and playback are host supplied, never fabricated.
use super::common;
use crate::{ControlButton, FlowTheme, NodeRenderContext};
use gpui::{prelude::*, *};
use gpui_flow_core::{DataType, Node, NodeId, Point as ModelPoint, Port};
use serde_json::json;
type PlayCallback = Box<dyn Fn(&str, &mut Window, &mut App)>;
#[derive(IntoElement)]
pub struct AudioNode {
    pub id: ElementId,
    pub source: SharedString,
    pub caption: SharedString,
    pub duration: Option<f64>,
    pub waveform: Vec<f32>,
    pub theme: FlowTheme,
    pub zoom: f32,
    on_play: Option<PlayCallback>,
}
impl AudioNode {
    pub fn new(source: impl Into<SharedString>) -> Self {
        Self {
            id: "audio-node".into(),
            source: source.into(),
            caption: "Audio metadata · playback adapter needed".into(),
            duration: None,
            waveform: vec![],
            theme: FlowTheme::default(),
            zoom: 1.,
            on_play: None,
        }
    }
    pub fn from_node(node: &Node) -> Self {
        let mut result = Self::new(common::string(node, "source", ""))
            .id(SharedString::from(node.id.to_string()))
            .caption(common::string(
                node,
                "caption",
                "Audio metadata · playback adapter needed",
            ));
        result.duration = common::number(node, "duration");
        result
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
    pub fn waveform(mut self, samples: impl IntoIterator<Item = f32>) -> Self {
        self.waveform = samples
            .into_iter()
            .map(|x| {
                if x.is_finite() {
                    x.abs().clamp(0., 1.)
                } else {
                    0.
                }
            })
            .collect();
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
    /// An adapter should start/stop its own playback and update the host state.
    pub fn on_play(mut self, callback: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_play = Some(Box::new(callback));
        self
    }
}
impl RenderOnce for AudioNode {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let z = crate::components::valid_zoom(self.zoom);
        let color = super::kind_color("audio");
        let mut body = common::body(self.theme, z);
        if self.waveform.is_empty() {
            body = body.child(common::placeholder("AUDIO · metadata", 45., color, z));
        } else {
            let mut wave = div()
                .h(px(45. * z))
                .flex()
                .items_center()
                .gap(px(3. * z))
                .overflow_hidden();
            for value in self.waveform {
                wave = wave.child(
                    div()
                        .w(px(4. * z))
                        .flex_shrink_0()
                        .h(px((value * 40.).max(2.) * z))
                        .rounded_sm()
                        .bg(rgb(color).alpha(0.7)),
                );
            }
            body = body.child(wave);
        }
        if let Some(duration) = self.duration {
            body = body.child(format!("{duration:.1} seconds"));
        }
        if !self.caption.is_empty() {
            body = body.child(self.caption);
        }
        if let Some(callback) = self.on_play {
            let source = self.source;
            body = body.child(
                ControlButton::new("play-audio", "Play audio")
                    .theme(self.theme)
                    .disabled(source.is_empty())
                    .on_click(move |_, window, cx| callback(source.as_ref(), window, cx)),
            );
        }
        div().id(self.id).child(body)
    }
}
pub fn audio_node_body(node: &Node, context: NodeRenderContext) -> AnyElement {
    AudioNode::from_node(node)
        .theme(context.theme)
        .zoom(context.zoom)
        .into_any_element()
}
pub(super) fn model(id: NodeId, position: ModelPoint) -> Node {
    let mut node = common::model("audio", "Audio / 音频", id, position);
    node.data =
        json!({"source":"","duration":24,"caption":"Audio metadata · playback adapter needed"});
    node.ports = vec![
        Port::input("in", "Asset", DataType::Named("audio".into())),
        Port::output("out", "Asset", DataType::Named("audio".into())),
    ];
    node
}
