//! Rasterizing a scroll layer tile: the layer's content over the tile,
//! translated into the tile's space, drawn into the tile's texture cleared
//! to the layer's baked background, with the frame's pipelines. Generic
//! content-space layers use a viewport the size of the tile.
//!
//! An explicitly window-space producer retains original path vertices and
//! the full-window projection, with the viewport translated onto the tile.
//! This preserves direct multisample coverage without rounding geometry;
//! fragment clipping and shading still use tile-local positions and bounds.

use anyhow::{Context as _, Result};
use gpui::{Path, Rgba, ScaledPixels, Scene, SceneLayers, TileCoord};

use crate::fast::frame::{
    FrameState, FrameTarget, PathBatch, PathTargets, PathUpload, SceneDraw, SceneUpload, draw_scene,
};

/// A tile to rasterize this frame, with the part of its layer's content
/// that covers it.
pub(crate) struct TileRaster<'a> {
    /// The index of the tile's layer in the frame's `SceneLayers::frames`.
    pub(crate) layer: usize,
    pub(crate) tile: TileCoord,
    pub(crate) scene: Scene,
    /// In tile PathId (paint insertion) order. Keep original vertices rather
    /// than undoing the tile translation, which can lose floating-point bits.
    pub(crate) window_paths: Option<Vec<&'a Path<ScaledPixels>>>,
}

/// The scenes of the tiles `planned` by `TileCache::begin_frame`.
pub(crate) fn plan<'a>(
    layers: &'a SceneLayers,
    planned: &[(usize, TileCoord)],
    window_space: bool,
) -> Result<Vec<TileRaster<'a>>> {
    planned
        .iter()
        .map(|&(layer, tile)| {
            let frame = &layers.frames[layer];
            let scene = frame.tile_scene(tile);
            let window_paths = if window_space {
                let source = frame
                    .content
                    .scene()
                    .context("window-space layer requires single-scene content")?;
                let bounds = frame.tile_bounds(tile);
                let mut paths: Vec<_> = source
                    .paths
                    .iter()
                    .filter(|path| path.clipped_bounds().intersects(&bounds))
                    .collect();
                paths.sort_by_key(|path| path.id.0);
                anyhow::ensure!(
                    paths.len() == scene.paths.len(),
                    "tile path selection changed"
                );
                for path in &scene.paths {
                    anyhow::ensure!(
                        paths
                            .get(path.id.0)
                            .is_some_and(|source| source.vertices.len() == path.vertices.len()),
                        "tile path identity changed",
                    );
                }
                Some(paths)
            } else {
                None
            };
            Ok(TileRaster {
                layer,
                tile,
                scene,
                window_paths,
            })
        })
        .collect()
}

/// The clear colour a tile over `background` starts from: the bytes an
/// opaque quad of that colour writes to the frame's non-sRGB texture.
pub(crate) fn clear_color(background: Rgba) -> wgpu::Color {
    wgpu::Color {
        r: background.r as f64,
        g: background.g as f64,
        b: background.b as f64,
        a: background.a as f64,
    }
}

/// Records the passes that rasterize `tile` into its texture, whose scene's
/// instance data is `upload` and whose paths are `path_batches` of `paths`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn rasterize(
    target: &FrameTarget,
    state: &FrameState,
    encoder: &mut wgpu::CommandEncoder,
    layers: &SceneLayers,
    tile: &TileRaster<'_>,
    upload: &SceneUpload,
    paths: &PathUpload,
    path_batches: &[PathBatch],
) {
    let layer = &layers.frames[tile.layer];
    let cache = &state.layers;
    let (Some(view), Some(globals), Some(tile_paths)) = (
        cache.texture(layer.key, tile.tile),
        cache.globals(layer.tile_size),
        cache.paths(layer.tile_size),
    ) else {
        debug_assert!(false, "a planned tile was not prepared");
        return;
    };
    draw_scene(
        target,
        state,
        encoder,
        &SceneDraw {
            scene: &tile.scene,
            upload,
            paths,
            path_batches,
            view,
            clear: clear_color(layer.background),
            label: "layer_tile_pass",
            globals: &globals.globals,
            path_targets: PathTargets {
                globals: if tile.window_paths.is_some() {
                    target.path_globals_bind_group
                } else {
                    &globals.path_globals
                },
                intermediate: Some(&tile_paths.intermediate),
                msaa: tile_paths.msaa.as_ref(),
                viewport: tile.window_paths.as_ref().map(|_| {
                    let origin = layer.tile_bounds(tile.tile).origin;
                    [
                        -origin.x.0,
                        -origin.y.0,
                        target.viewport_size[0],
                        target.viewport_size[1],
                    ]
                }),
            },
        },
    );
}
