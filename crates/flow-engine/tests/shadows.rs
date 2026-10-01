use gpui_flow_engine::{DirtyFlags, PropertyKey, SceneEngine};
use gpui_flow_scene::{display, *};

#[test]
fn shadow_property_updates_effect_damage_without_geometry_or_hit_dirty() {
    let mut s = Scene::new();
    s.insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    s.insert(
        DisplayObject::new("r", display::Rect::new(10., 10.)),
        Some("g".into()),
        None,
    )
    .unwrap();
    let mut e = SceneEngine::new(s).unwrap();
    let h = e.handle(&"r".into()).unwrap();
    let geometric = e.world_bounds(&"g".into()).unwrap();
    let shadow = Shadow::new(30., 0., 4., Color::BLACK);
    let c = e
        .set_property(
            h,
            PropertyKey::Shadow,
            serde_json::to_value(shadow).unwrap(),
        )
        .unwrap();
    assert_eq!(c.properties[0].property, "style.shadow");
    assert_eq!(c.dirty[&"r".into()], DirtyFlags::PAINT);
    assert_eq!(c.layout.bounds_recomputed, 0);
    assert_eq!(e.world_bounds(&"g".into()).unwrap(), geometric);
    assert!(e.effect_bounds(&"g".into()).unwrap().unwrap().max_x() >= 48.);
    assert!(c.damage_bounds().unwrap().max_x() >= 48.);
    assert!(
        e.hit_test(Point::new(35., 5.), Default::default())
            .unwrap()
            .is_none()
    );
    let c = e
        .set_property(h, PropertyKey::Shadow, serde_json::Value::Null)
        .unwrap();
    assert!(c.damage_bounds().unwrap().max_x() >= 48.);
    assert_eq!(e.effect_bounds(&"g".into()).unwrap(), geometric);
    assert_eq!(
        PropertyKey::Shadow.descriptor().unwrap().default,
        Some(serde_json::Value::Null)
    );
}

#[test]
fn rejected_shadow_property_preserves_document_and_revision() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(10., 10.)),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEngine::new(s).unwrap();
    let before = e.scene().clone();
    let revision = e.revision();
    for value in [
        serde_json::json!({"x":0,"y":0,"blur":-1,"color":{"r":0,"g":0,"b":0,"a":255}}),
        serde_json::json!({"x":0,"y":0,"blur":1,"color":{"r":0,"g":0,"b":0,"a":255},"spread":1}),
    ] {
        assert!(
            e.set_property(e.handle(&"r".into()).unwrap(), PropertyKey::Shadow, value)
                .is_err()
        );
        assert_eq!(e.scene(), &before);
        assert_eq!(e.revision(), revision);
    }
}
