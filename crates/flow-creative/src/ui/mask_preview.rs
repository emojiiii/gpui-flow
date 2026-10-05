use super::*;
use crate::mask::{MaskRasterSize, MaskSource, MaskState};
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_PREVIEWS: usize = 8;
static PREVIEW_GATE: Mutex<()> = Mutex::new(());

#[derive(Clone)]
pub(super) struct MaskContext {
    pub source: MaskSource,
    pub path: PathBuf,
}

#[derive(Default)]
pub(super) struct MaskViews {
    contexts: Mutex<BTreeMap<NodeId, Result<MaskContext, String>>>,
    views: Mutex<BTreeMap<NodeId, Entity<MaskPreview>>>,
    draft: Mutex<Option<(NodeId, MaskState)>>,
}

impl MaskViews {
    pub(super) fn sync(&self, graph: &gpui_flow_core::Graph, project: Option<&LocalProject>) {
        let mut contexts = self.contexts.lock().unwrap();
        contexts.clear();
        for node in graph.nodes.values().filter(|n| n.kind == "mask") {
            let context = project
                .ok_or_else(|| "请在本地项目中编辑遮罩".to_string())
                .and_then(|project| {
                    let source = crate::mask_source(graph, node, project.assets())
                        .map_err(|e| e.to_string())?;
                    let path = project
                        .resolve_asset_path(&source.asset_key)
                        .map_err(|e| e.to_string())?;
                    Ok(MaskContext { source, path })
                });
            contexts.insert(node.id.clone(), context);
        }
        self.views
            .lock()
            .unwrap()
            .retain(|id, _| contexts.contains_key(id));
        let mut draft = self.draft.lock().unwrap();
        if draft
            .as_ref()
            .is_some_and(|(id, _)| !contexts.contains_key(id))
        {
            *draft = None;
        }
    }
    pub(super) fn context(&self, id: &NodeId) -> Result<MaskContext, String> {
        self.contexts
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_else(|| Err("请连接原图".into()))
    }
    pub(super) fn set_draft(&self, draft: Option<(NodeId, MaskState)>) {
        *self.draft.lock().unwrap() = draft;
    }
    pub(super) fn render(&self, node: &Node, cx: &mut App) -> AnyElement {
        let context = match self.context(&node.id) {
            Ok(context) => context,
            Err(error) => return message(error),
        };
        let data = match MaskNodeData::from_value(&node.data) {
            Ok(data) => data,
            Err(error) => return message(error.to_string()),
        };
        let mut body = div()
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb(0xeef0f3))
            .child(img(context.path).size_full().object_fit(ObjectFit::Contain));
        if data.state.source.is_some() && !data.state.source_matches(&context.source) {
            return body
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .left_0()
                        .w_full()
                        .p_2()
                        .bg(rgb(0xffe8b5))
                        .text_xs()
                        .child("原图已变更，请重置遮罩"),
                )
                .into_any_element();
        }
        let draft = self.draft.lock().unwrap();
        let state = draft
            .as_ref()
            .filter(|(id, _)| id == &node.id)
            .map(|(_, s)| s.clone())
            .unwrap_or_else(|| {
                if data.state.source.is_some() {
                    data.state
                } else {
                    MaskState::new(context.source.clone()).expect("validated source")
                }
            });
        drop(draft);
        let mut views = self.views.lock().unwrap();
        if !views.contains_key(&node.id) {
            if views.len() >= MAX_PREVIEWS {
                return body
                    .child(
                        div()
                            .absolute()
                            .bottom_0()
                            .p_2()
                            .text_xs()
                            .child("最多同时预览 8 个遮罩"),
                    )
                    .into_any_element();
            }
            views.insert(node.id.clone(), cx.new(MaskPreview::new));
        }
        let view = views.get(&node.id).unwrap().clone();
        drop(views);
        view.update(cx, |view, cx| {
            view.request(
                PreviewRequest {
                    source: context.source,
                    state,
                },
                cx,
            )
        });
        body = body.child(div().absolute().inset_0().child(view));
        body.into_any_element()
    }
}
fn message(text: String) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_3()
        .bg(rgb(0xeee8ef))
        .text_size(px(12.))
        .text_color(rgb(0x8d7891))
        .child(text)
        .into_any_element()
}

#[derive(Clone, PartialEq)]
struct PreviewRequest {
    source: MaskSource,
    state: MaskState,
}
struct MaskPreview {
    desired: Option<PreviewRequest>,
    completed: Option<PreviewRequest>,
    running: Option<Arc<AtomicBool>>,
    image: Option<Arc<RenderImage>>,
    error: Option<String>,
}
impl MaskPreview {
    fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Self, cx| {
            if let Some(cancelled) = this.running.take() {
                cancelled.store(true, Ordering::Release);
            }
            if let Some(image) = this.image.take() {
                cx.defer(move |cx| cx.drop_image(image, None));
            }
        })
        .detach();
        Self {
            desired: None,
            completed: None,
            running: None,
            image: None,
            error: None,
        }
    }
    fn request(&mut self, request: PreviewRequest, cx: &mut Context<Self>) {
        if self.desired.as_ref() == Some(&request) {
            return;
        }
        if (request.state.strokes.is_empty()
            || self
                .desired
                .as_ref()
                .is_some_and(|old| old.source != request.source))
            && let Some(image) = self.image.take()
        {
            cx.defer(move |cx| cx.drop_image(image, None));
        }
        self.desired = Some(request);
        if let Some(cancelled) = &self.running {
            cancelled.store(true, Ordering::Release);
        }
        self.start(cx);
    }
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() || self.desired == self.completed {
            return;
        }
        let Some(request) = self.desired.clone() else {
            return;
        };
        let cancelled = Arc::new(AtomicBool::new(false));
        self.running = Some(cancelled.clone());
        let work = request.clone();
        let check = cancelled.clone();
        cx.spawn(async move |this, cx| {
            let pixels = cx
                .background_executor()
                .spawn(async move {
                    let _gate = PREVIEW_GATE
                        .lock()
                        .map_err(|_| "遮罩预览任务异常".to_string())?;
                    let raster = crate::mask::rasterize(
                        &work.state,
                        &work.source,
                        MaskRasterSize::Preview { max_edge: 512 },
                        &check,
                    )
                    .map_err(|e| e.to_string())?;
                    let mut rgba = Vec::with_capacity(raster.pixels.len() * 4);
                    for pixel in raster.pixels {
                        rgba.extend_from_slice(&[
                            173,
                            113,
                            209,
                            (u16::from(pixel) * 110 / 255) as u8,
                        ]);
                    }
                    image::RgbaImage::from_raw(raster.width, raster.height, rgba)
                        .ok_or_else(|| "遮罩预览尺寸无效".to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.running = None;
                if !cancelled.load(Ordering::Acquire) && this.desired.as_ref() == Some(&request) {
                    this.completed = Some(request);
                    if let Some(image) = this.image.take() {
                        cx.defer(move |cx| cx.drop_image(image, None));
                    }
                    match pixels {
                        Ok(pixels) => {
                            this.image =
                                Some(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])));
                            this.error = None;
                        }
                        Err(error) => this.error = Some(error),
                    }
                    cx.notify();
                }
                this.start(cx);
            });
        })
        .detach();
    }
}
impl Render for MaskPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .when_some(self.image.clone(), |body, image| {
                body.child(img(image).size_full().object_fit(ObjectFit::Contain))
            })
            .when_some(self.error.clone(), |body, error| {
                body.child(div().p_2().bg(rgb(0xffe8b5)).text_xs().child(error))
            })
    }
}
