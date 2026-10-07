use super::*;
use crate::image_adjust::ImageAdjustSpec;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct AdjustmentPreview {
    base: Option<Arc<image::RgbaImage>>,
    desired: ImageAdjustSpec,
    serial: u64,
    completed: Option<u64>,
    working: bool,
    lifetime_cancel: Arc<AtomicBool>,
    render_cancel: Option<Arc<AtomicBool>>,
    image: Option<Arc<RenderImage>>,
    view_size: WorldSize,
    fallback_path: PathBuf,
    error: Option<String>,
}
impl AdjustmentPreview {
    pub(super) fn new(path: PathBuf, asset: ProjectAsset, cx: &mut Context<Self>) -> Self {
        let fallback_path = path.clone();
        let lifetime_cancel = Arc::new(AtomicBool::new(false));
        let cancel = lifetime_cancel.clone();
        cx.on_release(|this: &mut Self, cx| {
            this.lifetime_cancel.store(true, Ordering::Release);
            if let Some(cancel) = &this.render_cancel {
                cancel.store(true, Ordering::Release);
            }
            this.clear_image(cx);
        })
        .detach();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    crate::image_edit::load_preview(&path, &asset, &cancel)
                        .map_err(|e| e.to_string())
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                match result {
                    Ok(base) => this.base = Some(Arc::new(base)),
                    Err(error) => this.error = Some(error),
                }
                this.start(cx);
                cx.notify();
            });
        })
        .detach();
        Self {
            base: None,
            desired: Default::default(),
            serial: 0,
            completed: None,
            working: false,
            lifetime_cancel,
            render_cancel: None,
            image: None,
            view_size: WorldSize::new(1., 1.),
            fallback_path,
            error: None,
        }
    }
    fn clear_image(&mut self, cx: &mut App) {
        if let Some(image) = self.image.take() {
            cx.defer(move |cx| cx.drop_image(image, None));
        }
    }
    pub(super) fn request(&mut self, spec: ImageAdjustSpec, cx: &mut Context<Self>) {
        if self.desired == spec {
            return;
        }
        self.desired = spec;
        self.serial = self.serial.wrapping_add(1);
        self.completed = None;
        if let Some(cancel) = &self.render_cancel {
            cancel.store(true, Ordering::Release);
        }
        if self.base.is_some() {
            self.error = None;
        }
        self.start(cx);
        cx.notify();
    }
    pub(super) fn ready_for(&self, spec: ImageAdjustSpec) -> bool {
        self.desired == spec
            && self.completed == Some(self.serial)
            && self.image.is_some()
            && self.error.is_none()
    }
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.working || self.completed == Some(self.serial) {
            return;
        }
        let Some(base) = self.base.clone() else {
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.render_cancel = Some(cancel.clone());
        self.working = true;
        let serial = self.serial;
        let spec = self.desired;
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut pixels = crate::image_adjust::adjust_preview(&base, &spec, &cancel)
                        .map_err(|e| e.to_string())?;
                    for pixel in pixels.pixels_mut() {
                        pixel.0.swap(0, 2);
                    }
                    Ok::<_, String>(pixels)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.working = false;
                this.render_cancel = None;
                if this.serial == serial {
                    this.completed = Some(serial);
                    match result {
                        Ok(pixels) => {
                            this.clear_image(cx);
                            this.image =
                                Some(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])));
                            this.error = None;
                        }
                        Err(error) => this.error = Some(error),
                    }
                }
                this.start(cx);
                cx.notify();
            });
        })
        .detach();
    }
}
impl AdjustmentPreview {
    pub(super) fn set_size(&mut self, size: WorldSize, cx: &mut Context<Self>) {
        if self.view_size != size {
            self.view_size = size;
            cx.notify();
        }
    }
}
impl Render for AdjustmentPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let image = match self.image.clone() {
            Some(pixels) => img(pixels),
            None => img(self.fallback_path.clone()),
        }
        // Adjustment changes pixels, never the object's framing. Pin the leaf
        // to the same viewport used by the normal image renderer so intrinsic
        // layout and Contain cannot make a freely resized image jump on entry.
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .object_fit(ObjectFit::Cover);
        let status = self
            .error
            .clone()
            .or_else(|| (!self.ready_for(self.desired)).then(|| "正在更新预览…".to_string()));
        div()
            .relative()
            .w(px(self.view_size.width))
            .h(px(self.view_size.height))
            .overflow_hidden()
            .child(image)
            .when_some(status, |v, text| {
                v.child(
                    div()
                        .absolute()
                        .left(px(8.))
                        .bottom(px(8.))
                        .px_2()
                        .py_1()
                        .rounded(px(6.))
                        .bg(rgba(0xffffffdd))
                        .text_size(px(10.))
                        .text_color(rgb(0x68707a))
                        .child(text),
                )
            })
    }
}
