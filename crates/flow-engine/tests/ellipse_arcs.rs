use gpui_flow_engine::{DirtyFlags, PropertyKey, SceneEngine};
use gpui_flow_scene::{
    Affine, DisplayObject, HitTestOptions, Point, Scene, Shape,
    display::{Ellipse, Group},
};
use serde_json::json;
fn engine() -> SceneEngine {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("g", Group::new()).with_transform(Affine::translation(50., 20.)),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("e", Ellipse::circle(20.)),
        Some("g".into()),
        None,
    )
    .unwrap();
    SceneEngine::new(s).unwrap()
}
#[test]
fn ellipse_sector_updates_real_bounds_ancestor_bounds_damage_and_hit() {
    let mut e = engine();
    let h = e.handle(&"e".into()).unwrap();
    let old = e.world_bounds(&"e".into()).unwrap().unwrap();
    assert!(
        e.hit_test(Point::new(60., 30.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    let changes = e
        .set_property(
            h,
            PropertyKey::Shape,
            json!(Shape::from(Ellipse::circle(20.).sector(0., 90.))),
        )
        .unwrap();
    let new = e.world_bounds(&"e".into()).unwrap().unwrap();
    assert!(new.min_x() >= 69.99 && new.min_y() >= 39.99);
    assert!(new.size.width < 20.1 && new.size.height < 20.1);
    assert_eq!(e.world_bounds(&"g".into()).unwrap(), Some(new));
    let damage = changes.damage_bounds().unwrap();
    assert_eq!(damage.union(old), damage);
    assert_eq!(damage.union(new), damage);
    assert!(
        changes.dirty[&"e".into()]
            .contains(DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT)
    );
    assert!(
        e.hit_test(Point::new(60., 30.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        e.hit_test(Point::new(80., 50.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id,
        "e".into()
    );
}
#[test]
fn adding_a_hole_refreshes_hit_even_with_the_same_outer_box() {
    let mut e = engine();
    let h = e.handle(&"e".into()).unwrap();
    let point = Point::new(70., 40.);
    assert!(
        e.hit_test(point, HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(Ellipse::circle(20.).ring(0.5))),
    )
    .unwrap();
    assert!(
        e.hit_test(point, HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert!(
        e.hit_test(Point::new(85., 40.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
}
#[test]
fn invalid_ellipse_update_is_atomic_and_new_fields_survive_scene_json() {
    let mut e = engine();
    let h = e.handle(&"e".into()).unwrap();
    let before = e.scene().clone();
    let layout = e.layout().clone();
    for ellipse in [
        Ellipse::circle(20.).ring(1.1),
        Ellipse::circle(20.).arc(0., 400.),
    ] {
        assert!(
            e.set_property(h, PropertyKey::Shape, json!(Shape::from(ellipse)))
                .is_err()
        );
    }
    assert_eq!(e.scene(), &before);
    assert_eq!(e.layout(), &layout);
    assert_eq!(e.revision(), 0);
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(Ellipse::circle(20.).sector(-90., 0.).ring(0.3))),
    )
    .unwrap();
    let restored: Scene = serde_json::from_value(json!(e.scene())).unwrap();
    assert_eq!(&restored, e.scene());
    assert!(SceneEngine::new(restored).is_ok());
}
#[test]
fn zero_span_clears_geometry_without_destroying_identity() {
    let mut e = engine();
    let h = e.handle(&"e".into()).unwrap();
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(Ellipse::circle(20.).sector(45., 45.))),
    )
    .unwrap();
    assert_eq!(e.world_bounds(&"e".into()).unwrap(), None);
    assert_eq!(e.handle(&"e".into()), Some(h));
    assert!(
        e.hit_test(Point::new(70., 40.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
}
