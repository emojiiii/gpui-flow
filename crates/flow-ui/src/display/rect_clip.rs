//! Shared exact rectangular ancestor clipping for native Text and Image.
use super::RenderObject;
use gpui_flow_scene::{Affine, Bounds, Point, Size};

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum RectClip {
    Unclipped,
    Rect(Bounds),
    Empty,
}
pub(super) fn supports_transform(m: Affine) -> bool {
    [m.a, m.b, m.c, m.d, m.tx, m.ty]
        .iter()
        .all(|v| v.is_finite())
        && m.b == 0.
        && m.c == 0.
        && m.a > 0.
        && m.a == m.d
}
fn intersection(a: Bounds, b: Bounds) -> Option<Bounds> {
    let x = a.min_x().max(b.min_x());
    let y = a.min_y().max(b.min_y());
    let right = a.max_x().min(b.max_x());
    let bottom = a.max_y().min(b.max_y());
    (right > x && bottom > y)
        .then(|| Bounds::new(Point::new(x, y), Size::new(right - x, bottom - y)))
}
impl RectClip {
    /// Also intersect the element's own explicit overflow rectangle. Layout and
    /// image-fit/text-alignment continue to use the original, unclipped rectangle.
    pub(super) fn visible(self, bounds: Bounds) -> Option<Bounds> {
        if !bounds.is_valid() || !bounds.size.is_positive() {
            return None;
        }
        match self {
            Self::Unclipped => Some(bounds),
            Self::Rect(clip) => intersection(bounds, clip),
            Self::Empty => None,
        }
    }
}
pub(super) fn plan(object: &RenderObject) -> Result<RectClip, ()> {
    let mut result = RectClip::Unclipped;
    for clip in &object.clips {
        let transform = object.viewport_transform * clip.world_transform;
        // Never replace curved/rotated/skewed clips with their bounding box.
        if !clip.is_rectangular() || !supports_transform(transform) || !clip.local_bounds.is_valid()
        {
            return Err(());
        }
        let bounds = transform.transform_bounds(clip.local_bounds);
        if !bounds.is_valid() {
            return Err(());
        }
        result = if !bounds.size.is_positive() {
            RectClip::Empty
        } else {
            match result {
                RectClip::Unclipped => RectClip::Rect(bounds),
                RectClip::Rect(previous) => {
                    intersection(previous, bounds).map_or(RectClip::Empty, RectClip::Rect)
                }
                RectClip::Empty => RectClip::Empty,
            }
        };
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::UnsupportedFeature;
    use gpui_flow_core::Viewport;
    use gpui_flow_engine::SceneEngine;
    use gpui_flow_scene::{
        ClipRegion, DisplayObject, HitPolicy, HitTestOptions, Scene, Shape, display,
    };
    fn object() -> RenderObject {
        let mut value = DisplayObject::new("text", display::Text::new("clip", Size::new(80., 70.)));
        value.hit_policy = HitPolicy::Bounds;
        RenderObject {
            object: value,
            screen_transform: Affine::IDENTITY,
            viewport_transform: Affine::IDENTITY,
            opacity: 1.,
            clips: vec![],
        }
    }
    fn clip(x: f32, y: f32, w: f32, h: f32) -> ClipRegion {
        ClipRegion {
            corner_radii: None,
            local_bounds: Bounds::new(Point::ZERO, Size::new(w, h)),
            corner_radius: 0.,
            world_transform: Affine::translation(x, y),
        }
    }
    #[test]
    fn nested_rectangles_and_nonzero_local_origins_use_viewport_once() {
        let mut value = object();
        value.viewport_transform = Affine::translation(11., -7.) * Affine::scale(2., 2.);
        value.clips = vec![clip(10., 20., 100., 80.), clip(60., 30., 80., 60.)];
        value.clips[1].local_bounds.origin = Point::new(5., 5.);
        assert_eq!(
            plan(&value).unwrap(),
            RectClip::Rect(Bounds::new(Point::new(141., 63.), Size::new(90., 120.)))
        );
        assert!(
            !value
                .diagnostics()
                .iter()
                .any(|x| x.feature == UnsupportedFeature::ClippedText)
        );
        value.object.shape = display::Image::new("local.png", Size::new(80., 70.)).into();
        assert!(
            !value
                .diagnostics()
                .iter()
                .any(|x| x.feature == UnsupportedFeature::ClippedImage)
        );
    }
    #[test]
    fn disjoint_touching_and_own_box_intersections_are_empty() {
        let mut value = object();
        for second_x in [100., 110.] {
            value.clips = vec![clip(0., 0., 100., 100.), clip(second_x, 0., 100., 100.)];
            assert_eq!(plan(&value).unwrap(), RectClip::Empty);
        }
        value.clips = vec![clip(100., 0., 100., 100.)];
        assert_eq!(
            plan(&value)
                .unwrap()
                .visible(Bounds::new(Point::ZERO, Size::new(100., 100.))),
            None
        );
        value.clips.clear();
        assert_eq!(plan(&value).unwrap(), RectClip::Unclipped);
    }
    #[test]
    fn curved_skewed_mirrored_and_nonuniform_clips_remain_explicit_errors() {
        let mut value = object();
        value.clips.push(clip(0., 0., 100., 100.));
        for transform in [
            Affine::rotation(30.),
            Affine::scale(-1., -1.),
            Affine::scale(1., 2.),
            Affine {
                c: 0.000001,
                ..Affine::IDENTITY
            },
        ] {
            value.clips[0].world_transform = transform;
            assert!(plan(&value).is_err());
        }
        value.clips[0].world_transform = Affine::IDENTITY;
        value.clips[0].corner_radius = 0.001;
        assert!(plan(&value).is_err());
        value.clips.insert(0, clip(1000., 1000., 10., 10.));
        assert!(
            plan(&value).is_err(),
            "empty intersection does not suppress an unsupported clip"
        );
        value.clips.clear();
        value.screen_transform.c = 0.000001;
        assert!(
            value
                .diagnostics()
                .iter()
                .any(|x| x.feature == UnsupportedFeature::TransformedText)
        );
    }
    fn fixture(image: bool) -> Scene {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("outer", display::Frame::new(100., 80.))
                    .with_transform(Affine::translation(10., 20.) * Affine::scale(2., 2.)),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new("inner", display::Frame::new(90., 70.))
                    .with_transform(Affine::translation(60., 10.) * Affine::scale(0.5, 0.5)),
                Some("outer".into()),
                None,
            )
            .unwrap();
        let shape: Shape = if image {
            display::Image::new("local.png", Size::new(120., 100.)).into()
        } else {
            display::Text::new("clip", Size::new(120., 100.)).into()
        };
        let mut leaf =
            DisplayObject::new("leaf", shape).with_transform(Affine::translation(-20., -15.));
        leaf.hit_policy = HitPolicy::Bounds;
        scene.insert(leaf, Some("inner".into()), None).unwrap();
        scene
    }
    #[test]
    fn screen_clip_plan_matches_scene_and_engine_hits_across_zoom_and_pan() {
        for image in [false, true] {
            let scene = fixture(image);
            let engine = SceneEngine::new(scene.clone()).unwrap();
            for viewport in [
                Viewport::default(),
                Viewport {
                    pan: Point::new(-17., 33.),
                    zoom: 0.5,
                    ..Default::default()
                },
                Viewport {
                    pan: Point::new(20., -9.),
                    zoom: 2.,
                    ..Default::default()
                },
            ] {
                let item = engine
                    .paint_list()
                    .unwrap()
                    .into_iter()
                    .find(|p| p.object.id.as_str() == "leaf")
                    .unwrap();
                let render = RenderObject::from_paint_item(&item, viewport);
                let own = render
                    .screen_transform
                    .transform_bounds(item.object.shape.local_bounds().unwrap());
                let visible = plan(&render).unwrap().visible(own).unwrap();
                assert!(render.diagnostics().is_empty());
                for x in (80..260).step_by(5) {
                    for y in (10..150).step_by(5) {
                        let world = Point::new(x as f32, y as f32);
                        let screen = render.viewport_transform.transform_point(world);
                        let expected = visible.contains(screen);
                        for hits in [
                            scene
                                .hit_test_all(world, HitTestOptions::default())
                                .unwrap(),
                            engine
                                .hit_test_all(world, HitTestOptions::default())
                                .unwrap(),
                        ] {
                            assert_eq!(
                                hits.iter().any(|h| h.id.as_str() == "leaf"),
                                expected,
                                "image={image}, world={world:?}, zoom={}",
                                viewport.zoom
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn unclipped_parent_is_absent_and_clip_toggle_changes_plan_without_document_rewrite() {
        let mut scene = fixture(false);
        let before = scene.clone();
        let get = |s: &Scene| {
            let items = s.paint_list().unwrap();
            let item = items
                .iter()
                .find(|i| i.object.id.as_str() == "leaf")
                .unwrap();
            plan(&RenderObject::from_paint_item(item, Viewport::default())).unwrap()
        };
        let clipped = get(&scene);
        scene
            .update_object(&"outer".into(), |o| {
                let Shape::Frame(f) = &mut o.shape else {
                    panic!()
                };
                f.clip_children = false;
            })
            .unwrap();
        assert_ne!(get(&scene), clipped);
        assert_eq!(get(&before), clipped);
    }
    #[test]
    fn native_media_rejects_any_rounded_corner_but_accepts_explicit_zero_override() {
        let mut value = object();
        value.clips = vec![clip(0., 0., 100., 80.)];
        value.clips[0].corner_radii = Some([15., 0., 0., 0.].into());
        assert!(plan(&value).is_err());
        assert!(
            value
                .diagnostics()
                .iter()
                .any(|issue| issue.feature == UnsupportedFeature::ClippedText)
        );
        value.object.shape = display::Image::new("unused.png", Size::new(80., 70.)).into();
        assert!(
            value
                .diagnostics()
                .iter()
                .any(|issue| issue.feature == UnsupportedFeature::ClippedImage)
        );
        value.clips[0].corner_radius = 7.;
        value.clips[0].corner_radii = Some([0.; 4].into());
        assert!(plan(&value).is_ok());
        assert!(
            !value
                .diagnostics()
                .iter()
                .any(|issue| issue.feature == UnsupportedFeature::ClippedImage)
        );
        value.clips[0].local_bounds.size.width = 0.;
        assert_eq!(plan(&value).unwrap(), RectClip::Empty);
    }
}
