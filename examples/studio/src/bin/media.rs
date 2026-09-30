//! Generate fixtures with scripts/generate-media-fixtures.py, then run this bin.
use gpui::{prelude::*, *};
use gpui_flow::{
    AudioNode, FlowTheme, MediaView, NodeFrame, VideoNode, core,
    media_backend::{AudioOutput, FfmpegBackend, MediaBackend, PlaybackOptions},
};
use std::path::PathBuf;

struct MediaDemo {
    video: Option<Entity<MediaView>>,
    audio: Option<Entity<MediaView>>,
    errors: Vec<String>,
    muted: bool,
}
impl MediaDemo {
    fn new(video_path: PathBuf, audio_path: PathBuf, muted: bool, cx: &mut Context<Self>) -> Self {
        let backend = FfmpegBackend::default();
        let options = PlaybackOptions {
            audio: if muted {
                AudioOutput::Disabled
            } else {
                AudioOutput::System
            },
            ..Default::default()
        };
        let mut errors = Vec::new();
        let mut open = |path: &std::path::Path| match backend.open(path, options.clone()) {
            Ok(controller) => {
                Some(cx.new(|cx| MediaView::new(controller, cx).preview_height(240.)))
            }
            Err(error) => {
                errors.push(format!("{}: {error}", path.display()));
                None
            }
        };
        let video = open(&video_path);
        let audio = open(&audio_path);
        Self {
            video,
            audio,
            errors,
            muted,
        }
    }
}
impl Render for MediaDemo {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let mut row = div().flex().gap_6();
        if let Some(video) = &self.video {
            row = row.child(
                NodeFrame::new("real-video", "Video · FFmpeg BGRA frames + audio")
                    .size(core::Size::new(500., 410.))
                    .theme(theme)
                    .body(VideoNode::new("").id("video-body").player(video.clone())),
            );
        }
        if let Some(audio) = &self.audio {
            row = row.child(
                NodeFrame::new("real-audio", "Audio · native FFplay/SDL output")
                    .size(core::Size::new(360., 225.))
                    .theme(theme)
                    .body(AudioNode::new("").id("audio-body").player(audio.clone())),
            );
        }
        let mut root=div().size_full().p_6().flex().flex_col().gap_4().bg(rgb(theme.background)).text_color(rgb(theme.foreground)).font_family("sans-serif")
            .child(div().text_xl().child("gpui-flow · decoded media"))
            .child(div().text_sm().text_color(rgb(theme.muted)).child(if self.muted { "Audio output explicitly disabled. Synthetic local video still decodes." } else { "Click Play to start. Native audio output defaults to 25% volume; no autoplay." }))
            .child(row)
            .child(div().text_sm().text_color(rgb(theme.muted)).child("Click the timeline to seek. Pause/seek restart bounded decoder/output processes. This preview backend uses a shared wall clock; device startup latency can cause A/V drift."));
        for error in &self.errors {
            root = root.child(div().text_color(rgb(0xf59696)).child(error.clone()));
        }
        if !self.errors.is_empty() {
            root = root.child("Run: python3 scripts/generate-media-fixtures.py");
        }
        root
    }
}
fn main() {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let muted = args.iter().any(|a| a == "--muted");
    let paths: Vec<_> = args.iter().filter(|a| a.as_str() != "--muted").collect();
    let video = paths.first().map_or_else(
        || PathBuf::from("target/media-fixtures/test-pattern.mkv"),
        PathBuf::from,
    );
    let audio = paths.get(1).map_or_else(
        || PathBuf::from("target/media-fixtures/tone.wav"),
        PathBuf::from,
    );
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            let bounds = Bounds::centered(None, size(px(1000.), px(700.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                move |window, cx| {
                    window.set_window_title("gpui-flow · real media preview");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(|cx| MediaDemo::new(video, audio, muted, cx))
                },
            )
            .expect("open media demo");
            cx.activate(true);
        });
}
