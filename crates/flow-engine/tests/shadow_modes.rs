use gpui_flow_engine::{DirtyFlags, PropertyKey, SceneEngine};
use gpui_flow_scene::{display, *};

fn shadow(spread: f32) -> Shadow {
    Shadow {
        spread,
        ..Shadow::new(10., 0., 0., Color::BLACK)
    }
}

#[test]
fn shadow_modes_edit_every_slot_as_atomic_paint_only_properties() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(100., 40.)),
            None,
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let handle = engine.handle(&"r".into()).unwrap();
    let geometry = engine.world_bounds(&"r".into()).unwrap();
    for key in [
        PropertyKey::Shadow,
        PropertyKey::ShadowLayers,
        PropertyKey::InnerShadowLayers,
    ] {
        let effect = Shadow {
            r#box: true,
            scale_fixed: true,
            ..shadow(-30.)
        };
        let value = if key == PropertyKey::Shadow {
            serde_json::to_value(effect).unwrap()
        } else {
            serde_json::to_value([effect]).unwrap()
        };
        let change = engine.set_property(handle, key.clone(), value).unwrap();
        assert_eq!(change.dirty[&"r".into()], DirtyFlags::PAINT);
        assert_eq!(change.layout.bounds_recomputed, 0);
        assert_eq!(engine.world_bounds(&"r".into()).unwrap(), geometry);
        assert!(
            change
                .damage_bounds()
                .unwrap()
                .contains_rect(geometry.unwrap())
        );
        assert_eq!(
            Scene::from_json(&engine.scene().to_json().unwrap()).unwrap(),
            *engine.scene()
        );
        let before = engine.scene().clone();
        let revision = engine.revision();
        let bad = serde_json::to_value(Shadow {
            spread: 4097.,
            color: Color::TRANSPARENT,
            ..effect
        })
        .unwrap();
        let bad = if key == PropertyKey::Shadow {
            bad
        } else {
            serde_json::json!([effect, bad])
        };
        assert!(engine.set_property(handle, key, bad).is_err());
        assert_eq!(engine.scene(), &before);
        assert_eq!(engine.revision(), revision);
    }
}

#[test]
fn fixed_box_spread_bounds_use_own_reference_and_complete_overflow_source() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("b", display::Box::new(100., 40.)).with_style(Style {
                shadow: Some(shadow(10.)),
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(10., 10.))
                .with_transform(Affine::translation(200., 0.))
                .with_style(Style {
                    shadow: Some(Shadow::new(60., 0., 0., Color::BLACK)),
                    ..Default::default()
                }),
            Some("b".into()),
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let reference = Bounds::new(Point::ZERO, Size::new(100., 40.));
    let source = Bounds::new(Point::ZERO, Size::new(270., 40.));
    let expected =
        shadow(10.).expand_bounds_with_reference(source, Some(reference), Affine::IDENTITY);
    assert_eq!(engine.effect_bounds(&"b".into()).unwrap(), Some(expected));
    let geometry = engine.world_bounds(&"b".into()).unwrap();
    let change = engine
        .set_property(
            engine.handle(&"r".into()).unwrap(),
            PropertyKey::Shadow,
            serde_json::to_value(Shadow::new(100., 0., 0., Color::BLACK)).unwrap(),
        )
        .unwrap();
    assert_eq!(change.layout.bounds_recomputed, 0);
    assert_eq!(engine.world_bounds(&"b".into()).unwrap(), geometry);
    assert!(engine.effect_bounds(&"b".into()).unwrap().unwrap().max_x() > expected.max_x());
    assert!(change.damage_bounds().unwrap().contains_rect(expected));
}

#[test]
fn group_spread_damage_follows_descendant_effects_without_moving_geometric_reference() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("g", display::Group::new())
                .with_transform(Affine::rotation(90.))
                .with_style(Style {
                    shadow: Some(Shadow {
                        scale_fixed: true,
                        ..shadow(10.)
                    }),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(100., 40.)).with_style(Style {
                shadow: Some(Shadow::new(60., 0., 0., Color::BLACK)),
                ..Default::default()
            }),
            Some("g".into()),
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let geometric = engine.world_bounds(&"g".into()).unwrap();
    let before = engine.effect_bounds(&"g".into()).unwrap().unwrap();
    let reference = engine.scene().shadow_reference_bounds(&"g".into()).unwrap();
    let change = engine
        .set_property(
            engine.handle(&"r".into()).unwrap(),
            PropertyKey::Shadow,
            serde_json::to_value(Shadow::new(120., 0., 0., Color::BLACK)).unwrap(),
        )
        .unwrap();
    assert_eq!(change.layout.bounds_recomputed, 0);
    assert_eq!(engine.world_bounds(&"g".into()).unwrap(), geometric);
    assert_eq!(
        engine.scene().shadow_reference_bounds(&"g".into()).unwrap(),
        reference
    );
    assert!(
        engine
            .effect_bounds(&"g".into())
            .unwrap()
            .unwrap()
            .size
            .height
            > before.size.height
    );
    assert!(change.damage_bounds().unwrap().contains_rect(before));
}

#[test]
fn mode_changes_invalidate_even_when_conservative_extent_is_unchanged() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(100., 40.)).with_style(Style {
                shadow: Some(shadow(10.)),
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let handle = engine.handle(&"r".into()).unwrap();
    let extent = engine.effect_bounds(&"r".into()).unwrap();
    for effect in [
        Shadow {
            r#box: true,
            ..shadow(10.)
        },
        Shadow {
            r#box: true,
            scale_fixed: true,
            ..shadow(10.)
        },
    ] {
        let old_revision = engine.layout().get(&"r".into()).unwrap().revision;
        let change = engine
            .set_property(
                handle,
                PropertyKey::Shadow,
                serde_json::to_value(effect).unwrap(),
            )
            .unwrap();
        assert_eq!(change.dirty[&"r".into()], DirtyFlags::PAINT);
        assert_eq!(engine.effect_bounds(&"r".into()).unwrap(), extent);
        assert!(engine.layout().get(&"r".into()).unwrap().revision > old_revision);
        assert!(
            change
                .damage_bounds()
                .unwrap()
                .contains_rect(extent.unwrap())
        );
    }
}

#[test]
fn zero_width_box_edit_preserves_committed_engine_state() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("b", display::Box::new(20., 40.)).with_style(Style {
                shadow: Some(shadow(1.)),
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(10., 10.)),
            Some("b".into()),
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let before = engine.scene().clone();
    let revision = engine.revision();
    assert!(
        engine
            .transaction(|scene| {
                scene.update_object(&"b".into(), |object| {
                    if let Shape::Box(shape) = &mut object.shape {
                        shape.size.width = 0.;
                    }
                })
            })
            .is_err()
    );
    assert_eq!(engine.scene(), &before);
    assert_eq!(engine.revision(), revision);
}
