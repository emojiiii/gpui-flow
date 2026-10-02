//! Bounded, toolkit-independent vector/image/text-ink isolation using tiny-skia 0.11.4.
//!
//! A domain is all-or-nothing: unsupported descendants, unsafe coordinates or
//! exhausted budgets return an error before an output can be published. Native
//! custom renderers must remain outside these domains. Image and text ink pixels
//! are supplied by the host, never fetched, decoded or shaped by this crate.
mod canvas_surface;
mod image;
mod nonseparable;
mod plan;
mod raster;
mod scene_export;
mod scene_svg;
mod shadow;
mod text;
mod vector;
pub use canvas_surface::*;
pub use image::*;
pub use plan::*;
pub use raster::*;
pub use scene_export::*;
pub use text::*;

use gpui_flow_scene::DisplayId;
use std::fmt;

/// Fixed safety ceilings. Callers may reduce them, never increase them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompositeLimits {
    pub max_dimension: u32,
    pub max_pixels: u64,
    pub max_surface_bytes: u64,
    /// Canvas path storage/candidates/snapshots, separate from pixel memory.
    pub max_path_bytes: u64,
    pub max_surface_writes: u64,
    pub max_nodes: usize,
    pub max_domains: usize,
    pub max_points: usize,
    pub max_total_points: usize,
    pub max_triangles: usize,
    pub max_depth: usize,
    pub max_mask_runs: usize,
}
impl Default for CompositeLimits {
    fn default() -> Self {
        Self {
            max_dimension: 4096,
            max_pixels: 4_194_304,
            max_surface_bytes: 128 * 1024 * 1024,
            max_path_bytes: 16 * 1024 * 1024,
            max_surface_writes: 32_000_000,
            max_nodes: 512,
            max_domains: 8,
            max_points: 65_536,
            max_total_points: 262_144,
            max_triangles: 262_144,
            max_depth: 16,
            max_mask_runs: 64,
        }
    }
}
impl CompositeLimits {
    pub fn bounded(self) -> Self {
        let ceiling = Self::default();
        Self {
            max_dimension: self.max_dimension.min(ceiling.max_dimension),
            max_pixels: self.max_pixels.min(ceiling.max_pixels),
            max_surface_bytes: self.max_surface_bytes.min(ceiling.max_surface_bytes),
            max_path_bytes: self.max_path_bytes.min(ceiling.max_path_bytes),
            max_surface_writes: self.max_surface_writes.min(ceiling.max_surface_writes),
            max_nodes: self.max_nodes.min(ceiling.max_nodes),
            max_domains: self.max_domains.min(ceiling.max_domains),
            max_points: self.max_points.min(ceiling.max_points),
            max_total_points: self.max_total_points.min(ceiling.max_total_points),
            max_triangles: self.max_triangles.min(ceiling.max_triangles),
            max_depth: self.max_depth.min(ceiling.max_depth),
            max_mask_runs: self.max_mask_runs.min(ceiling.max_mask_runs),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CompositeError {
    UnsupportedShape(String),
    ImagePending(DisplayId),
    ImageUnavailable {
        image: DisplayId,
        reason: String,
    },
    UnsupportedAnimatedImage,
    InvalidImage,
    ImageEncoding(String),
    TextBackendRequired(DisplayId),
    UnsupportedTextStyle(DisplayId),
    TextRasterization {
        text: DisplayId,
        reason: String,
    },
    UnsupportedExternalBackdrop,
    UnsupportedTransform,
    UnsupportedShadow(DisplayId),
    /// Custom producers do not expose a supported inner-shadow alpha source.
    UnsupportedInnerShadow(DisplayId),
    /// A shadow would isolate a pass-through container whose descendants need
    /// its external backdrop. Select Normal explicitly or isolate those blends.
    UnsupportedShadowBackdrop(DisplayId),
    InvalidGeometry(String),
    TooLarge,
    WorkingSetLimit,
    WorkLimit,
    InputLimit,
    NestingLimit,
    AllocationFailed,
    Superseded,
    /// A previous complete frame is temporarily shown at unchanged placement.
    StaleFrame,
    UploadFailed(String),
}
impl fmt::Display for CompositeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CompositeError {}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositeIssue {
    pub object: DisplayId,
    pub error: CompositeError,
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod canvas_tests;

#[cfg(test)]
mod shadow_tests;

#[cfg(test)]
mod container_phase_tests;

#[cfg(test)]
mod group_shadow_tests;

#[cfg(test)]
mod multi_shadow_tests;

#[cfg(test)]
mod inner_shadow_tests;

#[cfg(test)]
mod container_inner_shadow_tests;

#[cfg(test)]
mod ellipse_tests;

#[cfg(test)]
mod rect_corner_tests;

#[cfg(test)]
mod container_corner_tests;

#[cfg(test)]
mod canvas_component_tests;

#[cfg(test)]
mod image_corner_tests;

#[cfg(test)]
mod text_clip_tests;

#[cfg(test)]
mod canvas_pixel_tests;

#[cfg(test)]
mod canvas_command_tests;

#[cfg(test)]
mod text_affine_tests;

#[cfg(test)]
mod canvas_image_command_tests;

#[cfg(test)]
mod canvas_text_command_tests;

#[cfg(test)]
mod scene_export_tests;

#[cfg(test)]
mod scene_svg_tests;

#[cfg(test)]
mod canvas_current_path_tests;

#[cfg(test)]
mod canvas_pattern_command_tests;
