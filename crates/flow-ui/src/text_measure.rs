//! Compatibility export for existing `gpui_flow::GpuiTextMeasurer` callers.
pub(crate) use gpui_flow_text::gpui_backend::shape_text as shape_scene_text;
pub use gpui_flow_text::{GpuiTextMeasurer, GpuiTextRasterizer};
