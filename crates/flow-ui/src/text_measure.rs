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

pub(crate) fn text_decoration(
    value: gpui_flow_scene::display::TextDecoration,
) -> gpui_flow_text::TextDecoration {
    use gpui_flow_scene::display::TextDecoration as Scene;
    use gpui_flow_text::TextDecoration as Paint;
    match value {
        Scene::None => Paint::None,
        Scene::Underline => Paint::Underline,
        Scene::Strikethrough => Paint::Strikethrough,
        Scene::UnderlineStrikethrough => Paint::UnderlineStrikethrough,
    }
}

pub(crate) fn text_case(value: gpui_flow_scene::display::TextCase) -> gpui_flow_text::TextCase {
    use gpui_flow_scene::display::TextCase as Scene;
    match value {
        Scene::None => gpui_flow_text::TextCase::None,
        Scene::Upper => gpui_flow_text::TextCase::Upper,
        Scene::Lower => gpui_flow_text::TextCase::Lower,
    }
}
