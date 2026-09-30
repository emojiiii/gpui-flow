//! Native font shaping used by measured FlowLayout and the scene text renderer.
use gpui::{
    Font, FontFallbacks, FontWeight, SharedString, TextRun, TextSystem, WindowTextSystem, font, px,
};
use gpui_flow_layout::{TextMeasureRequest, TextMeasurer, TextMetrics};
use std::{
    collections::VecDeque,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, Mutex},
};

pub(crate) fn scene_font(family: &str, weight: u16) -> Font {
    let mut value = font(SharedString::from(family.to_owned()));
    value.weight = FontWeight(weight as f32);
    // Explicitly suppress inherited custom fallback/style/features. Platform
    // fallback and missing-glyph selection remain GPUI's responsibility.
    value.fallbacks = Some(FontFallbacks::default());
    value
}

/// Both native measurement and painting use this exact shaping entry point.
pub(crate) fn shape_scene_text(
    system: &WindowTextSystem,
    request: &TextMeasureRequest,
    color: gpui::Hsla,
) -> Result<Vec<gpui::WrappedLine>, String> {
    if !request.font_size.is_finite()
        || request.font_size <= 0.
        || !request.line_height.is_finite()
        || request.line_height <= 0.
        || request
            .wrap_width
            .is_some_and(|w| !w.is_finite() || w <= 0.)
    {
        return Err("invalid native text dimensions".into());
    }
    let descriptor = scene_font(&request.font_family, request.font_weight);
    // Empty strings also fail explicitly when no usable primary/fallback exists.
    system.resolve_font(&descriptor);
    system
        .shape_text(
            SharedString::from(request.content.clone()),
            px(request.font_size),
            &[TextRun {
                len: request.content.len(),
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
        let lines = shape_scene_text(&shaper, request, gpui::Hsla::default())?;
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
        metrics.height = metrics.line_height * metrics.lines as f32;
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
            line_height: 20.,
            wrap_width: None,
        }
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
}
