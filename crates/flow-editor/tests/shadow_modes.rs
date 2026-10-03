use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{DirtyFlags, PropertyKey};
use gpui_flow_scene::{display, *};
use serde_json::{Value, json};

fn shadow(spread: f32, knockout: bool, fixed: bool) -> Shadow {
    Shadow {
        spread,
        r#box: knockout,
        scale_fixed: fixed,
        ..Shadow::new(12., -4., 2., Color::BLACK)
    }
}

fn value(key: &PropertyKey, effect: Shadow) -> Value {
    match key {
        PropertyKey::Shadow => serde_json::to_value(effect).unwrap(),
        _ => serde_json::to_value([Shadow::new(0., 0., 0., Color::TRANSPARENT), effect]).unwrap(),
    }
}

fn contains(outer: Bounds, inner: Bounds) -> bool {
    outer.origin.x <= inner.origin.x
        && outer.origin.y <= inner.origin.y
        && outer.max_x() >= inner.max_x()
        && outer.max_y() >= inner.max_y()
}

#[test]
fn shadow_modes_undo_redo_keep_geometry_identity_selection_and_old_plus_new_damage() {
    for shape in [
        Shape::from(display::Rect::new(20., 10.)),
        display::Group::new().into(),
        display::Box::new(20., 10.).into(),
        display::Frame::new(20., 10.).into(),
    ] {
        for key in [
            PropertyKey::Shadow,
            PropertyKey::ShadowLayers,
            PropertyKey::InnerShadowLayers,
        ] {
            let mut scene = Scene::new();
            let is_container = !matches!(shape, Shape::Rect(_));
            scene
                .insert(
                    DisplayObject::new("owner", shape.clone())
                        .with_style(Style::fill(Color::WHITE)),
                    None,
                    None,
                )
                .unwrap();
            if is_container {
                scene
                    .insert(
                        DisplayObject::new("child", display::Rect::new(8., 6.))
                            .with_style(Style::fill(Color::WHITE)),
                        Some("owner".into()),
                        None,
                    )
                    .unwrap();
            }
            let mut editor = SceneEditor::new(scene).unwrap();
            editor.selection.select("owner".into(), false);
            let handle = editor.engine.handle(&"owner".into()).unwrap();
            let identity = editor.engine.instance(handle).unwrap();
            let geometry = editor.engine.world_bounds(&"owner".into()).unwrap();
            let selection = editor.selection.clone();
            let before = editor.scene().clone();
            let before_effect = editor
                .engine
                .effect_bounds(&"owner".into())
                .unwrap()
                .unwrap();
            editor
                .set_property(handle, key.clone(), value(&key, shadow(3., true, true)))
                .unwrap();
            let after = editor.scene().clone();
            let after_effect = editor
                .engine
                .effect_bounds(&"owner".into())
                .unwrap()
                .unwrap();
            let change = editor.engine.last_change();
            assert_eq!(change.dirty[&"owner".into()], DirtyFlags::PAINT);
            assert_eq!(change.layout.bounds_recomputed, 0);
            let damage = change.damage_bounds().unwrap();
            assert!(contains(damage, before_effect));
            assert!(contains(damage, after_effect));
            assert_eq!(Scene::from_json(&after.to_json().unwrap()).unwrap(), after);
            assert_eq!(
                editor.engine.world_bounds(&"owner".into()).unwrap(),
                geometry
            );
            assert_eq!(editor.engine.instance(handle).unwrap(), identity);
            assert_eq!(editor.selection, selection);
            assert!(editor.undo());
            assert_eq!(editor.scene(), &before);
            assert!(contains(
                editor.engine.last_change().damage_bounds().unwrap(),
                after_effect
            ));
            assert!(editor.redo());
            assert_eq!(editor.scene(), &after);
            assert_eq!(
                editor.engine.effect_bounds(&"owner".into()).unwrap(),
                Some(after_effect)
            );
            assert_eq!(editor.selection, selection);
            assert_eq!(editor.engine.instance(handle).unwrap(), identity);
            assert_eq!(
                editor.engine.world_bounds(&"owner".into()).unwrap(),
                geometry
            );
        }
    }
}

#[test]
fn invalid_mode_fields_in_late_transparent_layers_are_atomic_and_keep_redo() {
    for key in [
        PropertyKey::Shadow,
        PropertyKey::ShadowLayers,
        PropertyKey::InnerShadowLayers,
    ] {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("r", display::Rect::new(20., 10.)),
                None,
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        editor.selection.select("r".into(), false);
        let handle = editor.engine.handle(&"r".into()).unwrap();
        editor
            .set_property(handle, key.clone(), value(&key, shadow(-3., true, true)))
            .unwrap();
        let edited = editor.scene().clone();
        assert!(editor.undo());
        let before = editor.scene().clone();
        let revision = editor.engine.revision();
        let identity = editor.engine.instance(handle).unwrap();
        let selection = editor.selection.clone();
        let damage = editor.engine.last_change().damage.clone();
        for (field, invalid) in [
            ("spread", json!(4097)),
            ("spread", json!(-4097)),
            ("spread", Value::Null),
            ("scale_fixed", json!(1)),
            ("scale_fixed", json!("zoom-in")),
            ("box", json!(1)),
            ("unknown_mode", json!(true)),
        ] {
            let mut bad =
                serde_json::to_value(Shadow::new(0., 0., 0., Color::TRANSPARENT)).unwrap();
            bad[field] = invalid;
            let input = if key == PropertyKey::Shadow {
                bad
            } else {
                json!([shadow(2., true, true), bad])
            };
            assert!(
                editor.set_property(handle, key.clone(), input).is_err(),
                "{key:?}: {field}"
            );
            assert_eq!(editor.scene(), &before);
            assert_eq!(editor.engine.revision(), revision);
            assert_eq!(editor.engine.instance(handle).unwrap(), identity);
            assert_eq!(editor.selection, selection);
            assert_eq!(editor.engine.last_change().damage, damage);
        }
        assert!(editor.redo());
        assert_eq!(editor.scene(), &edited);
    }
}

#[test]
fn inner_box_is_retained_as_an_edit_even_when_pixel_semantics_are_inert() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(20., 10.)).with_style(Style {
                inner_shadow_layers: vec![shadow(2., false, true)],
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    let before = editor.scene().clone();
    let handle = editor.engine.handle(&"r".into()).unwrap();
    editor
        .set_property(
            handle,
            PropertyKey::InnerShadowLayers,
            json!([shadow(2., true, true)]),
        )
        .unwrap();
    let after = editor.scene().clone();
    assert_ne!(before, after);
    assert!(
        after
            .get(&"r".into())
            .unwrap()
            .object
            .style
            .inner_shadow_layers[0]
            .r#box
    );
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
}
