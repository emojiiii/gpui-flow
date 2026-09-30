//! Device-resolution gradient textures for the native backend.
//!
//! GPUI's Background shader exposes only two-stop linear gradients. This path
//! samples arbitrary linear/radial stops with the same transformed/clipped
//! tessellation as solid vectors, rather than substituting a solid paint.
use super::{RenderObject, vector::PaintLayer};
use gpui::{App, Bounds, Corners, Path, Pixels, Point, RenderImage, Window, point, px, size};
use gpui_flow_scene::{self as scene, Paint};
use std::{cell::RefCell, collections::VecDeque, sync::Arc};

const MAX_CACHE_ENTRIES: usize = 32;
const MAX_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_DEFERRED_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GradientCacheStats {
    pub entries: usize,
    pub retained_bytes: usize,
    pub deferred_bytes: usize,
    pub hits: u64,
    pub misses: u64,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GradientRenderIssue {
    pub window_id: u64,
    pub object: String,
    pub message: String,
}
#[derive(Clone, PartialEq)]
struct CacheKey {
    object: RenderObject,
    origin: scene::Point,
    window_id: u64,
    options: scene::RasterOptions,
    layer: PaintLayer,
}
struct Entry {
    key: CacheKey,
    image: Arc<RenderImage>,
    bounds: scene::Bounds,
    bytes: usize,
}
struct RetiredImages {
    images: Vec<Arc<RenderImage>>,
    bytes: usize,
}
impl Drop for RetiredImages {
    fn drop(&mut self) {
        // Also runs when a window closes before its scheduled next frame.
        let _ = CACHE.try_with(|cache| {
            let mut cache = cache.borrow_mut();
            cache.deferred_bytes = cache.deferred_bytes.saturating_sub(self.bytes);
        });
    }
}
#[derive(Default)]
struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
    deferred_bytes: usize,
    hits: u64,
    misses: u64,
    issues: VecDeque<GradientRenderIssue>,
}
impl Cache {
    fn make_room(&mut self, bytes: usize) -> (Vec<Arc<RenderImage>>, usize) {
        let mut retired = Vec::new();
        let mut retired_bytes = 0;
        while !self.entries.is_empty()
            && (self.entries.len() >= MAX_CACHE_ENTRIES || self.bytes + bytes > MAX_CACHE_BYTES)
        {
            let entry = self.entries.pop_front().unwrap();
            self.bytes -= entry.bytes;
            retired_bytes += entry.bytes;
            retired.push(entry.image);
        }
        self.deferred_bytes += retired_bytes;
        (retired, retired_bytes)
    }
}
thread_local! {static CACHE:RefCell<Cache>=RefCell::new(Cache::default());}
/// Retained CPU texture memory is bounded to 64 MiB / 32 entries. GPU atlas
/// eviction is deferred until the next frame so current-frame sprites stay valid.
pub fn gradient_cache_stats() -> GradientCacheStats {
    CACHE.with(|cache| {
        let c = cache.borrow();
        GradientCacheStats {
            entries: c.entries.len(),
            retained_bytes: c.bytes,
            deferred_bytes: c.deferred_bytes,
            hits: c.hits,
            misses: c.misses,
        }
    })
}
/// Retrieve explicit allocation/upload failures. No solid fallback is painted.
/// Hosts may show these messages in an inspector or error panel.
pub fn take_gradient_render_issues() -> Vec<GradientRenderIssue> {
    CACHE.with(|cache| cache.borrow_mut().issues.drain(..).collect())
}
/// Non-destructive window-scoped diagnostics. A SceneView can filter these by
/// its current object IDs without consuming another view's errors.
pub fn gradient_render_issues(window_id: u64) -> Vec<GradientRenderIssue> {
    CACHE.with(|cache| {
        cache
            .borrow()
            .issues
            .iter()
            .filter(|issue| issue.window_id == window_id)
            .cloned()
            .collect()
    })
}
pub(super) fn clear_object_issues(window_id: u64, object: &str) {
    CACHE.with(|cache| {
        cache
            .borrow_mut()
            .issues
            .retain(|issue| issue.window_id != window_id || issue.object != object)
    });
}
/// Release retained textures; call outside the paint phase (e.g. when closing a scene).
pub fn clear_gradient_cache(window: &mut Window, cx: &mut App) {
    let images = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        cache.bytes = 0;
        cache.entries.drain(..).map(|e| e.image).collect::<Vec<_>>()
    });
    for image in images {
        cx.drop_image(image, Some(window));
    }
}
fn record_error(object: &RenderObject, message: String, window_id: u64) {
    CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let issue = GradientRenderIssue {
            window_id,
            object: object.object.id.to_string(),
            message,
        };
        if !cache.issues.contains(&issue) {
            if cache.issues.len() >= 64 {
                cache.issues.pop_front();
            }
            cache.issues.push_back(issue);
        }
    });
}
fn gpui_bounds(bounds: scene::Bounds) -> Bounds<Pixels> {
    Bounds::new(
        point(px(bounds.origin.x), px(bounds.origin.y)),
        size(px(bounds.size.width), px(bounds.size.height)),
    )
}

pub(super) fn paint_gradient(
    object: &RenderObject,
    origin: Point<Pixels>,
    path: Path<Pixels>,
    paint: &Paint,
    layer: PaintLayer,
    window: &mut Window,
    cx: &mut App,
) {
    let window_id = window.window_handle().window_id().as_u64();
    let content = window.content_mask().bounds;
    let options = scene::RasterOptions {
        scale_factor: window.scale_factor(),
        opacity: object.opacity,
        clip_bounds: Some(scene::Bounds::new(
            scene::Point::new(content.origin.x.into(), content.origin.y.into()),
            scene::Size::new(content.size.width.into(), content.size.height.into()),
        )),
        ..Default::default()
    };
    let origin = scene::Point::new(origin.x.into(), origin.y.into());
    let key = CacheKey {
        object: object.clone(),
        origin,
        window_id,
        options,
        layer,
    };
    let cached = CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if let Some(index) = cache.entries.iter().position(|entry| entry.key == key) {
            let entry = cache.entries.remove(index).unwrap();
            let image = entry.image.clone();
            let bounds = entry.bounds;
            cache.entries.push_back(entry);
            cache.hits += 1;
            Some((image, bounds))
        } else {
            cache.misses += 1;
            None
        }
    });
    let (image, bounds) = if let Some(cached) = cached {
        cached
    } else {
        if CACHE.with(|cache| cache.borrow().deferred_bytes >= MAX_DEFERRED_BYTES) {
            record_error(
                object,
                "gradient texture retirement budget reached; retry on the next frame".into(),
                window_id,
            );
            return;
        }
        let triangles: Vec<_> = path
            .vertices
            .as_chunks::<3>()
            .0
            .iter()
            .map(|triangle| {
                std::array::from_fn(|i| {
                    let v = &triangle[i];
                    scene::Point::new(v.xy_position.x.into(), v.xy_position.y.into())
                })
            })
            .collect();
        let transform = scene::Affine::translation(origin.x, origin.y) * object.screen_transform;
        let raster = match scene::rasterize_paint(paint, &triangles, transform, options) {
            Ok(Some(raster)) => raster,
            Ok(None) => return,
            Err(error) => {
                record_error(object, error.to_string(), window_id);
                return;
            }
        };
        let bytes = raster.byte_len();
        let bounds = raster.bounds;
        let Some(buffer) = image::RgbaImage::from_raw(raster.width, raster.height, raster.bgra)
        else {
            record_error(
                object,
                "gradient image buffer dimensions do not match data".into(),
                window_id,
            );
            return;
        };
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        let (retired, retired_bytes) = CACHE.with(|cache| {
            let mut cache = cache.borrow_mut();
            let retired = cache.make_room(bytes);
            cache.bytes += bytes;
            cache.entries.push_back(Entry {
                key,
                image: image.clone(),
                bounds,
                bytes,
            });
            retired
        });
        if !retired.is_empty() {
            let mut retired = RetiredImages {
                images: retired,
                bytes: retired_bytes,
            };
            window.on_next_frame(move |window, cx| {
                for image in retired.images.drain(..) {
                    cx.drop_image(image, Some(window));
                }
            });
        }
        (image, bounds)
    };
    let bounds = gpui_bounds(bounds);
    if let Err(error) = window.paint_image(bounds, bounds, Corners::all(px(0.0)), image, 0, false) {
        record_error(
            object,
            format!("gradient texture upload failed: {error}"),
            window_id,
        );
    }
    let _ = cx; // Reserved for explicit host-side cache lifecycle; paint eviction is frame-deferred.
}

#[cfg(test)]
mod tests {
    use super::*;
    fn object() -> RenderObject {
        RenderObject {
            object: scene::DisplayObject::new("gradient", scene::display::Rect::new(10.0, 10.0)),
            screen_transform: scene::Affine::IDENTITY,
            viewport_transform: scene::Affine::IDENTITY,
            opacity: 1.0,
            clips: vec![],
        }
    }
    fn entry(index: usize, bytes: usize) -> Entry {
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(
            image::RgbaImage::new(1, 1),
        )]));
        Entry {
            key: CacheKey {
                object: object(),
                origin: scene::Point::new(index as f32, 0.0),
                window_id: 1,
                options: scene::RasterOptions::default(),
                layer: PaintLayer::Fill(0),
            },
            image,
            bounds: scene::Bounds::new(scene::Point::ZERO, scene::Size::new(1.0, 1.0)),
            bytes,
        }
    }
    #[test]
    fn texture_cache_enforces_entry_and_byte_budgets() {
        for bytes in [4, 3 * 1024 * 1024] {
            let mut cache = Cache::default();
            for index in 0..100 {
                cache.make_room(bytes);
                cache.bytes += bytes;
                cache.entries.push_back(entry(index, bytes));
                assert!(cache.entries.len() <= MAX_CACHE_ENTRIES);
                assert!(cache.bytes <= MAX_CACHE_BYTES);
            }
            assert_eq!(cache.entries.back().unwrap().key.origin.x, 99.0);
            assert!(cache.deferred_bytes > 0);
        }
    }
    #[test]
    fn cache_key_invalidates_for_transform_opacity_clip_and_scale() {
        let key = entry(0, 4).key;
        let mut changed = key.clone();
        changed.object.screen_transform.tx = 1.0;
        assert!(changed != key);
        let mut changed = key.clone();
        changed.object.opacity = 0.5;
        assert!(changed != key);
        let mut changed = key.clone();
        changed.options.scale_factor = 2.0;
        assert!(changed != key);
        let mut changed = key.clone();
        changed.object.clips.push(scene::ClipRegion {
            local_bounds: scene::Bounds::new(scene::Point::ZERO, scene::Size::new(5.0, 5.0)),
            corner_radius: 1.0,
            world_transform: scene::Affine::IDENTITY,
        });
        assert!(changed != key);
    }
    #[test]
    fn cache_keys_never_alias_distinct_fill_or_stroke_layers() {
        let key = entry(0, 4).key;
        for layer in [
            PaintLayer::Fill(1),
            PaintLayer::Stroke(0),
            PaintLayer::Stroke(1),
        ] {
            let mut changed = key.clone();
            changed.layer = layer;
            assert!(changed != key);
        }
        let mut thick = key.clone();
        thick.layer = PaintLayer::Stroke(0);
        let mut thin = thick.clone();
        thin.layer = PaintLayer::Stroke(1);
        assert!(thick != thin);
    }
    #[test]
    fn diagnostics_are_window_scoped_non_destructive_and_clear_on_repaint() {
        CACHE.with(|cache| cache.borrow_mut().issues.clear());
        record_error(&object(), "too large".into(), 1);
        record_error(&object(), "upload".into(), 2);
        assert_eq!(gradient_render_issues(1).len(), 1);
        assert_eq!(gradient_render_issues(1).len(), 1);
        assert_eq!(gradient_render_issues(2).len(), 1);
        clear_object_issues(1, "gradient");
        assert!(gradient_render_issues(1).is_empty());
        assert_eq!(gradient_render_issues(2).len(), 1);
        CACHE.with(|cache| cache.borrow_mut().issues.clear());
    }
}
