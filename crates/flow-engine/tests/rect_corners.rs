use gpui_flow_engine::{DirtyFlags, PropertyKey, SceneEngine};
use gpui_flow_scene::{DisplayObject, HitTestOptions, Point, Scene, Shape, display::Rect};
use serde_json::json;
fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("r", Rect::new(60., 40.)), None, None)
        .unwrap();
    SceneEngine::new(scene).unwrap()
}
#[test]
fn corner_edits_refresh_hit_and_paint_without_changing_nominal_bounds() {
    let mut e = engine();
    let h = e.handle(&"r".into()).unwrap();
    let bounds = e.world_bounds(&"r".into()).unwrap();
    assert!(
        e.hit_test(Point::new(1., 1.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    let changes = e
        .set_property(
            h,
            PropertyKey::Shape,
            json!(Shape::from(
                Rect::new(60., 40.).with_corners([15., 0., 10., 0.])
            )),
        )
        .unwrap();
    assert_eq!(e.world_bounds(&"r".into()).unwrap(), bounds);
    assert!(
        changes.dirty[&"r".into()]
            .contains(DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT)
    );
    assert!(changes.damage_bounds().is_some());
    assert!(
        e.hit_test(Point::new(1., 1.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert!(
        e.hit_test(Point::new(59., 1.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    assert_eq!(e.handle(&"r".into()), Some(h));
}
#[test]
fn invalid_corners_roll_back_scene_revision_and_layout() {
    let mut e = engine();
    let h = e.handle(&"r".into()).unwrap();
    let before = e.scene().clone();
    let cache = e.layout().clone();
    assert!(
        e.set_property(
            h,
            PropertyKey::Shape,
            json!(Shape::from(
                Rect::new(60., 40.).with_corners([-1., 2., 3., 4.])
            ))
        )
        .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.layout(), &cache);
    assert_eq!(e.revision(), 0);
}
#[test]
fn registry_and_scene_json_preserve_typed_corner_arrays() {
    let mut e = engine();
    let (h, _) = e
        .create(
            "rect",
            "new",
            json!({"size":{"width":60.,"height":40.},"corner_radii":[1.,2.,3.,4.]}),
            None,
            None,
        )
        .unwrap();
    let Shape::Rect(rect) = &e.object(h).unwrap().shape else {
        panic!()
    };
    assert_eq!(rect.corner_radii.unwrap().0, [1., 2., 3., 4.]);
    let restored: Scene = serde_json::from_value(json!(e.scene())).unwrap();
    assert_eq!(&restored, e.scene());
    assert!(SceneEngine::new(restored).is_ok());
}

#[test]
fn optional_corner_factory_default_and_invalid_array_are_validated() {
    let mut e = engine();
    let (legacy, _) = e.create("rect", "legacy", json!({}), None, None).unwrap();
    assert_eq!(
        &e.object(legacy).unwrap().shape,
        &Shape::from(Rect::new(100., 100.))
    );
    let before = e.scene().clone();
    let revision = e.revision();
    for value in [
        json!([-1., 0., 0., 0.]),
        json!([1., 2., 3.]),
        json!("round"),
    ] {
        assert!(
            e.create("rect", "invalid", json!({"corner_radii":value}), None, None)
                .is_err()
        );
    }
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
}
