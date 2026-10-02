use gpui_flow_engine::{PropertyKey, SceneEngine};
use gpui_flow_scene::{
    Affine, Bounds, ClipRegion, DisplayObject, HitPolicy, HitTestOptions, Point, Scene, Shape,
    Size, Style,
    display::{self, CornerRadii},
};
use serde_json::json;
fn container(frame: bool, radii: [f32; 4], clip: bool) -> Shape {
    if frame {
        let mut f = display::Frame::new(60., 40.).with_corners(radii);
        f.clip_children = clip;
        f.into()
    } else {
        display::Box::new(60., 40.)
            .with_corners(radii)
            .clipped(clip)
            .into()
    }
}
fn engine(frame: bool, clip: bool, transform: Affine) -> SceneEngine {
    let mut s = Scene::new();
    let mut parent = DisplayObject::new("parent", container(frame, [15., 0., 10., 0.], clip))
        .with_transform(transform)
        .with_style(Style {
            fill: None,
            ..Default::default()
        });
    parent.hit_policy = HitPolicy::None;
    s.insert(parent, None, None).unwrap();
    s.insert(
        DisplayObject::new("child", display::Rect::new(60., 40.)),
        Some("parent".into()),
        None,
    )
    .unwrap();
    SceneEngine::new(s).unwrap()
}
fn hit(e: &SceneEngine, point: Point) -> bool {
    e.hit_test(point, HitTestOptions::default())
        .unwrap()
        .is_some()
}
#[test]
fn box_and_frame_clip_real_independent_corners_with_cached_and_scene_hit_parity() {
    for frame in [false, true] {
        let e = engine(frame, true, Affine::IDENTITY);
        for (point, expected) in [
            (Point::new(1., 1.), false),
            (Point::new(59., 1.), true),
            (Point::new(59., 39.), false),
            (Point::new(1., 39.), true),
            (Point::new(30., 20.), true),
        ] {
            assert_eq!(hit(&e, point), expected);
            assert_eq!(
                e.scene()
                    .hit_test(point, HitTestOptions::default())
                    .unwrap()
                    .is_some(),
                expected
            );
        }
        assert_eq!(
            e.layout().get(&"child".into()).unwrap().clips[0].corner_radii,
            Some(CornerRadii([15., 0., 10., 0.]))
        );
    }
}
#[test]
fn affine_parent_clip_uses_local_corners_once() {
    let matrix = Affine::translation(100., 50.) * Affine::rotation(30.) * Affine::scale(2., 0.75);
    let e = engine(true, true, matrix);
    assert!(!hit(&e, matrix.transform_point(Point::new(1., 1.))));
    assert!(hit(&e, matrix.transform_point(Point::new(59., 1.))));
}
#[test]
fn clip_corner_edit_invalidates_descendants_when_aabb_is_unchanged() {
    let mut e = engine(true, true, Affine::IDENTITY);
    let h = e.handle(&"parent".into()).unwrap();
    let bounds = e.world_bounds(&"parent".into()).unwrap();
    let change = e
        .set_property(
            h,
            PropertyKey::Shape,
            json!(container(true, [0., 15., 0., 10.], true)),
        )
        .unwrap();
    assert_eq!(e.world_bounds(&"parent".into()).unwrap(), bounds);
    assert!(change.layout.world_recomputed > 0);
    assert!(hit(&e, Point::new(1., 1.)));
    assert!(!hit(&e, Point::new(59., 1.)));
    assert_eq!(
        e.layout().get(&"child".into()).unwrap().clips[0].corner_radii,
        Some(CornerRadii([0., 15., 0., 10.]))
    );
}
#[test]
fn clipping_toggle_and_nested_intersections_do_not_replace_the_geometry() {
    let mut e = engine(false, false, Affine::IDENTITY);
    assert!(hit(&e, Point::new(1., 1.)));
    let h = e.handle(&"parent".into()).unwrap();
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(container(false, [15., 0., 10., 0.], true)),
    )
    .unwrap();
    assert!(!hit(&e, Point::new(1., 1.)));
    e.transaction(|s| {
        let mut inner = DisplayObject::new(
            "inner",
            display::Frame::new(60., 40.).with_corners([0., 15., 0., 0.]),
        );
        inner.hit_policy = HitPolicy::None;
        inner.style.fill = None;
        s.insert(inner, Some("parent".into()), None)?;
        s.reparent(&"child".into(), Some("inner".into()), None, false)
    })
    .unwrap();
    assert_eq!(e.layout().get(&"child".into()).unwrap().clips.len(), 2);
    assert!(!hit(&e, Point::new(1., 1.)));
    assert!(!hit(&e, Point::new(59., 1.)));
    assert!(hit(&e, Point::new(30., 20.)));
}
#[test]
fn clip_region_preserves_nonzero_origins_and_zero_array_override() {
    let clip = ClipRegion {
        local_bounds: Bounds::new(Point::new(10., 20.), Size::new(60., 40.)),
        corner_radius: 7.,
        corner_radii: Some(CornerRadii([15., 0., 10., 0.])),
        world_transform: Affine::translation(100., 50.) * Affine::rotation(30.),
    };
    assert!(!clip.contains(clip.world_transform.transform_point(Point::new(11., 21.))));
    assert!(clip.contains(clip.world_transform.transform_point(Point::new(69., 21.))));
    assert!(!clip.is_rectangular());
    let zero = ClipRegion {
        corner_radii: Some(CornerRadii::uniform(0.)),
        ..clip
    };
    assert!(zero.is_rectangular());
    assert!(zero.contains(zero.world_transform.transform_point(Point::new(11., 21.))));
}
#[test]
fn factory_defaults_json_and_invalid_container_corner_updates_are_safe() {
    for (kind, default_clip) in [("box", false), ("frame", true)] {
        let mut e = SceneEngine::new(Scene::new()).unwrap();
        let (h, _) = e.create(kind, "parent", json!({}), None, None).unwrap();
        assert_eq!(
            e.object(h).unwrap().shape.child_clip().is_some(),
            default_clip
        );
        let (new, _) = e
            .create(
                kind,
                "corners",
                json!({"corner_radii":[15.,0.,10.,0.],"clip_children":true}),
                None,
                None,
            )
            .unwrap();
        assert_eq!(
            e.object(new).unwrap().shape.child_clip().unwrap().2,
            Some(CornerRadii([15., 0., 10., 0.]))
        );
        let before = e.scene().clone();
        let cache = e.layout().clone();
        assert!(
            e.set_property(
                h,
                PropertyKey::Shape,
                json!(container(kind == "frame", [-1., 0., 0., 0.], true))
            )
            .is_err()
        );
        assert_eq!(e.scene(), &before);
        assert_eq!(e.layout(), &cache);
        assert_eq!(
            serde_json::from_value::<Scene>(json!(e.scene())).unwrap(),
            *e.scene()
        );
    }
}
