use super::*;
use crate::image_rotation::ImageRotationSpec;
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const PREVIEW_WIDTH: f32 = 292.;
pub(super) const PREVIEW_HEIGHT: f32 = 120.;

pub(super) struct RotationPreview {
    base: Option<Arc<image::RgbaImage>>,
    desired: ImageRotationSpec,
    serial: u64,
    completed: Option<u64>,
    working: bool,
    lifetime_cancel: Arc<AtomicBool>,
    render_cancel: Option<Arc<AtomicBool>>,
    image: Option<Arc<RenderImage>>,
    image_size: (u32, u32),
    error: Option<String>,
}
impl RotationPreview {
    pub(super) fn new(path: PathBuf, asset: ProjectAsset, cx: &mut Context<Self>) -> Self {
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
            image_size: (1, 1),
            error: None,
        }
    }
    fn clear_image(&mut self, cx: &mut App) {
        if let Some(image) = self.image.take() {
            cx.defer(move |cx| cx.drop_image(image, None));
        }
    }
    pub(super) fn request(&mut self, spec: ImageRotationSpec, cx: &mut Context<Self>) {
        if self.desired == spec {
            return;
        }
        self.desired = spec;
        self.serial = self.serial.wrapping_add(1);
        self.completed = None;
        if let Some(cancel) = &self.render_cancel {
            cancel.store(true, Ordering::Release);
        }
        self.clear_image(cx);
        if self.base.is_some() {
            self.error = None;
        }
        self.start(cx);
        cx.notify();
    }
    pub(super) fn ready_for(&self, spec: ImageRotationSpec) -> bool {
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
                    let mut pixels = crate::image_rotation::rotate_preview(&base, &spec, &cancel)
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
                    this.clear_image(cx);
                    match result {
                        Ok(pixels) => {
                            this.image_size = pixels.dimensions();
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
impl Render for RotationPreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let fitted = fit_preview(self.image_size);
        div()
            .relative()
            .size_full()
            .bg(rgb(0xe9ebee))
            .rounded(px(8.))
            .overflow_hidden()
            .when_some(
                self.image.clone().filter(|_| self.ready_for(self.desired)),
                |v, pixels| {
                    v.child(
                        img(pixels)
                            .absolute()
                            .left(px(fitted.origin.x))
                            .top(px(fitted.origin.y))
                            .w(px(fitted.size.width))
                            .h(px(fitted.size.height))
                            .object_fit(ObjectFit::Contain),
                    )
                },
            )
            .when_some(self.error.clone(), |v, error| {
                v.child(div().p_3().text_xs().text_color(rgb(0xa25640)).child(error))
            })
            .when(self.image.is_none() && self.error.is_none(), |v| {
                v.child(div().p_3().text_xs().child(tr_format!(
                    "正在预览 {}°…",
                    "Previewing {}°…",
                    self.desired.degrees
                )))
            })
    }
}

fn fit_preview((width, height): (u32, u32)) -> gpui_flow_core::Rect {
    let scale = (f64::from(PREVIEW_WIDTH) / f64::from(width))
        .min(f64::from(PREVIEW_HEIGHT) / f64::from(height));
    let size = WorldSize::new(
        ((f64::from(width) * scale) as f32).min(PREVIEW_WIDTH),
        ((f64::from(height) * scale) as f32).min(PREVIEW_HEIGHT),
    );
    gpui_flow_core::Rect::new(
        WorldPoint::new(
            (PREVIEW_WIDTH - size.width) / 2.,
            (PREVIEW_HEIGHT - size.height) / 2.,
        ),
        size,
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn expanded_preview_fits_the_panel_without_cropping_or_distortion() {
        for dimensions in [(640, 400), (755, 667), (400, 640)] {
            let fitted = fit_preview(dimensions);
            assert!(fitted.origin.x >= 0. && fitted.origin.y >= 0.);
            assert!(
                fitted.max_x() <= PREVIEW_WIDTH + 0.001 && fitted.max_y() <= PREVIEW_HEIGHT + 0.001
            );
            assert!(
                (fitted.size.width / fitted.size.height
                    - dimensions.0 as f32 / dimensions.1 as f32)
                    .abs()
                    < 0.0001
            );
        }
    }
}
