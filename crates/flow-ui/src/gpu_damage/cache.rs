//! Damage admission is separate from backend execution. Only a submitted receipt
//! acknowledges an engine revision; this module never invents GPU work counts.
use gpui::{self as g, *};
use gpui_flow_composite::RasterViewport;
use gpui_flow_engine::ChangeSet;
use gpui_flow_scene::{Affine, Bounds as WorldBounds};
use std::{collections::BTreeSet, rc::Rc};

pub(crate) const TILE: u32 = 512;
pub(crate) const MAX_TILES: usize = 32;
const MAX_OPERATIONS: usize = 4096;
const MAX_VERTICES: usize = 262_144;
const MAX_EXPANDED_UPLOAD_BYTES: usize = 48 * 1024 * 1024;

/// Conservative window-wide tiled upload allowance, not a VRAM measurement.
/// The pinned backend uses 104-byte raster vertices; 128 per vertex and 512
/// per operation leave room for instances, sprites and alignment. Every input
/// is charged to every visible tile, even when clipping avoids actual work.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Admission {
    pub tiles: usize,
    pub upload_bytes: usize,
}
impl Admission {
    pub fn budget(tiles: usize) -> Self {
        Self {
            tiles,
            upload_bytes: MAX_EXPANDED_UPLOAD_BYTES,
        }
    }
    fn expanded(source: &g::Scene, tiles: usize) -> Option<Self> {
        let vertices = source
            .paths
            .iter()
            .try_fold(0usize, |total, path| total.checked_add(path.vertices.len()))?;
        let bytes = vertices
            .checked_mul(128)?
            .checked_add(source.len().checked_mul(512)?)?
            .checked_add(4096)?
            .checked_mul(tiles)?
            .checked_add(tiles.checked_mul(512)?)?;
        Some(Self {
            tiles,
            upload_bytes: bytes,
        })
    }
    pub fn consume(&mut self, used: Self) {
        self.tiles = self.tiles.saturating_sub(used.tiles);
        self.upload_bytes = self.upload_bytes.saturating_sub(used.upload_bytes);
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Damage {
    pub revision: u64,
    pub before: u64,
    pub bounds: Option<WorldBounds>,
    pub full: bool,
}
impl Damage {
    pub fn new(revision: u64) -> Self {
        Self {
            revision,
            before: revision,
            bounds: None,
            full: true,
        }
    }
    pub fn observe(&mut self, change: &ChangeSet) {
        if !change.changed() {
            return;
        }
        self.full |= self.revision != change.before_revision || change.full_paint_invalidation;
        self.revision = change.revision;
        for bounds in &change.damage {
            if !finite_world(*bounds) {
                self.full = true;
                continue;
            }
            self.bounds = Some(self.bounds.map_or(*bounds, |old| old.union(*bounds)));
            if self.bounds.is_some_and(|b| !finite_world(b)) {
                self.full = true;
                self.bounds = None;
            }
        }
        // A changed paint revision without analytic damage is conservative.
        self.full |= change.damage.is_empty();
    }
    pub fn acknowledge(&mut self, revision: u64) {
        if self.revision == revision {
            self.before = revision;
            self.bounds = None;
            self.full = false;
        }
    }
}
fn finite_world(b: WorldBounds) -> bool {
    [
        b.origin.x,
        b.origin.y,
        b.size.width,
        b.size.height,
        b.origin.x + b.size.width,
        b.origin.y + b.size.height,
    ]
    .into_iter()
    .all(f32::is_finite)
        && b.size.width >= 0.
        && b.size.height >= 0.
}
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct FrameKey {
    pub engine: u64,
    pub document: u64,
    pub revision: u64,
    pub viewport: RasterViewport,
    pub background: u32,
    pub grid: Option<u32>,
}
impl FrameKey {
    fn same_geometry(&self, other: &Self) -> bool {
        self.engine == other.engine
            && self.document == other.document
            && self.viewport == other.viewport
            && self.background == other.background
            && self.grid == other.grid
    }
}
#[derive(Default)]
pub(crate) struct Cache {
    generation: u64,
    committed: Option<FrameKey>,
    prepared: Option<(FrameKey, Rc<g::Scene>)>,
}
pub(crate) struct Prepared {
    pub scene: g::Scene,
    pub key: Option<FrameKey>,
    pub admission: Admission,
    pub fallback: Option<&'static str>,
}
impl Cache {
    pub fn invalidate(&mut self) {
        self.committed = None;
        self.prepared = None;
    }
    pub fn acknowledge(&mut self, key: &FrameKey) {
        if self.prepared.as_ref().is_some_and(|(k, _)| k == key) {
            self.committed = Some(key.clone());
        }
    }
    pub fn prepare(
        &mut self,
        source: g::Scene,
        key: FrameKey,
        damage: &Damage,
        layer: LayerKey,
        eligible: bool,
        remaining: Admission,
    ) -> Prepared {
        let reject = if !eligible {
            Some("unsupported scene presentation")
        } else if !source.layers.frames.is_empty() {
            Some("host GPU layers use the native fallback")
        } else if !source.monochrome_sprites.is_empty()
            || !source.subpixel_sprites.is_empty()
            || !source.polychrome_sprites.is_empty()
            || !source.surfaces.is_empty()
            || !source.shadows.is_empty()
            || !source.underlines.is_empty()
        {
            Some("atlas or effect primitives use the native fallback")
        } else if source.len() > MAX_OPERATIONS
            || source.paths.iter().map(|p| p.vertices.len()).sum::<usize>() > MAX_VERTICES
        {
            Some("GPU input budget exceeded")
        } else {
            None
        };
        if let Some(reason) = reject {
            self.invalidate();
            return native(source, reason);
        }
        let Some((clip, coords)) = visible_tiles(&key.viewport) else {
            self.invalidate();
            return native(source, "invalid or excessive device viewport");
        };
        if coords.len() > remaining.tiles {
            self.invalidate();
            return native(source, "window tile admission budget exceeded");
        }
        let Some(admission) = Admission::expanded(&source, coords.len())
            .filter(|usage| usage.upload_bytes <= remaining.upload_bytes)
        else {
            self.invalidate();
            return native(source, "window tile-expanded upload budget exceeded");
        };
        // The cache owns an opaque canvas only. Ancestor opacity/rounding or a
        // different actual background makes this frame use the unchanged path.
        let bg = rgba((key.background << 8) | 255);
        if !opaque_background(&source, clip, bg, &key.viewport) {
            self.invalidate();
            return native(source, "canvas backdrop is not a supported opaque quad");
        }
        let baseline_current = self
            .prepared
            .as_ref()
            .is_some_and(|(key, _)| self.committed.as_ref() == Some(key));
        let unchanged = baseline_current && self.committed.as_ref() == Some(&key);
        let content = if unchanged {
            self.prepared
                .as_ref()
                .map(|(_, scene)| scene.clone())
                .unwrap_or_else(|| Rc::new(source))
        } else {
            let Some(next) = self.generation.checked_add(1) else {
                self.invalidate();
                return native(source, "GPU layer generation exhausted");
            };
            self.generation = next;
            Rc::new(source)
        };
        let full = !unchanged
            && (!baseline_current
                || damage.full
                || damage.revision != key.revision
                || self.committed.as_ref().is_none_or(|last| {
                    !last.same_geometry(&key) || last.revision != damage.before
                }));
        let dirty = if unchanged {
            Vec::new()
        } else if full {
            coords.clone()
        } else {
            damage
                .bounds
                .map(|b| damaged_tiles(b, &key.viewport, &coords))
                .unwrap_or_default()
        };
        self.prepared = Some((key.clone(), content.clone()));
        let mut output = g::Scene::default();
        output.layers.frames.push(LayerFrame {
            key: layer,
            generation: self.generation,
            background: bg,
            tile_size: TILE,
            content: LayerContent::from(content),
            dirty_tiles: dirty,
        });
        for coord in &coords {
            output.insert_primitive(PolychromeSprite {
                order: 0,
                pad: 0,
                grayscale: false.into(),
                opacity: 1.,
                bounds: Bounds::new(
                    point(
                        ScaledPixels(coord.x as f32 * TILE as f32),
                        ScaledPixels(coord.y as f32 * TILE as f32),
                    ),
                    size(ScaledPixels(TILE as f32), ScaledPixels(TILE as f32)),
                ),
                content_mask: ContentMask { bounds: clip },
                corner_radii: Default::default(),
                tile: AtlasTile {
                    texture_id: layer_tile_texture_id(layer),
                    tile_id: layer_tile_id(*coord),
                    padding: 0,
                    bounds: Bounds::new(
                        point(DevicePixels(0), DevicePixels(0)),
                        size(DevicePixels(TILE as i32), DevicePixels(TILE as i32)),
                    ),
                },
            });
        }
        output.finish();
        Prepared {
            scene: output,
            key: Some(key),
            admission,
            fallback: None,
        }
    }
}
fn native(scene: g::Scene, reason: &'static str) -> Prepared {
    Prepared {
        scene,
        key: None,
        admission: Admission::default(),
        fallback: Some(reason),
    }
}
fn opaque_background(
    scene: &g::Scene,
    clip: Bounds<ScaledPixels>,
    bg: Rgba,
    viewport: &RasterViewport,
) -> bool {
    let canvas = viewport.canvas_bounds;
    let dpr = viewport.device_scale;
    let expected = Bounds::new(
        point(
            ScaledPixels(canvas.origin.x * dpr),
            ScaledPixels(canvas.origin.y * dpr),
        ),
        size(
            ScaledPixels(canvas.size.width * dpr),
            ScaledPixels(canvas.size.height * dpr),
        ),
    );
    scene.quads.first().is_some_and(|quad| {
        quad.background == Background::from(bg)
            && quad.corner_radii == Corners::default()
            && quad.border_widths == Edges::default()
            && quad.bounds == expected
            && quad.content_mask.bounds == clip
    })
}
fn visible_tiles(viewport: &RasterViewport) -> Option<(Bounds<ScaledPixels>, Vec<TileCoord>)> {
    let scale = viewport.device_scale;
    if !viewport.world_to_screen.is_finite()
        || !scale.is_finite()
        || scale <= 0.
        || !finite_world(viewport.canvas_bounds)
        || !finite_world(viewport.content_mask)
    {
        return None;
    }
    let a = viewport.canvas_bounds;
    let b = viewport.content_mask;
    let x0 = a.origin.x.max(b.origin.x) as f64 * scale as f64;
    let y0 = a.origin.y.max(b.origin.y) as f64 * scale as f64;
    let x1 = (a.origin.x + a.size.width).min(b.origin.x + b.size.width) as f64 * scale as f64;
    let y1 = (a.origin.y + a.size.height).min(b.origin.y + b.size.height) as f64 * scale as f64;
    // Fractional clipping can cover a pixel differently when rasterized into
    // an opaque tile and composited again. Keep that case on the direct path.
    if ![x0, y0, x1, y1]
        .into_iter()
        .all(|v| v.is_finite() && v.fract() == 0.)
        || x1 <= x0
        || y1 <= y0
    {
        return None;
    }
    let (left, top, right, bottom) = (
        (x0 / TILE as f64).floor() as i64,
        (y0 / TILE as f64).floor() as i64,
        (x1 / TILE as f64).ceil() as i64,
        (y1 / TILE as f64).ceil() as i64,
    );
    if left < -2048
        || top < -2048
        || right > 2048
        || bottom > 2048
        || (right - left) * (bottom - top) > MAX_TILES as i64
    {
        return None;
    }
    let coords = (top..bottom)
        .flat_map(|y| {
            (left..right).map(move |x| TileCoord {
                x: x as i32,
                y: y as i32,
            })
        })
        .collect();
    Some((
        Bounds::new(
            point(ScaledPixels(x0 as f32), ScaledPixels(y0 as f32)),
            size(
                ScaledPixels((x1 - x0) as f32),
                ScaledPixels((y1 - y0) as f32),
            ),
        ),
        coords,
    ))
}
fn damaged_tiles(
    bounds: WorldBounds,
    viewport: &RasterViewport,
    visible: &[TileCoord],
) -> Vec<TileCoord> {
    let transform =
        Affine::scale(viewport.device_scale, viewport.device_scale) * viewport.world_to_screen;
    let points = [
        bounds.origin,
        bounds.origin + gpui_flow_scene::Point::new(bounds.size.width, 0.),
        bounds.origin + gpui_flow_scene::Point::new(0., bounds.size.height),
        bounds.origin + gpui_flow_scene::Point::new(bounds.size.width, bounds.size.height),
    ]
    .map(|p| transform.transform_point(p));
    if points.iter().any(|p| !p.x.is_finite() || !p.y.is_finite()) {
        return visible.to_vec();
    }
    let x0 = points
        .iter()
        .map(|p| p.x as f64)
        .fold(f64::INFINITY, f64::min)
        - 2.;
    let y0 = points
        .iter()
        .map(|p| p.y as f64)
        .fold(f64::INFINITY, f64::min)
        - 2.;
    let x1 = points
        .iter()
        .map(|p| p.x as f64)
        .fold(f64::NEG_INFINITY, f64::max)
        + 2.;
    let y1 = points
        .iter()
        .map(|p| p.y as f64)
        .fold(f64::NEG_INFINITY, f64::max)
        + 2.;
    let dirty: BTreeSet<_> = visible
        .iter()
        .copied()
        .filter(|t| {
            let x = t.x as f64 * TILE as f64;
            let y = t.y as f64 * TILE as f64;
            x < x1 && x + TILE as f64 > x0 && y < y1 && y + TILE as f64 > y0
        })
        .collect();
    dirty.into_iter().collect()
}
