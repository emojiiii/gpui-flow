//! Optional GPUI implementation. No dependency on Flow scene, layout, or editor.
use crate::{TextMeasureRequest, TextMeasurer, TextMetrics};
use gpui::{
    Font, FontFallbacks, FontWeight, SharedString, TextRun, TextSystem, WindowTextSystem, font, px,
};
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex},
};

fn scene_font(family: &str, weight: u16, italic: bool) -> Font {
    let mut value = font(SharedString::from(family.to_owned()));
    value.weight = FontWeight(weight as f32);
    value.style = if italic {
        gpui::FontStyle::Italic
    } else {
        gpui::FontStyle::Normal
    };
    // Explicitly suppress inherited custom fallback/style/features. Platform
    // fallback and missing-glyph selection remain GPUI's responsibility.
    value.fallbacks = Some(FontFallbacks::default());
    value
}

/// Shape plain text with GPUI for both measurement and painting.
///
/// Uses explicit family/weight/italic face and the platform's fallback/shaping implementation.
/// The returned lines can be passed directly to `WrappedLine::paint`; use the
/// request's absolute line height. Like GPUI, font resolution may panic when the
/// platform cannot resolve any font. `GpuiTextMeasurer` catches that failure;
/// direct painting callers should handle it at their render boundary.
///
/// This lower-level painting entry point does not apply the bounded measurement
/// request's content-length limits, preserving explicitly sized text rendering.
pub fn shape_text(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    color: gpui::Hsla,
) -> Result<Vec<gpui::WrappedLine>, String> {
    if !request.para_spacing.is_finite()
        || !(0. ..=1_000_000.).contains(&request.para_spacing)
        || (request.para_spacing != 0. && request.content.len() > crate::MAX_TEXT_BYTES)
        || !request.font_size.is_finite()
        || request.font_size <= 0.
        || !request.line_height.is_finite()
        || request.line_height <= 0.
        || request
            .wrap_width
            .is_some_and(|w| !w.is_finite() || w <= 0.)
    {
        return Err("invalid native text dimensions".into());
    }
    let content = request.text_case.presentation(&request.content)?;
    let descriptor = scene_font(&request.font_family, request.font_weight, request.italic);
    // Empty strings also fail explicitly when no usable primary/fallback exists.
    system.resolve_font(&descriptor);
    let content_len = content.len();
    system
        .shape_text(
            SharedString::from(content.into_owned()),
            px(request.font_size),
            &[TextRun {
                len: content_len,
                font: descriptor,
                color,
                ..Default::default()
            }],
            request.wrap_width.map(px),
            None,
        )
        .map(|lines| lines.into_vec())
        .map_err(|error| error.to_string())
}

const CACHE_ENTRIES: usize = 128;
const CACHE_BYTES: usize = 1_048_576;
#[derive(Default)]
struct Cache {
    epoch: u64,
    bytes: usize,
    entries: VecDeque<(TextMeasureRequest, TextMetrics)>,
}
fn bytes(request: &TextMeasureRequest) -> usize {
    request.content.len() + request.font_family.len()
}
impl Cache {
    fn invalidate(&mut self) {
        self.epoch = self.epoch.wrapping_add(1);
        self.entries.clear();
        self.bytes = 0;
    }
    fn complete(
        &mut self,
        epoch: u64,
        request: TextMeasureRequest,
        metrics: TextMetrics,
    ) -> Result<TextMetrics, String> {
        if self.epoch != epoch {
            return Err("fonts invalidated during measurement; retry the transaction".into());
        }
        self.insert(request, metrics.clone());
        Ok(metrics)
    }
    fn insert(&mut self, request: TextMeasureRequest, metrics: TextMetrics) {
        // Concurrent identical misses must not grow the cache indefinitely.
        if self.entries.iter().any(|(key, _)| key == &request) {
            return;
        }
        let size = bytes(&request);
        while self.entries.len() >= CACHE_ENTRIES || self.bytes + size > CACHE_BYTES {
            let Some((old, _)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= bytes(&old);
        }
        self.bytes += size;
        self.entries.push_back((request, metrics));
    }
}

/// Real native shaping, with a bounded cache keyed by every measurement input.
/// Construct from the same App::text_system used for rendering. Font resources
/// belong to the host; after changing them use MeasuredFlowLayout's
/// refresh_text_metrics to invalidate this cache AND commit an atomic reflow.
pub struct GpuiTextMeasurer {
    text_system: Arc<TextSystem>,
    cache: Mutex<Cache>,
}
impl GpuiTextMeasurer {
    pub fn new(text_system: Arc<TextSystem>) -> Self {
        Self {
            text_system,
            cache: Mutex::new(Cache::default()),
        }
    }
    fn shape(&self, request: &TextMeasureRequest) -> Result<TextMetrics, String> {
        if self.text_system.all_font_names().is_empty() {
            return Err("native text system has no available fonts".into());
        }
        let shaper = WindowTextSystem::new(self.text_system.clone());
        let lines = shape_text(&shaper, request, gpui::Hsla::default())?;
        let mut metrics = TextMetrics {
            width: 0.,
            height: 0.,
            line_height: request.line_height,
            lines: 0,
        };
        for line in lines {
            let extent = line.size(px(request.line_height));
            metrics.width = metrics.width.max(extent.width.into());
            metrics.lines += line.wrap_boundaries.len() + 1;
        }
        metrics.height = request.block_height(metrics.lines);
        metrics.validate()?;
        Ok(metrics)
    }
}
impl TextMeasurer for GpuiTextMeasurer {
    fn measure(&self, request: &TextMeasureRequest) -> Result<TextMetrics, String> {
        request.validate()?;
        let epoch = {
            let cache = self
                .cache
                .lock()
                .map_err(|_| "text metric cache poisoned")?;
            if let Some((_, value)) = cache.entries.iter().find(|(key, _)| key == request) {
                return Ok(value.clone());
            }
            cache.epoch
        };
        let metrics = catch_unwind(AssertUnwindSafe(|| self.shape(request)))
            .map_err(|_| "native text shaping failed (font resolution or platform panic)")??;
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| "text metric cache poisoned")?;
        // Stale results must not escape to the document, even uncached.
        cache.complete(epoch, request.clone(), metrics)
    }
    fn first_baseline(&self, request: &TextMeasureRequest) -> Result<f32, String> {
        request.validate()?;
        let epoch = self
            .cache
            .lock()
            .map_err(|_| "text metric cache poisoned")?
            .epoch;
        let baseline = catch_unwind(AssertUnwindSafe(|| {
            if self.text_system.all_font_names().is_empty() {
                return Err("native text system has no available fonts".to_owned());
            }
            let shaper = WindowTextSystem::new(self.text_system.clone());
            let lines = shape_text(&shaper, request, gpui::Hsla::default())?;
            let first = lines
                .first()
                .ok_or_else(|| "native text shaping returned no first line".to_owned())?;
            let baseline = (request.line_height
                - f32::from(first.unwrapped_layout.ascent)
                - f32::from(first.unwrapped_layout.descent))
                * 0.5
                + f32::from(first.unwrapped_layout.ascent);
            if !baseline.is_finite() {
                return Err("native text returned a non-finite baseline".into());
            }
            Ok(baseline)
        }))
        .map_err(|_| "native text baseline shaping failed (font resolution or platform panic)")??;
        if self
            .cache
            .lock()
            .map_err(|_| "text metric cache poisoned")?
            .epoch
            != epoch
        {
            return Err(
                "fonts invalidated during baseline measurement; retry the transaction".into(),
            );
        }
        Ok(baseline)
    }
    fn invalidate(&self) {
        let mut cache = self.cache.lock().unwrap_or_else(|e| e.into_inner());
        cache.invalidate();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request(text: String) -> TextMeasureRequest {
        TextMeasureRequest {
            content: text,
            font_family: "sans-serif".into(),
            font_size: 16.,
            font_weight: 400,
            italic: false,
            text_case: Default::default(),
            para_spacing: 0.,
            line_height: 20.,
            wrap_width: None,
        }
    }
    // Deterministic platform plumbing exercises the actual GPUI override rather
    // than a fixture implementation of TextMeasurer. Native font pixels are
    // separately covered by the advanced_flow_layout Studio fixture.
    struct BaselinePlatform {
        panic_on_shape: bool,
    }
    impl gpui::PlatformTextSystem for BaselinePlatform {
        fn add_fonts(&self, _: Vec<std::borrow::Cow<'static, [u8]>>) -> gpui::Result<()> {
            Ok(())
        }
        fn all_font_names(&self) -> Vec<String> {
            vec!["baseline-fixture".into()]
        }
        fn font_id(&self, font: &Font) -> gpui::Result<gpui::FontId> {
            gpui::NoopTextSystem.font_id(font)
        }
        fn font_metrics(&self, id: gpui::FontId) -> gpui::FontMetrics {
            gpui::NoopTextSystem.font_metrics(id)
        }
        fn typographic_bounds(
            &self,
            f: gpui::FontId,
            g: gpui::GlyphId,
        ) -> gpui::Result<gpui::Bounds<f32>> {
            gpui::NoopTextSystem.typographic_bounds(f, g)
        }
        fn advance(&self, f: gpui::FontId, g: gpui::GlyphId) -> gpui::Result<gpui::Size<f32>> {
            gpui::NoopTextSystem.advance(f, g)
        }
        fn glyph_for_char(&self, f: gpui::FontId, c: char) -> Option<gpui::GlyphId> {
            gpui::NoopTextSystem.glyph_for_char(f, c)
        }
        fn glyph_raster_bounds(
            &self,
            p: &gpui::RenderGlyphParams,
        ) -> gpui::Result<gpui::Bounds<gpui::DevicePixels>> {
            gpui::NoopTextSystem.glyph_raster_bounds(p)
        }
        fn rasterize_glyph(
            &self,
            p: &gpui::RenderGlyphParams,
            b: gpui::Bounds<gpui::DevicePixels>,
        ) -> gpui::Result<(gpui::Size<gpui::DevicePixels>, Vec<u8>)> {
            gpui::NoopTextSystem.rasterize_glyph(p, b)
        }
        fn layout_line(
            &self,
            text: &str,
            size: gpui::Pixels,
            runs: &[gpui::FontRun],
        ) -> gpui::LineLayout {
            assert!(!self.panic_on_shape, "deliberate platform shaping panic");
            let mut line = gpui::NoopTextSystem.layout_line(text, size, runs);
            line.ascent = size * 0.75;
            line.descent = size * 0.25;
            line
        }
        fn recommended_rendering_mode(
            &self,
            _: gpui::FontId,
            _: gpui::Pixels,
        ) -> gpui::TextRenderingMode {
            gpui::TextRenderingMode::Grayscale
        }
    }
    #[test]
    fn first_baseline_direct_gpui_override_uses_shaped_ascent_descent_and_line_height() {
        let backend =
            GpuiTextMeasurer::new(Arc::new(TextSystem::new(Arc::new(BaselinePlatform {
                panic_on_shape: false,
            }))));
        for content in ["Baseline", "Baseline\nsecond line", ""] {
            let mut input = request(content.into());
            input.font_family = "baseline-fixture".into();
            input.line_height = 24.;
            // Shaped ascent=12, descent=4; centered leading adds 4.
            assert_eq!(backend.first_baseline(&input).unwrap(), 16.);
            input.line_height = 32.;
            assert_eq!(backend.first_baseline(&input).unwrap(), 20.);
            backend.invalidate();
            assert_eq!(backend.first_baseline(&input).unwrap(), 20.);
        }
    }
    #[test]
    fn first_baseline_direct_gpui_override_rejects_invalid_missing_fonts_and_panics() {
        let empty =
            GpuiTextMeasurer::new(Arc::new(TextSystem::new(Arc::new(gpui::NoopTextSystem))));
        let input = request("Baseline".into());
        assert!(
            empty
                .first_baseline(&input)
                .unwrap_err()
                .contains("no available fonts")
        );
        let mut invalid = input.clone();
        invalid.line_height = f32::NAN;
        assert!(empty.first_baseline(&invalid).is_err());
        let panics = GpuiTextMeasurer::new(Arc::new(TextSystem::new(Arc::new(BaselinePlatform {
            panic_on_shape: true,
        }))));
        assert!(
            panics
                .first_baseline(&input)
                .unwrap_err()
                .contains("platform panic")
        );
    }
    #[test]
    fn italic_is_part_of_the_measurement_cache_key() {
        let mut cache = Cache::default();
        let regular = request("face".into());
        let mut italic = regular.clone();
        italic.italic = true;
        cache.insert(
            regular,
            TextMetrics {
                width: 10.,
                height: 20.,
                line_height: 20.,
                lines: 1,
            },
        );
        cache.insert(
            italic.clone(),
            TextMetrics {
                width: 11.,
                height: 20.,
                line_height: 20.,
                lines: 1,
            },
        );
        assert_eq!(cache.entries.len(), 2);
        assert_eq!(
            cache
                .entries
                .iter()
                .find(|(key, _)| key == &italic)
                .unwrap()
                .1
                .width,
            11.
        );
    }
    #[test]
    fn cache_is_bounded_by_entries_and_content_bytes() {
        let mut cache = Cache::default();
        let metrics = TextMetrics {
            width: 1.,
            height: 20.,
            line_height: 20.,
            lines: 1,
        };
        for i in 0..200 {
            cache.insert(request(i.to_string()), metrics.clone());
        }
        assert_eq!(cache.entries.len(), CACHE_ENTRIES);
        for i in 0..40 {
            cache.insert(
                request(format!("{i}{}", "x".repeat(65_000))),
                metrics.clone(),
            );
        }
        assert!(cache.bytes <= CACHE_BYTES);
        assert!(cache.entries.len() < 20);
        let key = cache.entries.back().unwrap().0.clone();
        let len = cache.entries.len();
        cache.insert(key, metrics);
        assert_eq!(cache.entries.len(), len);
    }
    #[test]
    fn invalidation_interleaved_with_shape_rejects_old_result_then_recovers() {
        use std::sync::Barrier;
        let cache = Arc::new(Mutex::new(Cache::default()));
        let barrier = Arc::new(Barrier::new(2));
        let worker_cache = cache.clone();
        let worker_barrier = barrier.clone();
        let metrics = TextMetrics {
            width: 10.,
            height: 20.,
            line_height: 20.,
            lines: 1,
        };
        let fresh = metrics.clone();
        let worker = std::thread::spawn(move || {
            let epoch = worker_cache.lock().unwrap().epoch;
            worker_barrier.wait(); // Native shaping has begun outside the lock.
            worker_barrier.wait(); // Host invalidates before shaping finishes.
            worker_cache
                .lock()
                .unwrap()
                .complete(epoch, request("old".into()), metrics)
        });
        barrier.wait();
        cache.lock().unwrap().invalidate();
        barrier.wait();
        assert!(worker.join().unwrap().unwrap_err().contains("retry"));
        let mut cache = cache.lock().unwrap();
        assert!(cache.entries.is_empty());
        let epoch = cache.epoch;
        assert_eq!(
            cache
                .complete(epoch, request("fresh".into()), fresh.clone())
                .unwrap(),
            fresh
        );
        assert_eq!(cache.entries.len(), 1);
    }
    #[test]
    fn case_and_paragraph_spacing_are_independent_measurement_cache_keys() {
        let mut cache = Cache::default();
        for (case, spacing) in [
            (crate::TextCase::None, 0.),
            (crate::TextCase::Upper, 0.),
            (crate::TextCase::Lower, 0.),
            (crate::TextCase::Upper, 9.),
        ] {
            let mut key = request("Mixed\nText".into());
            key.text_case = case;
            key.para_spacing = spacing;
            cache.insert(
                key,
                TextMetrics {
                    width: 10.,
                    height: 40. + spacing,
                    line_height: 20.,
                    lines: 2,
                },
            );
        }
        assert_eq!(cache.entries.len(), 4);
        cache.invalidate();
        assert!(cache.entries.is_empty());
    }
}
