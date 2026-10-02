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
    vertical_align: gpui_flow_text::VerticalAlign,
    overflow: gpui_flow_text::TextOverflow,
    bounds: Bounds<Pixels>,
}
impl TextPaintPlan {
    fn new(text: &Text, m: gpui_flow_scene::Affine) -> Self {
        let content = text.content_bounds();
        Self {
            vertical_align: crate::text_measure::vertical_align(text.vertical_align),
            overflow: crate::text_measure::text_overflow(text.overflow),
            request: TextMeasureRequest {
                content: text.content.clone(),
                font_family: text.font_family.clone(),
                font_size: text.font_size * m.d,
                font_weight: text.font_weight,
                line_height: text.font_size * text.line_height * m.d,
                wrap_width: text.wrap.then_some(content.size.width * m.a),
            },
            align: match text.align {
                SceneTextAlign::Start => TextAlign::Left,
                SceneTextAlign::Center => TextAlign::Center,
                SceneTextAlign::End => TextAlign::Right,
            },
            bounds: Bounds::new(
                point(
                    px(m.tx + content.origin.x * m.a),
                    px(m.ty + content.origin.y * m.d),
                ),
                size(px(content.size.width * m.a), px(content.size.height * m.d)),
            ),
        }
    }
}

fn native_mask(
    visible: gpui_flow_scene::Bounds,
    model_origin: Point<Pixels>,
    actual_bounds: Bounds<Pixels>,
) -> ContentMask<Pixels> {
    let offset = actual_bounds.origin - model_origin;
    ContentMask {
        bounds: Bounds::new(
            point(px(visible.origin.x), px(visible.origin.y)) + offset,
            size(px(visible.size.width), px(visible.size.height)),
        ),
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
        vertical_align,
        overflow,
        bounds: model_bounds,
    } = TextPaintPlan::new(text, m);
    let own_bounds = gpui_flow_scene::Bounds::new(
        gpui_flow_scene::Point::new(model_bounds.origin.x.into(), model_bounds.origin.y.into()),
        gpui_flow_scene::Size::new(
            model_bounds.size.width.into(),
            model_bounds.size.height.into(),
        ),
    );
    let Ok(clip) = super::rect_clip::plan(object) else {
        return div().into_any_element();
    };
    let Some(visible) = clip.visible(own_bounds) else {
        return div().into_any_element();
    };
    canvas(
        move |_, window, _| {
            catch_unwind(AssertUnwindSafe(|| {
                gpui_flow_text::gpui_overflow::shape_text_in_box(
                    window.text_system(),
                    &request,
                    color.into(),
                    overflow,
                    model_bounds.size.width.into(),
                    model_bounds.size.height.into(),
                )
            }))
            .unwrap_or_else(|_| Err("native text shaping panicked".into()))
            .map(|shaped| (shaped, px(request.line_height)))
        },
        move |bounds, result, window, cx| {
            let (shaped, line_height) = match result {
                Ok(value) => value,
                Err(error) => {
                    eprintln!("gpui-flow text rendering: {error}");
                    return;
                }
            };
            // canvas bounds includes the host's origin; clip plans are viewport
            // relative. Translate both by the same origin before intersection.
            let mask = native_mask(visible, model_bounds.origin, bounds);
            // GPUI intersects this with the existing host content mask.
            window.with_content_mask(Some(mask), |window| {
                let block_height = shaped.block_height;
                let mut origin = bounds.origin;
                origin.y += px(vertical_align.offset(bounds.size.height.into(), block_height));
                for line in shaped.lines {
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
    #[::core::prelude::v1::test]
    fn native_mask_translates_host_origin_and_intersects_existing_occlusion() {
        let visible = gpui_flow_scene::Bounds::new(
            gpui_flow_scene::Point::new(20., 30.),
            Size::new(40., 20.),
        );
        let original = Bounds::new(point(px(310.), px(420.)), size(px(100.), px(50.)));
        let mask = native_mask(visible, point(px(10.), px(20.)), original);
        assert_eq!(
            mask.bounds,
            Bounds::new(point(px(320.), px(430.)), size(px(40.), px(20.)))
        );
        let host = ContentMask {
            bounds: Bounds::new(point(px(330.), px(435.)), size(px(10.), px(5.))),
        };
        assert_eq!(host.intersect(&mask), host);
        assert_eq!(mask.intersect(&host), host);
        assert_eq!(
            original.size,
            size(px(100.), px(50.)),
            "clip does not change text alignment box"
        );
    }
    #[::core::prelude::v1::test]
    fn text_box_padding_preserves_inner_alignment_frame_under_pan_and_zoom() {
        use gpui_flow_scene::display::TextVerticalAlign;
        let mut text = Text::new("wrapped content", Size::new(100., 60.));
        text.padding = [3., 5., 7., 11.];
        text.vertical_align = TextVerticalAlign::Bottom;
        for zoom in [0.5, 1., 2.] {
            let p = TextPaintPlan::new(
                &text,
                Affine::translation(13., 17.) * Affine::scale(zoom, zoom),
            );
            assert_eq!(p.request.wrap_width, Some(84. * zoom));
            assert_eq!(
                p.bounds.origin,
                point(px(13. + 11. * zoom), px(17. + 3. * zoom))
            );
            assert_eq!(p.bounds.size, size(px(84. * zoom), px(50. * zoom)));
            assert_eq!(p.vertical_align, gpui_flow_text::VerticalAlign::Bottom);
        }
    }
}
