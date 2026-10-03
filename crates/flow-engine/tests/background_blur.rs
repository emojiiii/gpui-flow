use gpui_flow_engine::{DirtyFlags, PropertyKey, PropertyType, SceneEngine};
use gpui_flow_scene::{display, *};
use serde_json::json;

fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("prefix", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("glass", display::Frame::new(20., 20.))
                .with_transform(Affine::translation(40., 0.))
                .with_style(Style {
                    background_blur: 2.,
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
    SceneEngine::new(scene).unwrap()
}

#[test]
fn background_blur_schema_is_paint_only_without_layout_or_hit_changes() {
    let descriptor = PropertyKey::BackgroundBlur.descriptor().unwrap();
    assert_eq!(descriptor.name, "style.background_blur");
    assert_eq!(descriptor.default, Some(json!(0.)));
    assert_eq!(
        descriptor.value_type,
        PropertyType::Number {
            min: Some(0.),
            max: Some(256.)
        }
    );
    assert_eq!(descriptor.dirty, DirtyFlags::PAINT);
    let mut engine = engine();
    let handle = engine.handle(&"glass".into()).unwrap();
    let identity = engine.instance(handle).unwrap();
    let geometry = engine.world_bounds(&"glass".into()).unwrap();
    let effects = engine.effect_bounds(&"glass".into()).unwrap();
    let hit = engine
        .hit_test(Point::new(45., 5.), Default::default())
        .unwrap();
    assert_eq!(
        geometry,
        Some(Bounds::new(Point::new(40., 0.), Size::new(20., 20.)))
    );
    assert_eq!(effects, geometry);

    for amount in [0.25, 256., 0.] {
        let change = engine
            .set_property(handle, PropertyKey::BackgroundBlur, json!(amount))
            .unwrap();
        assert_eq!(change.properties.len(), 1);
        assert_eq!(change.properties[0].property, "style.background_blur");
        assert_eq!(change.dirty[&"glass".into()], DirtyFlags::PAINT);
        assert_eq!(change.layout.bounds_recomputed, 0);
        assert_eq!(change.layout.world_recomputed, 0);
        assert!(change.full_paint_invalidation);
        assert_eq!(engine.world_bounds(&"glass".into()).unwrap(), geometry);
        assert_eq!(engine.effect_bounds(&"glass".into()).unwrap(), effects);
        assert_eq!(engine.instance(handle).unwrap(), identity);
        assert_eq!(
            engine
                .hit_test(Point::new(45., 5.), Default::default())
                .unwrap(),
            hit
        );

        let revision = engine.revision();
        let noop = engine
            .set_property(handle, PropertyKey::BackgroundBlur, json!(amount))
            .unwrap();
        assert!(noop.properties.is_empty());
        assert!(noop.dirty.is_empty());
        assert!(!noop.full_paint_invalidation);
        assert_eq!(engine.revision(), revision);
    }
    let revision = engine.revision();
    let negative_zero = engine
        .set_property(handle, PropertyKey::BackgroundBlur, json!(-0.))
        .unwrap();
    assert!(!negative_zero.changed());
    assert!(!negative_zero.full_paint_invalidation);
    assert_eq!(engine.revision(), revision);
}

#[test]
fn earlier_root_paint_opacity_order_transform_and_visibility_invalidate_glass() {
    let mut engine = engine();
    let prefix = engine.handle(&"prefix".into()).unwrap();
    let glass_extent = engine.effect_bounds(&"glass".into()).unwrap();
    for (property, value) in [
        (
            PropertyKey::Fill,
            serde_json::to_value(Paint::solid(Color::WHITE)).unwrap(),
        ),
        (PropertyKey::Opacity, json!(0.5)),
        (PropertyKey::ZIndex, json!(1)),
        (
            PropertyKey::Transform,
            serde_json::to_value(Affine::translation(1., 2.)).unwrap(),
        ),
        (PropertyKey::Visible, json!(false)),
    ] {
        let change = engine.set_property(prefix, property, value).unwrap();
        assert!(change.full_paint_invalidation);
        assert_eq!(engine.effect_bounds(&"glass".into()).unwrap(), glass_extent);
    }
}

#[test]
fn removing_a_prefix_root_requests_full_paint_with_unchanged_glass_extent() {
    let mut engine = engine();
    let extent = engine.effect_bounds(&"glass".into()).unwrap();
    let change = engine
        .transaction(|scene| {
            scene.remove(&"prefix".into())?;
            Ok(())
        })
        .unwrap();
    assert!(change.full_paint_invalidation);
    assert_eq!(engine.effect_bounds(&"glass".into()).unwrap(), extent);
    assert_eq!(engine.scene().roots(), &[DisplayId::from("glass")]);
}

#[test]
fn removing_the_last_background_blur_still_invalidates_the_old_domain() {
    let mut engine = engine();
    let extent = engine.effect_bounds(&"glass".into()).unwrap().unwrap();
    let change = engine
        .transaction(|scene| {
            scene.remove(&"glass".into())?;
            Ok(())
        })
        .unwrap();
    assert!(change.full_paint_invalidation);
    assert!(change.damage.contains(&extent));
    let prefix = engine.handle(&"prefix".into()).unwrap();
    let subsequent = engine
        .set_property(prefix, PropertyKey::Opacity, json!(0.5))
        .unwrap();
    assert!(!subsequent.full_paint_invalidation);
}

#[test]
fn disabling_the_last_background_blur_invalidates_once() {
    let mut engine = engine();
    let glass = engine.handle(&"glass".into()).unwrap();
    let change = engine
        .set_property(glass, PropertyKey::BackgroundBlur, json!(0.))
        .unwrap();
    assert!(change.full_paint_invalidation);
    let prefix = engine.handle(&"prefix".into()).unwrap();
    let subsequent = engine
        .set_property(prefix, PropertyKey::Opacity, json!(0.5))
        .unwrap();
    assert!(!subsequent.full_paint_invalidation);
}

#[test]
fn metadata_and_noop_edits_do_not_invalidate_a_background_domain() {
    let mut engine = engine();
    for id in ["prefix", "glass"] {
        let handle = engine.handle(&id.into()).unwrap();
        let change = engine
            .set_property(handle, PropertyKey::Name, json!("named"))
            .unwrap();
        assert!(change.changed());
        assert_eq!(change.dirty[&id.into()], DirtyFlags::DATA);
        assert!(!change.full_paint_invalidation);
        let revision = engine.revision();
        for (key, value) in [
            (PropertyKey::Name, json!("named")),
            (PropertyKey::Opacity, json!(1.)),
            (
                PropertyKey::BackgroundBlur,
                json!(if id == "glass" { 2. } else { 0. }),
            ),
        ] {
            let noop = engine.set_property(handle, key, value).unwrap();
            assert!(!noop.changed());
            assert!(!noop.full_paint_invalidation);
            assert_eq!(engine.revision(), revision);
        }
    }
}

#[test]
fn invalid_background_blur_properties_and_transactions_are_atomic_when_dormant() {
    for (visible, opacity) in [(false, 1.), (true, 0.), (false, 0.)] {
        let mut engine = engine();
        let glass = engine.handle(&"glass".into()).unwrap();
        engine
            .set_property(glass, PropertyKey::Visible, json!(visible))
            .unwrap();
        engine
            .set_property(glass, PropertyKey::Opacity, json!(opacity))
            .unwrap();
        let before = engine.scene().clone();
        let revision = engine.revision();
        let identity = engine.instance(glass).unwrap();
        for value in [
            json!(-1),
            json!(256.01),
            json!(null),
            json!(true),
            json!("2"),
            json!({"blur": 2}),
            json!([]),
        ] {
            assert!(
                engine
                    .set_property(glass, PropertyKey::BackgroundBlur, value)
                    .is_err()
            );
            assert_eq!(engine.scene(), &before);
            assert_eq!(engine.revision(), revision);
            assert_eq!(engine.instance(glass).unwrap(), identity);
        }
        for amount in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            let result = engine.transaction(|scene| {
                scene.update_object(&"prefix".into(), |object| {
                    object.style.fill = Some(Color::WHITE.into());
                })?;
                scene.update_object(&"glass".into(), |object| {
                    object.style.background_blur = amount;
                })
            });
            assert!(result.is_err());
            assert_eq!(engine.scene(), &before);
            assert_eq!(engine.revision(), revision);
            assert_eq!(engine.instance(glass).unwrap(), identity);
        }
    }
}
