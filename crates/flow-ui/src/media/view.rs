use crate::{ControlButton, FlowTheme};
use gpui::{prelude::*, *};
use gpui_flow_media::{
    AudioOutput, MediaBackend, MediaController, MediaError, PlaybackOptions, PlaybackSnapshot,
    PlaybackState,
};
use std::{path::Path, sync::Arc, time::Duration};

pub struct MediaView {
    pub controller: Arc<dyn MediaController>,
    pub theme: FlowTheme,
    pub preview_height: f32,
    snapshot: PlaybackSnapshot,
    image: Option<Arc<RenderImage>>,
    frame_sequence: Option<u64>,
    timeline: Bounds<Pixels>,
    command_error: Option<String>,
}
impl MediaView {
    pub fn open(
        backend: &dyn MediaBackend,
        source: &Path,
        options: PlaybackOptions,
        cx: &mut Context<Self>,
    ) -> Result<Self, MediaError> {
        Ok(Self::new(backend.open(source, options)?, cx))
    }
    pub fn new(controller: Arc<dyn MediaController>, cx: &mut Context<Self>) -> Self {
        let snapshot = controller.snapshot();
        cx.on_release(|this: &mut Self, cx| {
            this.controller.shutdown();
            if let Some(image) = this.image.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            controller,
            theme: FlowTheme::default(),
            preview_height: 100.,
            snapshot,
            image: None,
            frame_sequence: None,
            timeline: Bounds::default(),
            command_error: None,
        }
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    pub fn preview_height(mut self, value: f32) -> Self {
        if value.is_finite() {
            self.preview_height = value.max(0.);
        }
        self
    }
    pub fn snapshot(&self) -> &PlaybackSnapshot {
        &self.snapshot
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let snapshot = self.controller.snapshot();
        let sequence = snapshot.frame.as_ref().map(|frame| frame.sequence);
        if snapshot.revision == self.snapshot.revision && sequence == self.frame_sequence {
            return;
        }
        if sequence != self.frame_sequence {
            if let Some(image) = self.image.take() {
                cx.drop_image(image, None);
            }
            self.image = snapshot.frame.as_ref().and_then(|frame| {
                let buffer =
                    image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra.to_vec())?;
                Some(Arc::new(RenderImage::new(vec![image::Frame::new(buffer)])))
            });
            self.frame_sequence = sequence;
        }
        self.snapshot = snapshot;
        cx.notify();
    }
    fn report(&mut self, result: Result<(), MediaError>, cx: &mut Context<Self>) {
        self.command_error = result.err().map(|error| error.to_string());
        cx.notify();
    }
}
impl Render for MediaView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = self.theme;
        let state = self.snapshot.state;
        let has_video = self.snapshot.metadata.as_ref().is_some_and(|m| m.has_video);
        let duration = self.snapshot.metadata.as_ref().map_or(0., |m| m.duration);
        let progress = if duration > 0. {
            (self.snapshot.position / duration).clamp(0., 1.) as f32
        } else {
            0.
        };
        let weak = cx.entity().downgrade();
        let mut body = div()
            .id("media-view")
            .w_full()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .text_xs()
            .text_color(rgb(theme.muted))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if has_video || state == PlaybackState::Loading {
            let preview = div()
                .w_full()
                .h(px(self.preview_height))
                .bg(rgb(0x05070b))
                .rounded_md()
                .overflow_hidden()
                .flex()
                .items_center()
                .justify_center();
            body = body.child(if let Some(image) = &self.image {
                preview.child(
                    img(image.clone())
                        .w_full()
                        .h_full()
                        .object_fit(ObjectFit::Contain),
                )
            } else {
                preview.child(if state == PlaybackState::Loading {
                    "Loading media…"
                } else {
                    "No decoded frame"
                })
            });
        } else {
            body = body.child(div().text_color(rgb(theme.accent)).child(
                if self.snapshot.audio_output == AudioOutput::Disabled {
                    "AUDIO · output disabled"
                } else {
                    "AUDIO · native output"
                },
            ));
        }
        body = body.child(
            div()
                .flex()
                .justify_between()
                .child(format!("{:?}", state))
                .child(format!("{:.1} / {:.1} s", self.snapshot.position, duration)),
        );
        body = body.child(
            div()
                .id("media-timeline")
                .relative()
                .w_full()
                .h(px(9.))
                .rounded_full()
                .bg(rgb(theme.border))
                .cursor_pointer()
                .child(
                    div()
                        .h_full()
                        .w(relative(progress))
                        .rounded_full()
                        .bg(rgb(theme.accent)),
                )
                .child(
                    canvas(
                        move |bounds, _, cx| {
                            let _ = weak.update(cx, |this, _| this.timeline = bounds);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, _, cx| {
                        cx.stop_propagation();
                        let width: f32 = this.timeline.size.width.into();
                        if duration > 0. && width > 0. {
                            let x: f32 = (event.position.x - this.timeline.origin.x).into();
                            let result = this
                                .controller
                                .seek((x / width).clamp(0., 1.) as f64 * duration);
                            this.report(result, cx);
                        }
                    }),
                ),
        );
        let disabled = state == PlaybackState::Loading || self.snapshot.metadata.is_none();
        body = body.child(
            div()
                .flex()
                .gap_1()
                .child(
                    ControlButton::new(
                        "media-play",
                        if state == PlaybackState::Playing {
                            "Ⅱ"
                        } else {
                            "▶"
                        },
                    )
                    .theme(theme)
                    .disabled(disabled)
                    .on_click(cx.listener(|this, _, _, cx| {
                        let result = if this.snapshot.state == PlaybackState::Playing {
                            this.controller.pause()
                        } else {
                            this.controller.play()
                        };
                        this.report(result, cx);
                    })),
                )
                .child(
                    ControlButton::new("media-back", "−5s")
                        .theme(theme)
                        .disabled(disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let result =
                                this.controller.seek((this.snapshot.position - 5.).max(0.));
                            this.report(result, cx);
                        })),
                )
                .child(
                    ControlButton::new("media-forward", "+5s")
                        .theme(theme)
                        .disabled(disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let result = this.controller.seek(this.snapshot.position + 5.);
                            this.report(result, cx);
                        })),
                )
                .child(
                    ControlButton::new("media-stop", "■")
                        .theme(theme)
                        .disabled(disabled)
                        .on_click(cx.listener(|this, _, _, cx| {
                            let result = this.controller.stop();
                            this.report(result, cx);
                        })),
                )
                .child(
                    ControlButton::new(
                        "media-mute",
                        if self.snapshot.volume == 0. {
                            "25%"
                        } else {
                            "Mute"
                        },
                    )
                    .theme(theme)
                    .disabled(
                        disabled
                            || self.snapshot.audio_output == AudioOutput::Disabled
                            || !self.snapshot.metadata.as_ref().is_some_and(|m| m.has_audio),
                    )
                    .on_click(cx.listener(|this, _, _, cx| {
                        let volume = if this.snapshot.volume == 0. { 0.25 } else { 0. };
                        let result = this.controller.set_volume(volume);
                        this.report(result, cx);
                    })),
                ),
        );
        if let Some(error) = self
            .command_error
            .as_ref()
            .cloned()
            .or_else(|| self.snapshot.error.as_ref().map(ToString::to_string))
        {
            body = body.child(
                div()
                    .text_color(rgb(0xf59696))
                    .child(error.chars().take(300).collect::<String>()),
            );
        }
        body
    }
}
