//! One cancellable artwork job, a coalesced recent-project queue and a bounded
//! memory cache. Produces covers only; never saves/migrates project documents.
use super::*;
use crate::frame_text::FrameTextBackend;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_PREVIEWS: usize = 16;
static PREVIEW_GATE: Mutex<()> = Mutex::new(());

pub(super) struct ProjectPreviews {
    serial: u64,
    store: Option<ProjectStore>,
    backend: Option<FrameTextBackend>,
    pending: VecDeque<String>,
    running: Option<Arc<AtomicBool>>,
    cache: BTreeMap<String, PreviewResult>,
}

pub(super) struct PreviewResult {
    pub image: Option<Arc<RenderImage>>,
    pub thumbnail: Option<PathBuf>,
    pub label: &'static str,
}

impl ProjectPreviews {
    pub(super) fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|this: &mut Self, cx| {
            if let Some(cancel) = &this.running {
                cancel.store(true, Ordering::Release);
            }
            this.release_cache(cx);
        })
        .detach();
        Self {
            serial: 0,
            store: None,
            backend: None,
            pending: VecDeque::new(),
            running: None,
            cache: BTreeMap::new(),
        }
    }

    fn release_cache(&mut self, cx: &mut App) {
        for result in std::mem::take(&mut self.cache).into_values() {
            if let Some(image) = result.image {
                cx.defer(move |cx| cx.drop_image(image, None));
            }
        }
    }

    pub(super) fn request(
        &mut self,
        store: ProjectStore,
        ids: Vec<String>,
        backend: Option<FrameTextBackend>,
        cx: &mut Context<Self>,
    ) {
        self.cancel(cx);
        self.store = Some(store);
        self.backend = backend;
        self.pending = ids.into_iter().take(MAX_PREVIEWS).collect();
        self.start(cx);
    }

    pub(super) fn cancel(&mut self, cx: &mut Context<Self>) {
        self.serial = self.serial.wrapping_add(1);
        if let Some(cancel) = &self.running {
            cancel.store(true, Ordering::Release);
        }
        self.pending.clear();
        self.release_cache(cx);
    }

    pub(super) fn result(&self, id: &str) -> Option<&PreviewResult> {
        self.cache.get(id)
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        if self.running.is_some() {
            return;
        }
        let Some(id) = self.pending.pop_front() else {
            return;
        };
        let Some(store) = self.store.clone() else {
            return;
        };
        let backend = self.backend.clone();
        let serial = self.serial;
        let cancelled = Arc::new(AtomicBool::new(false));
        self.running = Some(cancelled.clone());
        let task_cancel = cancelled.clone();
        let task_id = id.clone();
        cx.spawn(async move |this, cx| {
            let (result, thumbnail) = cx
                .background_executor()
                .spawn(async move {
                    let thumbnail = if task_cancel.load(Ordering::Acquire) {
                        None
                    } else {
                        store.thumbnail_path(&task_id).ok().flatten()
                    };
                    let result = (|| {
                        let _gate = PREVIEW_GATE
                            .lock()
                            .map_err(|_| "Project preview is unavailable".to_string())?;
                        if task_cancel.load(Ordering::Acquire) {
                            return Err("Preview cancelled".into());
                        }
                        let project = store
                            .read_preview_project_cancellable(
                                &task_id,
                                &standard_registry(),
                                &task_cancel,
                            )
                            .map_err(|e| e.to_string())?;
                        let preview = crate::project_preview::prepare_project_preview(
                            &project.document().graph,
                            project.assets(),
                        )?;
                        let bytes = preview.export.encode_with_text_backend(
                            &project,
                            backend.as_ref(),
                            &task_cancel,
                        )?;
                        if task_cancel.load(Ordering::Acquire) {
                            return Err("Preview cancelled".into());
                        }
                        let mut pixels =
                            image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
                                .map_err(|error| error.to_string())?
                                .into_rgba8();
                        let [max_width, max_height] = crate::project_preview::PROJECT_PREVIEW_SIZE;
                        if pixels.width() > max_width || pixels.height() > max_height {
                            return Err("Preview exceeds its pixel budget".into());
                        }
                        for pixel in pixels.pixels_mut() {
                            pixel.0.swap(0, 2);
                        }
                        Ok::<_, String>((pixels, preview.label))
                    })();
                    (result, thumbnail)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.running = None;
                if this.serial == serial && !cancelled.load(Ordering::Acquire) {
                    let result = if this
                        .backend
                        .as_ref()
                        .is_some_and(|backend| !backend.current())
                    {
                        PreviewResult {
                            image: None,
                            thumbnail,
                            label: tr("文字预览已失效", "Text preview is out of date"),
                        }
                    } else {
                        match result {
                            Ok((pixels, label)) => PreviewResult {
                                image: Some(Arc::new(RenderImage::new(vec![image::Frame::new(
                                    pixels,
                                )]))),
                                thumbnail,
                                label,
                            },
                            Err(_) => PreviewResult {
                                image: None,
                                thumbnail,
                                label: tr("作品预览不可用", "Project preview unavailable"),
                            },
                        }
                    };
                    this.cache.insert(id, result);
                    cx.notify();
                }
                this.start(cx);
            });
        })
        .detach();
    }
}
