use gpui_flow_engine::{DirtyFlags, PropertyKey, PropertyType, SceneEngine};
use gpui_flow_scene::{display, *};
use serde_json::json;

fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("g", display::Frame::new(80., 80.)).with_style(Style {
                grayscale: 0.5,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("mid", display::Group::new()),
            Some("g".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(20., 20.)),
            Some("mid".into()),
            None,
        )
        .unwrap();
    SceneEngine::new(scene).unwrap()
}

#[test]
fn grayscale_schema_paint_only_delta_noop_and_removal_preserve_bounds_and_hits() {
    let schema = PropertyKey::Grayscale.descriptor().unwrap();
    assert_eq!(schema.name, "style.grayscale");
    assert_eq!(schema.default, Some(json!(0.)));
    assert_eq!(
        schema.value_type,
        PropertyType::Number {
            min: Some(0.),
            max: Some(1.)
        }
    );
    assert_eq!(schema.dirty, DirtyFlags::PAINT);
    let mut e = engine();
    let handle = e.handle(&"r".into()).unwrap();
    let geometry = e.world_bounds(&"r".into()).unwrap();
    let effect = e.effect_bounds(&"g".into()).unwrap();
    let hit = e.hit_test(Point::new(5., 5.), Default::default()).unwrap();
    for amount in [0.25, 1., 0.] {
        let change = e
            .set_property(handle, PropertyKey::Grayscale, json!(amount))
            .unwrap();
        assert_eq!(change.properties.len(), 1);
        assert_eq!(change.properties[0].property, "style.grayscale");
        assert_eq!(change.dirty[&"r".into()], DirtyFlags::PAINT);
        assert_eq!(change.layout.bounds_recomputed, 0);
        assert_eq!(e.world_bounds(&"r".into()).unwrap(), geometry);
        assert_eq!(e.effect_bounds(&"g".into()).unwrap(), effect);
        assert_eq!(
            e.hit_test(Point::new(5., 5.), Default::default()).unwrap(),
            hit
        );
        // The fixed-size filtered ancestor's full paint extent is invalidated,
        // while geometric bounds and layout caches remain unchanged.
        assert!(change.damage_bounds().unwrap().max_x() >= 80.);
        let revision = e.revision();
        let same = e
            .set_property(handle, PropertyKey::Grayscale, json!(amount))
            .unwrap();
        assert!(same.properties.is_empty());
        assert_eq!(e.revision(), revision);
    }
    let revision = e.revision();
    assert!(
        e.set_property(handle, PropertyKey::Grayscale, json!(-0.))
            .unwrap()
            .properties
            .is_empty()
    );
    assert_eq!(e.revision(), revision);
}

#[test]
fn grayscale_invalid_engine_edits_are_atomic() {
    let mut e = engine();
    let before = e.scene().clone();
    let revision = e.revision();
    let handle = e.handle(&"r".into()).unwrap();
    for value in [
        json!(-1),
        json!(1.01),
        json!(null),
        json!(true),
        json!("0.5"),
        json!({"grayscale": 0.5}),
    ] {
        assert!(
            e.set_property(handle, PropertyKey::Grayscale, value)
                .is_err()
        );
        assert_eq!(e.scene(), &before);
        assert_eq!(e.revision(), revision);
    }
}
