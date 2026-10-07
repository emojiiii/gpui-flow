//! Project-owned media objects. The existing decoder owns its bounded workers;
//! this adapter owns images and keeps transport controls outside the media.
use super::*;
use gpui_flow_media::{
    MediaBackend, MediaController, PlaybackOptions, PlaybackSnapshot, PlaybackState,
};
use std::time::Duration;

const MAX_MEDIA_VIEWS: usize = 4;
#[derive(Clone, Copy)]
pub(super) enum MediaAction {
    Toggle,
    Seek(f64),
    Mute,
    SeekFraction(f64),
}

struct Entry {
    asset: String,
    path: PathBuf,
    view: Entity<MediaObjectView>,
}
#[derive(Default)]
pub(super) struct MediaViews {
    entries: Mutex<BTreeMap<NodeId, Entry>>,
    owner: Mutex<Option<WeakEntity<CreativeCanvas>>>,
    primary_selection: Mutex<Option<NodeId>>,
}
impl MediaViews {
    pub(super) fn set_owner(&self, owner: WeakEntity<CreativeCanvas>) {
        *self.owner.lock().unwrap() = Some(owner);
    }
    pub(super) fn retain(&self, canvas: &Entity<FlowCanvas>, cx: &mut App) {
        let view = canvas.read(cx);
        let sources = view
            .editor
            .graph()
            .nodes
            .values()
            .filter(|n| gpui_flow_core::node_is_visible(view.editor.graph(), &n.id))
            .filter(|n| {
                matches!(
                    n.kind.as_str(),
                    "video"
                        | "audio"
                        | "generate_video"
                        | "generate_audio"
                        | "process_video"
                        | "process_audio"
                ) && (crate::processing::ProcessingKind::from_node_kind(&n.kind).is_none()
                    || crate::processing_node::processing_output(view.editor.graph(), n).is_ok())
            })
            .map(|n| {
                (
                    n.id.clone(),
                    n.data["asset"].as_str().unwrap_or_default().to_owned(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        let selected = view.editor.selection.nodes.clone();
        *self.primary_selection.lock().unwrap() = if selected.len() == 1 {
            selected.first().cloned()
        } else {
            None
        };
        let mut entries = self.entries.lock().unwrap();
        entries.retain(|id, entry| sources.get(id) == Some(&entry.asset));
        for (id, entry) in entries.iter() {
            if !selected.contains(id) {
                entry.view.update(cx, |view, cx| {
                    if view.play_requested
                        || view.controller.snapshot().state == PlaybackState::Playing
                    {
                        let result = view.controller.pause();
                        if result.is_ok() {
                            view.play_requested = false;
                        }
                        view.report(result, cx);
                    }
                });
            }
        }
    }
    pub(super) fn render(
        &self,
        node: &Node,
        path: &std::path::Path,
        selected: bool,
        cx: &mut App,
    ) -> AnyElement {
        let asset = node.data["asset"].as_str().unwrap_or_default();
        let mut entries = self.entries.lock().unwrap();
        if entries
            .get(&node.id)
            .is_some_and(|entry| entry.path != path || entry.asset != asset)
        {
            entries.remove(&node.id);
        }
        if !entries.contains_key(&node.id) {
            if entries.len() >= MAX_MEDIA_VIEWS {
                if selected && self.primary_selection.lock().unwrap().as_ref() == Some(&node.id) {
                    if let Some(id) = entries.keys().next().cloned() {
                        entries.remove(&id);
                    }
                } else {
                    return message("选中以预览本地媒体");
                }
            }
            let options = PlaybackOptions {
                autoplay: false,
                max_width: 640,
                max_height: 360,
                ..PlaybackOptions::default()
            };
            let controller = match creative_media_backend().open(path, options) {
                Ok(controller) => controller,
                Err(error) => return message(format!("媒体无法打开：{error}")),
            };
            let owner = self.owner.lock().unwrap().clone();
            let audio = matches!(
                node.kind.as_str(),
                "audio" | "generate_audio" | "process_audio"
            );
            let view = cx.new(|cx| MediaObjectView::new(controller, owner, audio, path.into(), cx));
            entries.insert(
                node.id.clone(),
                Entry {
                    asset: asset.into(),
                    path: path.into(),
                    view,
                },
            );
        }
        entries[&node.id].view.clone().into_any_element()
    }
    pub(super) fn snapshot(&self, node: &NodeId, cx: &App) -> Option<PlaybackSnapshot> {
        self.entries
            .lock()
            .unwrap()
            .get(node)
            .map(|entry| entry.view.read(cx).snapshot.clone())
    }
    pub(super) fn command(&self, node: &NodeId, action: MediaAction, cx: &mut App) {
        let entries = self.entries.lock().unwrap();
        let Some(entry) = entries.get(node) else {
            return;
        };
        entry.view.update(cx, |view, cx| {
            let snapshot = view.controller.snapshot();
            let result = match action {
                MediaAction::Toggle if view.play_requested => {
                    let result = view.controller.pause();
                    if result.is_ok() {
                        view.play_requested = false;
                    }
                    result
                }
                MediaAction::Toggle => {
                    let result = view.controller.play();
                    if result.is_ok() {
                        view.play_requested = true;
                    }
                    result
                }
                MediaAction::Seek(delta) => {
                    view.controller.seek((snapshot.position + delta).max(0.))
                }
                MediaAction::SeekFraction(fraction) => {
                    if !fraction.is_finite()
                        || !snapshot
                            .metadata
                            .as_ref()
                            .is_some_and(|m| m.duration.is_finite() && m.duration > 0.)
                    {
                        return;
                    }
                    view.controller
                        .seek(snapshot.metadata.as_ref().unwrap().duration * fraction.clamp(0., 1.))
                }
                MediaAction::Mute => {
                    view.controller
                        .set_volume(if snapshot.volume == 0. { 0.25 } else { 0. })
                }
            };
            view.report(result, cx);
        });
    }
}

fn message(text: impl Into<SharedString>) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_4()
        .bg(rgb(0xeff1f5))
        .text_size(px(12.))
        .text_color(rgb(0x868e9e))
        .child(text.into())
        .into_any_element()
}

struct MediaObjectView {
    controller: Arc<dyn MediaController>,
    owner: Option<WeakEntity<CreativeCanvas>>,
    snapshot: PlaybackSnapshot,
    image: Option<Arc<RenderImage>>,
    frame_sequence: Option<u64>,
    command_error: Option<String>,
    // Keep an accepted Play request until a later snapshot arrives so a rapid
    // selection change can enqueue Pause behind it instead of missing it.
    play_requested: bool,
    audio: bool,
    waveform: Option<media_waveform::WaveformLoad>,
}
impl MediaObjectView {
    fn new(
        controller: Arc<dyn MediaController>,
        owner: Option<WeakEntity<CreativeCanvas>>,
        audio: bool,
        source: PathBuf,
        cx: &mut Context<Self>,
    ) -> Self {
        let snapshot = controller.snapshot();
        cx.on_release(|this: &mut Self, cx| {
            if let Some(waveform) = &this.waveform {
                waveform.cancel();
            }
            this.controller.shutdown();
            if std::env::args().any(|arg| arg == "--trace") {
                eprintln!(
                    "creative-media: released audio={} remaining_processes={}",
                    this.audio,
                    this.controller.snapshot().process_ids.len()
                );
            }
            if let Some(image) = this.image.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(33))
                    .await;
                if this.update(cx, |this, cx| this.refresh(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            controller,
            owner,
            snapshot,
            image: None,
            frame_sequence: None,
            command_error: None,
            play_requested: false,
            audio,
            waveform: audio.then(|| media_waveform::WaveformLoad::new(source)),
        }
    }
    fn refresh(&mut self, cx: &mut Context<Self>) {
        if let Some((path, cancelled, permit)) = self.waveform.as_mut().and_then(|w| w.try_start())
        {
            cx.spawn(async move |this, cx| {
                let result = cx
                    .background_executor()
                    .spawn(async move {
                        let _permit = permit;
                        creative_media_backend().waveform(&path, &cancelled)
                    })
                    .await;
                let _ = this.update(cx, |this, cx| {
                    if std::env::args().any(|arg| arg == "--trace") {
                        match &result {
                            Ok(waveform) => eprintln!(
                                "creative-waveform: bins={} duration={:.3}",
                                waveform.peaks.len(),
                                waveform.duration_seconds
                            ),
                            Err(error) => eprintln!("creative-waveform: {error}"),
                        }
                    }
                    if let Some(waveform) = &mut this.waveform {
                        waveform.complete(result);
                    }
                    cx.notify();
                });
            })
            .detach();
        }
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
        if snapshot.state != self.snapshot.state && std::env::args().any(|arg| arg == "--trace") {
            eprintln!(
                "creative-media: audio={} state={:?} position={:.2} processes={}",
                self.audio,
                snapshot.state,
                snapshot.position,
                snapshot.process_ids.len()
            );
        }
        if matches!(snapshot.state, PlaybackState::Ended | PlaybackState::Error) {
            self.play_requested = false;
        }
        self.snapshot = snapshot;
        cx.notify();
        if let Some(owner) = &self.owner {
            let _ = owner.update(cx, |_, cx| cx.notify());
        }
    }
    fn report(&mut self, result: Result<(), gpui_flow_media::MediaError>, cx: &mut Context<Self>) {
        self.command_error = result.err().map(|e| e.to_string());
        cx.notify();
    }
}
impl Render for MediaObjectView {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        if let Some(error) = self
            .command_error
            .as_ref()
            .cloned()
            .or_else(|| self.snapshot.error.as_ref().map(ToString::to_string))
        {
            return message(format!("媒体预览失败：{error}"));
        }
        if self.audio {
            let duration = self.snapshot.metadata.as_ref().map_or_else(
                || "正在读取本地音频…".to_string(),
                |m| format!("{} / {}", clock(self.snapshot.position), clock(m.duration)),
            );
            return div()
                .size_full()
                .min_h_0()
                .overflow_hidden()
                .bg(rgb(0xf5efe7))
                .flex()
                .flex_col()
                .p_4()
                .gap_3()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap_2()
                                .child(chrome::icon("music", 16.).text_color(rgb(0xa77b47)))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .text_color(rgb(0x8c7963))
                                        .child("本地音频"),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgb(0x8c7963))
                                .child(duration),
                        ),
                )
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .overflow_hidden()
                        .when_some(self.waveform.as_ref(), |view, waveform| {
                            view.child(waveform.render(self.snapshot.position))
                        }),
                )
                .into_any_element();
        }
        if let Some(image) = &self.image {
            return img(image.clone())
                .size_full()
                .object_fit(ObjectFit::Contain)
                .bg(rgb(0x15171d))
                .into_any_element();
        }
        if self.snapshot.state == PlaybackState::Ended {
            return message("播放已结束，点击时间轴可返回任意位置");
        }
        message("正在解码本地视频首帧…")
    }
}
pub(super) fn clock(seconds: f64) -> String {
    let seconds = seconds.max(0.) as u64;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

impl CreativeCanvas {
    pub(super) fn media_controls(&self, node: &Node, cx: &mut Context<Self>) -> Div {
        let snapshot = self.media_views.snapshot(&node.id, cx);
        let ready = snapshot.as_ref().is_some_and(|s| {
            s.metadata.is_some() && s.state != PlaybackState::Loading && s.error.is_none()
        });
        let playing = snapshot
            .as_ref()
            .is_some_and(|s| s.state == PlaybackState::Playing);
        let muted = snapshot.as_ref().is_some_and(|s| s.volume == 0.);
        let has_audio = snapshot
            .as_ref()
            .is_some_and(|s| s.metadata.as_ref().is_some_and(|m| m.has_audio));
        let position = snapshot.as_ref().map_or(0., |s| s.position);
        let duration = snapshot
            .as_ref()
            .and_then(|s| s.metadata.as_ref())
            .map_or(0., |m| m.duration);
        let transport = div()
            .flex()
            .items_center()
            .gap_1()
            .child(self.icon_button(
                "media-play",
                if playing { "pause" } else { "play" },
                if playing {
                    "暂停"
                } else {
                    "播放本地媒体"
                },
                Action::Media(MediaAction::Toggle),
                !ready,
                cx,
            ))
            .child(self.icon_button(
                "media-back",
                "rotate-ccw",
                "后退5秒",
                Action::Media(MediaAction::Seek(-5.)),
                !ready,
                cx,
            ))
            .child(
                div()
                    .min_w(px(92.))
                    .text_center()
                    .text_size(px(12.))
                    .text_color(rgb(0x727987))
                    .child(format!("{} / {}", clock(position), clock(duration))),
            )
            .child(self.icon_button(
                "media-forward",
                "rotate-cw",
                "前进5秒",
                Action::Media(MediaAction::Seek(5.)),
                !ready,
                cx,
            ))
            .child(self.icon_button(
                "media-mute",
                if muted { "volume-x" } else { "volume-2" },
                "切换静音",
                Action::Media(MediaAction::Mute),
                !ready || !has_audio,
                cx,
            ));
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(transport)
            .child(self.media_timeline(node, ready, position, duration, cx))
    }

    fn media_timeline(
        &self,
        node: &Node,
        ready: bool,
        position: f64,
        duration: f64,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        use std::{cell::Cell, rc::Rc};
        let bounds = Rc::new(Cell::new(None::<Bounds<Pixels>>));
        let paint_bounds = bounds.clone();
        let progress = media_waveform::progress(position, duration) as f32;
        let id = node.id.clone();
        let asset = node.data["asset"].as_str().unwrap_or_default().to_string();
        let ready = ready && duration.is_finite() && duration > 0.;
        let audio = matches!(
            node.kind.as_str(),
            "audio" | "generate_audio" | "process_audio"
        );
        let (track, fill_color, thumb) = if audio {
            (0xe7dacc, 0xad7b3e, 0x80541f)
        } else {
            (0xdde5f0, 0x5489ce, 0x316db8)
        };
        div()
            .id("media-timeline")
            .relative()
            .w_full()
            .h(px(22.))
            .occlude()
            .when(ready, |view| view.cursor_pointer())
            .child(
                canvas(
                    move |actual, _, _| paint_bounds.set(Some(actual)),
                    move |actual, _, window, _| {
                        let width: f32 = actual.size.width.into();
                        window.paint_quad(fill(
                            Bounds::new(
                                actual.origin + point(px(0.), px(9.)),
                                size(px(width), px(4.)),
                            ),
                            rgb(track),
                        ));
                        if progress > 0. {
                            window.paint_quad(fill(
                                Bounds::new(
                                    actual.origin + point(px(0.), px(9.)),
                                    size(px(width * progress), px(4.)),
                                ),
                                rgb(fill_color),
                            ));
                        }
                        if ready {
                            window.paint_quad(fill(
                                Bounds::new(
                                    actual.origin
                                        + point(
                                            px((width * progress - 2.)
                                                .clamp(0., (width - 4.).max(0.))),
                                            px(6.),
                                        ),
                                    size(px(4.), px(10.)),
                                ),
                                rgb(thumb),
                            ));
                        }
                    },
                )
                .absolute()
                .size_full(),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                    cx.stop_propagation();
                    if !ready
                        || !this.selected(cx).is_some_and(|node| {
                            node.id == id && node.data["asset"].as_str() == Some(asset.as_str())
                        })
                    {
                        return;
                    }
                    let Some(bounds) = bounds.get() else {
                        return;
                    };
                    let Some(fraction) = media_waveform::seek_fraction(
                        event.position.x.into(),
                        bounds.origin.x.into(),
                        bounds.size.width.into(),
                    ) else {
                        return;
                    };
                    this.media_views
                        .command(&id, MediaAction::SeekFraction(fraction), cx);
                    this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                }),
            )
    }
}
