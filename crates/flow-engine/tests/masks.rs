use gpui_flow_engine::{PropertyKey, SceneEngine};
use gpui_flow_scene::{Affine, DisplayObject, HitTestOptions, MaskMode, Point, Scene, display};
use serde_json::json;
fn engine() -> SceneEngine {
    let mut s = Scene::new();
    s.insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    s.insert(
        DisplayObject::new("mask", display::Rect::new(40., 30.)).with_mask(MaskMode::Alpha),
        Some("g".into()),
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("ink", display::Rect::new(40., 30.))
            .with_transform(Affine::translation(20., 0.)),
        Some("g".into()),
        None,
    )
    .unwrap();
    SceneEngine::new(s).unwrap()
}
fn consistent(e: &SceneEngine) {
    let fresh = SceneEngine::new(e.scene().clone()).unwrap();
    for node in e.scene().nodes() {
        let id = &node.object.id;
        assert_eq!(
            e.local_bounds(id).unwrap(),
            e.scene().local_bounds(id).unwrap()
        );
        assert_eq!(e.world_bounds(id).unwrap(), fresh.world_bounds(id).unwrap());
        assert_eq!(
            e.effect_bounds(id).unwrap(),
            fresh.effect_bounds(id).unwrap()
        );
    }
    for x in [10., 30., 50.] {
        let p = Point::new(x, 10.);
        assert_eq!(
            e.hit_test_all(p, HitTestOptions::default()).unwrap(),
            e.scene()
                .hit_test_all(p, HitTestOptions::default())
                .unwrap()
        );
    }
}
#[test]
fn mask_visibility_opacity_order_and_hit_children_invalidate_cached_policy() {
    let mut e = engine();
    consistent(&e);
    let m = e.handle(&"mask".into()).unwrap();
    let g = e.handle(&"g".into()).unwrap();
    for (key, value, width) in [
        (PropertyKey::Visible, json!(false), 60.),
        (PropertyKey::Visible, json!(true), 40.),
        (PropertyKey::Opacity, json!(0.), 60.),
        (PropertyKey::Opacity, json!(1.), 40.),
        (PropertyKey::ZIndex, json!(1), 60.),
        (PropertyKey::ZIndex, json!(0), 40.),
    ] {
        e.set_property(m, key, value).unwrap();
        assert_eq!(e.local_bounds(&"g".into()).unwrap().unwrap().max_x(), width);
        consistent(&e);
    }
    e.set_property(g, PropertyKey::HitChildren, json!(false))
        .unwrap();
    consistent(&e);
    assert_eq!(
        e.hit_test(Point::new(30., 10.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "g"
    );
    e.set_property(m, PropertyKey::Mask, json!("clipping"))
        .unwrap();
    consistent(&e);
    assert_eq!(
        e.hit_test(Point::new(10., 10.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "g"
    );
}
#[test]
fn ordinary_content_outside_first_mask_remains_in_render_extent_and_hit_queries() {
    let e = engine();
    assert_eq!(e.local_bounds(&"g".into()).unwrap().unwrap().max_x(), 40.);
    assert!(e.effect_bounds(&"g".into()).unwrap().unwrap().max_x() >= 60.);
    assert_eq!(
        e.hit_test(Point::new(50., 10.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "ink"
    );
    consistent(&e);
}
