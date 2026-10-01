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

#[test]
fn parent_effect_bounds_expand_after_descendant_effects_and_clip_before_shadow() {
    for clip in [false, true] {
        let mut s = Scene::new();
        let shape: Shape = if clip {
            display::Frame::new(25., 20.).into()
        } else {
            display::Group::new().into()
        };
        s.insert(
            DisplayObject::new("g", shape).with_style(Style {
                shadow: Some(Shadow::new(40., 0., 0., Color::BLACK)),
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new("r", display::Rect::new(10., 10.)).with_style(Style {
                shadow: Some(Shadow::new(20., 0., 0., Color::BLACK)),
                ..Default::default()
            }),
            Some("g".into()),
            None,
        )
        .unwrap();
        let mut e = SceneEngine::new(s).unwrap();
        let geometry = e.world_bounds(&"g".into()).unwrap();
        assert_eq!(
            e.effect_bounds(&"g".into()).unwrap().unwrap().max_x(),
            if clip { 65. } else { 70. }
        );
        let change = e
            .set_property(
                e.handle(&"r".into()).unwrap(),
                PropertyKey::Shadow,
                serde_json::to_value(Shadow::new(30., 0., 0., Color::BLACK)).unwrap(),
            )
            .unwrap();
        assert_eq!(
            e.effect_bounds(&"g".into()).unwrap().unwrap().max_x(),
            if clip { 65. } else { 80. }
        );
        assert_eq!(e.world_bounds(&"g".into()).unwrap(), geometry);
        assert_eq!(change.layout.bounds_recomputed, 0);
        assert!(change.damage_bounds().unwrap().max_x() >= if clip { 65. } else { 80. });
        assert!(
            e.hit_test(Point::new(55., 5.), Default::default())
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn paint_only_descendant_changes_damage_shadow_through_non_effect_groups() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("root", display::Frame::new(50., 50.)).with_style(Style {
            shadow: Some(Shadow::new(60., 0., 0., Color::BLACK)),
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("middle", display::Group::new()),
        Some("root".into()),
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("ink", display::Rect::new(10., 10.)),
        Some("middle".into()),
        None,
    )
    .unwrap();
    let mut e = SceneEngine::new(s).unwrap();
    let before = e.effect_bounds(&"root".into()).unwrap();
    let change = e
        .set_property(
            e.handle(&"ink".into()).unwrap(),
            PropertyKey::Fill,
            serde_json::to_value(Paint::solid(Color::TRANSPARENT)).unwrap(),
        )
        .unwrap();
    assert_eq!(e.effect_bounds(&"root".into()).unwrap(), before);
    assert_eq!(change.layout.bounds_recomputed, 0);
    assert!(change.damage_bounds().unwrap().max_x() >= 110.);
}
