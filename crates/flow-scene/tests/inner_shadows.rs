use gpui_flow_scene::{display, *};

#[test]
fn inner_layers_default_round_trip_and_reject_atomically() {
    let old: Style = serde_json::from_str("{}").unwrap();
    assert!(old.inner_shadow_layers.is_empty());
    assert_eq!(
        serde_json::to_value(&old).unwrap()["inner_shadow_layers"],
        serde_json::json!([])
    );
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    scene
        .update_object(&"r".into(), |o| {
            o.style.inner_shadow_layers =
                vec![Shadow::new(2., -3., 4., Color::BLACK); MAX_INNER_SHADOW_LAYERS]
        })
        .unwrap();
    let json = scene.to_json().unwrap();
    assert_eq!(Scene::from_json(&json).unwrap(), scene);
    let before = scene.clone();
    for bad in [
        vec![Shadow::new(0., 0., 0., Color::TRANSPARENT); MAX_INNER_SHADOW_LAYERS + 1],
        vec![Shadow::new(f32::NAN, 0., 0., Color::BLACK)],
        vec![Shadow::new(0., 0., -1., Color::BLACK)],
    ] {
        assert!(
            scene
                .update_object(&"r".into(), |o| o.style.inner_shadow_layers = bad.clone())
                .is_err()
        );
        assert_eq!(scene, before);
    }
    let mut value = serde_json::to_value(Shadow::new(0., 0., 0., Color::BLACK)).unwrap();
    value["scale_fixed"] = serde_json::json!(2);
    assert!(
        serde_json::from_value::<Style>(serde_json::json!({"inner_shadow_layers":[value]}))
            .is_err()
    );
}

#[test]
fn inner_shadows_never_expand_geometric_or_outer_effect_bounds() {
    let mut style = Style::default();
    let b = Bounds::new(Point::ZERO, Size::new(20., 20.));
    style
        .inner_shadow_layers
        .push(Shadow::new(4096., -4096., 256., Color::BLACK));
    assert_eq!(style.expand_shadow_bounds(b, Affine::IDENTITY), b);
    style.shadow = Some(Shadow::new(20., 0., 0., Color::BLACK));
    assert_eq!(style.expand_shadow_bounds(b, Affine::IDENTITY).max_x(), 40.);
}
