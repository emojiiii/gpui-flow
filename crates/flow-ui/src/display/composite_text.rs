//! Same-native-backend text input producer, used only by an admitted CPU job.
use gpui_flow_composite::{
    CompositeError, CompositeLimits, DomainSource, RasterViewport, StaticImage, TextImage,
    TextInputs,
};
use gpui_flow_scene::{Affine, Bounds, Paint, Point, Shape, Size, display::TextAlign};
use gpui_flow_text::{GpuiTextRasterizer, TextMeasureRequest, TextRasterRequest};
use std::{
    fmt,
    sync::{Arc, atomic::AtomicBool},
};

#[derive(Clone)]
pub(crate) struct TextBackendSnapshot {
    bridge: Arc<GpuiTextRasterizer>,
    epoch: u64,
}
impl fmt::Debug for TextBackendSnapshot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("TextBackendSnapshot")
            .field("epoch", &self.epoch)
            .finish_non_exhaustive()
    }
}
impl PartialEq for TextBackendSnapshot {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.bridge, &other.bridge) && self.epoch == other.epoch
    }
}
impl TextBackendSnapshot {
    pub fn new(bridge: &Arc<GpuiTextRasterizer>) -> Self {
        Self {
            bridge: bridge.clone(),
            epoch: bridge.epoch(),
        }
    }
    pub fn current(&self) -> bool {
        self.bridge.epoch() == self.epoch
    }
}
fn positive_uniform(m: Affine) -> bool {
    m.a.is_finite()
        && m.d.is_finite()
        && m.a > 0.
        && m.d > 0.
        && m.b == 0.
        && m.c == 0.
        && (m.a - m.d).abs() <= m.a * 0.00001
}
/// These inputs are complete, immutable and tied to source/attachment/font epochs
/// in RequestKey. Bitmap pixels are rendered at final screen DPI, then mapped
/// back into the node's local coordinates without rescaling the glyph layout.
pub(crate) struct TextPreparation<'a> {
    pub backend: Option<&'a TextBackendSnapshot>,
    pub viewport: RasterViewport,
    pub cancelled: &'a AtomicBool,
    pub retained: u64,
    pub source_bytes: &'a mut u64,
    pub remaining_work: &'a mut u64,
}
impl TextPreparation<'_> {
    pub fn domain(&mut self, source: &DomainSource, inputs: &mut TextInputs) {
        for (object, world, full_source) in source.text_sources() {
            let result = (|| {
                let Shape::Text(text) = &object.shape else {
                    unreachable!()
                };
                if object.style.strokes().next().is_some()
                    || object.style.fills().count() > 1
                    || object
                        .style
                        .fills()
                        .any(|p| !matches!(p, Paint::Solid { .. }))
                {
                    return Err(CompositeError::UnsupportedTextStyle(object.id.clone()));
                }
                let m = self.viewport.world_to_screen * world;
                if !positive_uniform(m) {
                    return Err(CompositeError::UnsupportedTransform);
                }
                let Some(Paint::Solid { color }) = object.style.fills().next() else {
                    return Ok(None);
                };
                if color.a == 0 {
                    return Ok(None);
                }
                let backend = self
                    .backend
                    .ok_or_else(|| CompositeError::TextBackendRequired(object.id.clone()))?;
                let content = text.content_bounds();
                let bounds = [
                    m.tx + content.origin.x * m.a,
                    m.ty + content.origin.y * m.d,
                    content.size.width * m.a,
                    content.size.height * m.d,
                ];
                let clip = source_clip(full_source, bounds, self.viewport);
                let Some(clip) = clip else {
                    return Ok(None);
                };
                let request = TextRasterRequest {
                    overflow: crate::text_measure::text_overflow(text.overflow),
                    decoration: crate::text_measure::text_decoration(text.decoration),
                    vertical_align: crate::text_measure::vertical_align(text.vertical_align),
                    text: TextMeasureRequest {
                        content: text.content.clone(),
                        font_family: text.font_family.clone(),
                        font_size: text.font_size * m.d,
                        font_weight: text.font_weight,
                        italic: text.italic,
                        line_height: text.font_size * text.line_height * m.d,
                        wrap_width: text.wrap.then_some(content.size.width * m.a),
                    },
                    align: match text.align {
                        TextAlign::Start => gpui::TextAlign::Left,
                        TextAlign::Center => gpui::TextAlign::Center,
                        TextAlign::End => gpui::TextAlign::Right,
                    },
                    bounds,
                    clip: [
                        clip.origin.x,
                        clip.origin.y,
                        clip.size.width,
                        clip.size.height,
                    ],
                    device_scale: self.viewport.device_scale,
                    color: [color.r, color.g, color.b, color.a],
                };
                let capacity = (32 * 1024 * 1024u64)
                    .saturating_sub(*self.source_bytes)
                    .min(
                        CompositeLimits::default()
                            .max_surface_bytes
                            .saturating_sub(self.retained)
                            .saturating_sub(*self.source_bytes),
                    );
                let raster = backend
                    .bridge
                    .rasterize(
                        &request,
                        backend.epoch,
                        self.cancelled,
                        capacity as usize,
                        self.remaining_work,
                    )
                    .map_err(|reason| match reason.as_str() {
                        "text working-set limit" => CompositeError::WorkingSetLimit,
                        "text pixel work limit" => CompositeError::WorkLimit,
                        "text snapshot superseded" => CompositeError::Superseded,
                        _ => CompositeError::TextRasterization {
                            text: object.id.clone(),
                            reason,
                        },
                    })?;
                let Some(raster) = raster else {
                    return Ok(None);
                };
                let local_bounds = Bounds::new(
                    Point::new(
                        (raster.origin[0] as f32 / self.viewport.device_scale - m.tx) / m.a,
                        (raster.origin[1] as f32 / self.viewport.device_scale - m.ty) / m.d,
                    ),
                    Size::new(
                        raster.width as f32 / (self.viewport.device_scale * m.a),
                        raster.height as f32 / (self.viewport.device_scale * m.d),
                    ),
                );
                let bytes = raster.premul_rgba.len() as u64;
                let image = StaticImage::from_premultiplied_rgba(
                    raster.width,
                    raster.height,
                    raster.premul_rgba,
                    CompositeLimits::default(),
                )?;
                *self.source_bytes += bytes;
                Ok(Some(Arc::new(TextImage {
                    image: Arc::new(image),
                    local_bounds,
                })))
            })();
            inputs.insert(object.id.clone(), result);
        }
    }
}

fn source_clip(full_source: bool, bounds: [f32; 4], viewport: RasterViewport) -> Option<Bounds> {
    if full_source {
        Some(Bounds::new(
            Point::new(bounds[0], bounds[1]),
            Size::new(bounds[2], bounds[3]),
        ))
    } else {
        intersection(viewport.canvas_bounds, viewport.content_mask)
    }
}

fn intersection(a: Bounds, b: Bounds) -> Option<Bounds> {
    let x = a.min_x().max(b.min_x());
    let y = a.min_y().max(b.min_y());
    let right = a.max_x().min(b.max_x());
    let bottom = a.max_y().min(b.max_y());
    (right > x && bottom > y)
        .then(|| Bounds::new(Point::new(x, y), Size::new(right - x, bottom - y)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn effect_text_source_keeps_offscreen_ink_before_destination_clipping() {
        let viewport = RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: Bounds::new(Point::ZERO, Size::new(100., 100.)),
            content_mask: Bounds::new(Point::new(10., 10.), Size::new(80., 80.)),
            device_scale: 1.25,
        };
        let bounds = [-20., -30., 70., 80.];
        assert_eq!(
            source_clip(true, bounds, viewport),
            Some(Bounds::new(Point::new(-20., -30.), Size::new(70., 80.)))
        );
        assert_eq!(
            source_clip(false, bounds, viewport),
            Some(viewport.content_mask)
        );
    }
}
