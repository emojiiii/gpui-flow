use super::{RenderObject, rect_clip};
use gpui::{prelude::*, *};
use gpui_flow_scene::{
    Bounds as SceneBounds, Point as ScenePoint, Size as SceneSize,
    display::{Image, ImageFit},
};

/// An outer clip never changes the full image layout/fit box. The inner element
/// is translated relative to that wrapper, including negative offsets.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ImageBox {
    visible: SceneBounds,
    layout: SceneBounds,
    offset: ScenePoint,
}
fn image_box(image: &Image, object: &RenderObject) -> Option<ImageBox> {
    let m = object.screen_transform;
    let layout = SceneBounds::new(
        ScenePoint::new(m.tx, m.ty),
        SceneSize::new(image.size.width * m.a, image.size.height * m.d),
    );
    let visible = rect_clip::plan(object).ok()?.visible(layout)?;
    Some(ImageBox {
        visible,
        layout,
        offset: layout.origin - visible.origin,
    })
}
fn native_fit(fit: ImageFit) -> ObjectFit {
    match fit {
        ImageFit::Contain => ObjectFit::Contain,
        ImageFit::Cover => ObjectFit::Cover,
        ImageFit::Fill => ObjectFit::Fill,
    }
}
pub(super) fn image_element(
    image: &Image,
    object: &RenderObject,
    prepared: Option<crate::resources::SceneImagePresentation>,
    frame_index: Option<usize>,
) -> AnyElement {
    let Some(plan) = image_box(image, object) else {
        return div().into_any_element();
    };
    let fit = native_fit(image.fit);
    let [tl, tr, br, bl] = image
        .effective_corners()
        .0
        .map(|r| px(r * object.screen_transform.a));
    let corners = Corners {
        top_left: tl,
        top_right: tr,
        bottom_right: br,
        bottom_left: bl,
    };
    let content = div()
        .absolute()
        .left(px(plan.offset.x))
        .top(px(plan.offset.y))
        .w(px(plan.layout.size.width))
        .h(px(plan.layout.size.height))
        .overflow_hidden()
        .opacity(object.opacity);
    use crate::resources::SceneImagePresentation;
    let native_id = SharedString::from(format!("flow-image:{}:{}", object.object.id, image.source));
    let content = match prepared {
        Some(SceneImagePresentation::Loaded(image)) => {
            if let Some(index) = frame_index {
                content
                    .child(
                        canvas(
                            |_, _, _| (),
                            move |bounds, _, window, _| {
                                let fitted = fit.get_bounds(bounds, image.size(index));
                                let _ = window
                                    .paint_image(bounds, fitted, corners, image, index, false);
                            },
                        )
                        .size_full(),
                    )
                    .into_any_element()
            } else {
                content
                    .child(
                        img(image.clone())
                            .id(SharedString::from(format!("{native_id}:{:?}", image.id)))
                            .size_full()
                            .object_fit(fit)
                            .rounded_tl(tl)
                            .rounded_tr(tr)
                            .rounded_br(br)
                            .rounded_bl(bl),
                    )
                    .into_any_element()
            }
        }
        Some(SceneImagePresentation::Pending) => content.child("Loading image…").into_any_element(),
        Some(SceneImagePresentation::Failed(error)) => content
            .text_color(rgb(0xf5b971))
            .child(format!("Image failed: {error}"))
            .into_any_element(),
        Some(SceneImagePresentation::Cancelled) => content
            .text_color(rgb(0xf5b971))
            .child("Image not authorized by host")
            .into_any_element(),
        None => match crate::resources::local_image_resource(&image.source) {
            Some(resource) => content
                .child(
                    img(ImageSource::Resource(resource))
                        .id(native_id)
                        .size_full()
                        .object_fit(fit)
                        .rounded_tl(tl)
                        .rounded_tr(tr)
                        .rounded_br(br)
                        .rounded_bl(bl),
                )
                .into_any_element(),
            None => content
                .text_color(rgb(0xf5b971))
                .child("Remote image: provide a host asset loader")
                .into_any_element(),
        },
    };
    div()
        .absolute()
        .left(px(plan.visible.origin.x))
        .top(px(plan.visible.origin.y))
        .w(px(plan.visible.size.width))
        .h(px(plan.visible.size.height))
        .overflow_hidden()
        .child(content)
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_flow_scene::{Affine, ClipRegion, DisplayObject};
    #[test]
    fn clipping_keeps_full_fit_box_and_negative_offsets_for_every_fit_and_zoom() {
        let mut image = Image::new("local.png", SceneSize::new(200., 100.));
        image.natural_size = Some(SceneSize::new(100., 200.));
        for fit in [ImageFit::Contain, ImageFit::Cover, ImageFit::Fill] {
            for zoom in [0.5, 1., 2.] {
                image.fit = fit;
                let viewport = Affine::translation(17., -11.) * Affine::scale(zoom, zoom);
                let object = RenderObject {
                    object: DisplayObject::new("image", image.clone()),
                    screen_transform: viewport * Affine::translation(10., 20.),
                    viewport_transform: viewport,
                    opacity: 1.,
                    clips: vec![ClipRegion {
                        corner_radii: None,
                        local_bounds: SceneBounds::new(ScenePoint::ZERO, SceneSize::new(80., 60.)),
                        corner_radius: 0.,
                        world_transform: Affine::translation(50., 30.),
                    }],
                };
                let plan = image_box(&image, &object).unwrap();
                assert_eq!(plan.layout.size, SceneSize::new(200. * zoom, 100. * zoom));
                assert_eq!(plan.offset, ScenePoint::new(-40. * zoom, -10. * zoom));
                assert_eq!(plan.visible.size, SceneSize::new(80. * zoom, 60. * zoom));
                let bounds = Bounds::new(
                    point(px(plan.layout.origin.x), px(plan.layout.origin.y)),
                    size(px(plan.layout.size.width), px(plan.layout.size.height)),
                );
                let fitted = native_fit(fit).get_bounds(
                    bounds,
                    size(DevicePixels::from(100u32), DevicePixels::from(200u32)),
                );
                let wrapper_local =
                    Bounds::new(point(px(plan.offset.x), px(plan.offset.y)), bounds.size);
                let mut inner_fit = native_fit(fit).get_bounds(
                    wrapper_local,
                    size(DevicePixels::from(100u32), DevicePixels::from(200u32)),
                );
                inner_fit.origin += point(px(plan.visible.origin.x), px(plan.visible.origin.y));
                assert_eq!(
                    fitted, inner_fit,
                    "clipping must not recompute cover/contain using the smaller wrapper"
                );
                let expected = object
                    .screen_transform
                    .transform_bounds(image.content_bounds());
                assert_eq!(
                    fitted.origin,
                    point(px(expected.origin.x), px(expected.origin.y))
                );
                assert_eq!(
                    fitted.size,
                    size(px(expected.size.width), px(expected.size.height))
                );
            }
        }
    }
    #[test]
    fn completely_clipped_image_has_no_layout_element() {
        let image = Image::new("local.png", SceneSize::new(40., 30.));
        let object = RenderObject {
            object: DisplayObject::new("image", image.clone()),
            screen_transform: Affine::IDENTITY,
            viewport_transform: Affine::IDENTITY,
            opacity: 1.,
            clips: vec![ClipRegion {
                corner_radii: None,
                local_bounds: SceneBounds::new(ScenePoint::ZERO, SceneSize::new(10., 10.)),
                corner_radius: 0.,
                world_transform: Affine::translation(40., 0.),
            }],
        };
        assert!(image_box(&image, &object).is_none());
    }
}
