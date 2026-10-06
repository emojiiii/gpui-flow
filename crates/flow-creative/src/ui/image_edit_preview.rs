use super::*;
use crate::image_edit::ImageEditSpec;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) struct ImageEditPreview {
    base: Option<Arc<image::RgbaImage>>,
    desired: ImageEditSpec,
    rendered: Option<(u8, bool, bool)>,
    working: bool,
    cancelled: Arc<AtomicBool>,
    image: Option<Arc<RenderImage>>,
    error: Option<String>,
}
fn orientation(spec: &ImageEditSpec) -> (u8, bool, bool) {
    (spec.quarter_turns, spec.flip_horizontal, spec.flip_vertical)
}
impl ImageEditPreview {
    pub(super) fn new(path: PathBuf, asset: ProjectAsset, cx: &mut Context<Self>) -> Self {
        let cancelled = Arc::new(AtomicBool::new(false));
        let cancel = cancelled.clone();
        cx.on_release(|this: &mut Self, cx| {
            this.cancelled.store(true, Ordering::Release);
            if let Some(image) = this.image.take() {
                cx.defer(move |cx| cx.drop_image(image, None));
            }
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
                    Ok(pixels) => this.base = Some(Arc::new(pixels)),
                    Err(error) => this.error = Some(error),
                }
                this.start(cx);
                cx.notify();
            });
        })
        .detach();
        Self {
            base: None,
            desired: ImageEditSpec::default(),
            rendered: None,
            working: false,
            cancelled,
            image: None,
            error: None,
        }
    }
    pub(super) fn request(&mut self, spec: ImageEditSpec, cx: &mut Context<Self>) {
        self.desired = spec;
        self.start(cx);
    }
    pub(super) fn ready(&self) -> bool {
        self.error.is_none() && self.rendered == Some(orientation(&self.desired))
    }
    fn start(&mut self, cx: &mut Context<Self>) {
        if self.working || self.rendered == Some(orientation(&self.desired)) {
            return;
        }
        let Some(base) = self.base.clone() else {
            return;
        };
        self.working = true;
        let spec = self.desired;
        let expected = orientation(&spec);
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move {
                    let mut pixels = crate::image_edit::orient_preview(&base, &spec)
                        .map_err(|e| e.to_string())?;
                    for pixel in pixels.pixels_mut() {
                        pixel.0.swap(0, 2);
                    }
                    Ok::<_, String>(pixels)
                })
                .await;
            let _ = this.update(cx, |this, cx| {
                this.working = false;
                if orientation(&this.desired) == expected {
                    this.rendered = Some(expected);
                    if let Some(image) = this.image.take() {
                        cx.defer(move |cx| cx.drop_image(image, None));
                    }
                    match result {
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
impl Render for ImageEditPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .bg(rgb(0xe9ebee))
            .when_some(
                self.image.clone().filter(|_| self.ready()),
                |v, img_data| v.child(img(img_data).size_full().object_fit(ObjectFit::Contain)),
            )
            .when_some(self.error.clone(), |v, error| {
                v.child(div().p_2().text_xs().text_color(rgb(0xa25640)).child(error))
            })
            .when(!self.ready() && self.error.is_none(), |v| {
                v.child(div().p_2().text_xs().child("正在准备图片编辑预览…"))
            })
    }
}
