use super::RenderObject;
use gpui::{prelude::*, *};
use gpui_flow_layout::TextMeasureRequest;
use gpui_flow_scene::{
    Paint,
    display::{Text, TextAlign as SceneTextAlign},
};
use std::panic::{AssertUnwindSafe, catch_unwind};

struct TextPaintPlan {
    request: TextMeasureRequest,
    align: TextAlign,
    bounds: Bounds<Pixels>,
}
impl TextPaintPlan {
    fn new(text: &Text, m: gpui_flow_scene::Affine) -> Self {
        Self {
            request: TextMeasureRequest {
                content: text.content.clone(),
                font_family: text.font_family.clone(),
                font_size: text.font_size * m.d,
                font_weight: text.font_weight,
                line_height: text.font_size * text.line_height * m.d,
                wrap_width: text.wrap.then_some(text.size.width * m.a),
            },
            align: match text.align {
                SceneTextAlign::Start => TextAlign::Left,
                SceneTextAlign::Center => TextAlign::Center,
                SceneTextAlign::End => TextAlign::Right,
            },
            bounds: Bounds::new(
                point(px(m.tx), px(m.ty)),
                size(px(text.size.width * m.a), px(text.size.height * m.d)),
            ),
        }
    }
}

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
    let TextPaintPlan {
        request,
        align,
        bounds: model_bounds,
    } = TextPaintPlan::new(text, m);
    canvas(
        move |_, window, _| {
            catch_unwind(AssertUnwindSafe(|| {
                crate::text_measure::shape_scene_text(window.text_system(), &request, color.into())
            }))
            .unwrap_or_else(|_| Err("native text shaping panicked".into()))
            .map(|lines| (lines, px(request.line_height)))
        },
        move |bounds, result, window, cx| {
            let (lines, line_height) = match result {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("gpui-flow text rendering: {error}");
                    return;
                }
            };
            window.with_content_mask(Some(ContentMask { bounds }), |window| {
                let mut origin = bounds.origin;
                for line in lines {
                    if let Err(error) =
                        line.paint(origin, line_height, align, Some(bounds), window, cx)
                    {
                        eprintln!("gpui-flow text painting: {error}");
                    }
                    origin.y += line.size(line_height).height;
                }
            });
        },
    )
    .absolute()
    .left(model_bounds.origin.x)
    .top(model_bounds.origin.y)
    .w(model_bounds.size.width)
    .h(model_bounds.size.height)
    .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_scene::{Affine, Size};
    #[::core::prelude::v1::test]
    fn paint_plan_keeps_alignment_clipping_and_wrap_in_same_screen_coordinates() {
        let mut text = Text::new("A\n", Size::new(90., 40.));
        text.font_size = 20.;
        text.line_height = 1.4;
        text.font_weight = 700;
        for (model_align, native_align) in [
            (SceneTextAlign::Start, TextAlign::Left),
            (SceneTextAlign::Center, TextAlign::Center),
            (SceneTextAlign::End, TextAlign::Right),
        ] {
            text.align = model_align;
            let canonical = text.clone();
            for zoom in [0.5, 2., 1.] {
                let p = TextPaintPlan::new(
                    &text,
                    Affine::translation(13., 17.) * Affine::scale(zoom, zoom),
                );
                assert_eq!(p.align, native_align);
                assert_eq!(p.request.wrap_width, Some(90. * zoom));
                assert_eq!(p.request.line_height, 28. * zoom);
                assert_eq!(p.bounds.origin, point(px(13.), px(17.)));
                assert_eq!(p.bounds.size, size(px(90. * zoom), px(40. * zoom)));
                assert_eq!(text, canonical);
            }
        }
        text.wrap = false;
        assert_eq!(
            TextPaintPlan::new(&text, Affine::IDENTITY)
                .request
                .wrap_width,
            None
        );
    }
}
