use gpui_flow_engine::{DirtyFlags, PropertyKey, SceneEngine};
use gpui_flow_scene::{display, *};

#[test]
fn inner_shadow_property_is_paint_only_atomic_and_damages_outer_ancestor() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("g", display::Frame::new(40., 40.)).with_style(Style {
                shadow: Some(Shadow::new(30., 0., 0., Color::BLACK)),
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(20., 20.)),
            Some("g".into()),
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let handle = engine.handle(&"r".into()).unwrap();
    let geometry = engine.world_bounds(&"r".into()).unwrap();
    let effect = engine.effect_bounds(&"g".into()).unwrap();
    let change = engine
        .set_property(
            handle,
            PropertyKey::InnerShadowLayers,
            serde_json::to_value([Shadow::new(3., 0., 2., Color::BLACK)]).unwrap(),
        )
        .unwrap();
    assert_eq!(change.properties[0].property, "style.inner_shadow_layers");
    assert_eq!(change.dirty[&"r".into()], DirtyFlags::PAINT);
    assert_eq!(change.layout.bounds_recomputed, 0);
    assert_eq!(engine.world_bounds(&"r".into()).unwrap(), geometry);
    assert_eq!(engine.effect_bounds(&"g".into()).unwrap(), effect);
    assert!(change.damage_bounds().unwrap().max_x() >= 70.);
    assert!(
        engine
            .hit_test(Point::new(60., 10.), Default::default())
            .unwrap()
            .is_none()
    );
    let before = engine.scene().clone();
    let revision = engine.revision();
    assert!(
        engine
            .set_property(
                handle,
                PropertyKey::InnerShadowLayers,
                serde_json::to_value(vec![
                    Shadow::new(0., 0., 0., Color::TRANSPARENT);
                    MAX_INNER_SHADOW_LAYERS + 1
                ])
                .unwrap()
            )
            .is_err()
    );
    assert_eq!(engine.scene(), &before);
    assert_eq!(engine.revision(), revision);
    let removed = engine
        .set_property(
            handle,
            PropertyKey::InnerShadowLayers,
            serde_json::json!([]),
        )
        .unwrap();
    assert_eq!(removed.properties[0].property, "style.inner_shadow_layers");
    assert_eq!(
        PropertyKey::InnerShadowLayers.descriptor().unwrap().default,
        Some(serde_json::json!([]))
    );
}
