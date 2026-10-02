//! Compatibility export for existing `gpui_flow::GpuiTextMeasurer` callers.
pub(crate) use gpui_flow_text::gpui_backend::shape_text as shape_scene_text;
pub use gpui_flow_text::{GpuiTextMeasurer, GpuiTextRasterizer};

pub(crate) fn vertical_align(
    align: gpui_flow_scene::display::TextVerticalAlign,
) -> gpui_flow_text::VerticalAlign {
    use gpui_flow_scene::display::TextVerticalAlign as Scene;
    match align {
        Scene::Top => gpui_flow_text::VerticalAlign::Top,
        Scene::Middle => gpui_flow_text::VerticalAlign::Middle,
        Scene::Bottom => gpui_flow_text::VerticalAlign::Bottom,
    }
}
