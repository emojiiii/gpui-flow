//! Compatibility export for existing `gpui_flow::GpuiTextMeasurer` callers.
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

pub(crate) fn text_overflow(
    overflow: gpui_flow_scene::display::TextOverflow,
) -> gpui_flow_text::TextOverflow {
    match overflow {
        gpui_flow_scene::display::TextOverflow::Clip => gpui_flow_text::TextOverflow::Clip,
        gpui_flow_scene::display::TextOverflow::Ellipsis => gpui_flow_text::TextOverflow::Ellipsis,
    }
}
