use super::RenderObject;
use gpui::{prelude::*, *};
use gpui_flow_scene::{
    Paint,
    display::{Text, TextAlign},
};
pub(super) fn text_element(text: &Text, object: &RenderObject) -> AnyElement {
    let m = object.screen_transform;
    let color = match &object.object.style.fill {
        Some(Paint::Solid { color }) => Rgba {
            r: color.r as f32 / 255.,
            g: color.g as f32 / 255.,
            b: color.b as f32 / 255.,
            a: color.a as f32 / 255. * object.opacity,
        },
        _ => rgba(0x00000000),
    };
    div()
        .absolute()
        .left(px(m.tx))
        .top(px(m.ty))
        .w(px(text.size.width * m.a))
        .h(px(text.size.height * m.d))
        .overflow_hidden()
        .font_family(SharedString::from(text.font_family.clone()))
        .text_size(px(text.font_size * m.d))
        .line_height(relative(text.line_height))
        .text_color(color)
        .when(!text.wrap, |d| d.whitespace_nowrap())
        .when(text.align == TextAlign::Center, |d| d.text_center())
        .when(text.align == TextAlign::End, |d| d.text_right())
        .child(text.content.clone())
        .into_any_element()
}
