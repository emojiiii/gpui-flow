use gpui_flow_scene::{display, *};

#[test]
fn shadow_roundtrip_and_legacy_default_are_explicit() {
    let old: Style = serde_json::from_str(r#"{"opacity":0.5}"#).unwrap();
    assert_eq!(old.shadow, None);
    let style = Style {
        shadow: Some(Shadow::new(4., -2., 8., Color::rgba(1, 2, 3, 140))),
        ..old
    };
    assert_eq!(
        serde_json::from_str::<Style>(&serde_json::to_string(&style).unwrap()).unwrap(),
        style
    );
    for unsupported in ["spread", "box", "scaleFixed", "blendMode", "inner"] {
        let mut value = serde_json::to_value(style.shadow.unwrap()).unwrap();
        value[unsupported] = serde_json::json!(true);
        assert!(serde_json::from_value::<Shadow>(value).is_err());
    }
    assert!(serde_json::from_str::<Style>(r#"{"shadow":[]}"#).is_err());
}

#[test]
fn shadow_validation_is_finite_bounded_and_atomic() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(10., 10.)),
            None,
            None,
        )
        .unwrap();
    let before = scene.clone();
    for shadow in [
        Shadow::new(f32::NAN, 0., 1., Color::BLACK),
        Shadow::new(0., f32::INFINITY, 1., Color::BLACK),
        Shadow::new(0., 0., -1., Color::BLACK),
        Shadow::new(4097., 0., 0., Color::BLACK),
        Shadow::new(0., 0., 257., Color::BLACK),
    ] {
        assert!(
            scene
                .update_object(&"r".into(), |o| o.style.shadow = Some(shadow))
                .is_err()
        );
        assert_eq!(scene, before);
    }
}

#[test]
fn shadow_effect_extent_does_not_expand_geometry_or_hit_targets() {
    let mut scene = Scene::new();
    let object = DisplayObject::new("r", display::Rect::new(10., 10.)).with_style(Style {
        shadow: Some(Shadow::new(20., 0., 4., Color::BLACK)),
        ..Default::default()
    });
    assert_eq!(object.paint_bounds().unwrap().size, Size::new(10., 10.));
    assert_eq!(
        object.effect_bounds().unwrap(),
        Bounds::new(Point::new(0., -8.), Size::new(38., 26.))
    );
    scene.insert(object, None, None).unwrap();
    assert_eq!(
        scene.world_bounds(&"r".into()).unwrap().unwrap().size,
        Size::new(10., 10.)
    );
    assert!(
        scene
            .hit_test(Point::new(25., 5.), Default::default())
            .unwrap()
            .is_none()
    );
    scene
        .update_object(&"r".into(), |o| {
            o.style.shadow.as_mut().unwrap().color = Color::TRANSPARENT
        })
        .unwrap();
    let object = &scene.get(&"r".into()).unwrap().object;
    assert_eq!(object.effect_bounds(), object.paint_bounds());
}

#[test]
fn multiple_shadow_layers_preserve_legacy_shape_and_bound_the_combined_list() {
    let legacy: Style = serde_json::from_str(
        r#"{"shadow":{"x":2,"y":3,"blur":4,"color":{"r":0,"g":0,"b":0,"a":255}}}"#,
    )
    .unwrap();
    assert!(legacy.shadow_layers.is_empty());
    assert_eq!(
        serde_json::to_value(&legacy).unwrap()["shadow_layers"],
        serde_json::json!([])
    );
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(10., 10.)).with_style(legacy),
        None,
        None,
    )
    .unwrap();
    s.update_object(&"r".into(), |o| {
        o.style.shadow_layers =
            vec![Shadow::new(0., 0., 0., Color::TRANSPARENT); MAX_SHADOW_LAYERS - 1]
    })
    .unwrap();
    assert_eq!(Scene::from_json(&s.to_json().unwrap()).unwrap(), s);
    let before = s.clone();
    for layers in [
        vec![Shadow::new(0., 0., 0., Color::TRANSPARENT); MAX_SHADOW_LAYERS],
        vec![Shadow::new(0., 0., -1., Color::BLACK)],
        vec![Shadow::new(f32::INFINITY, 0., 0., Color::BLACK)],
    ] {
        assert!(
            s.update_object(&"r".into(), |o| o.style.shadow_layers = layers.clone())
                .is_err()
        );
        assert_eq!(s, before);
    }
    // Typed Scene JSON is also validated at the document ingestion boundary.
    let json = s.to_json().unwrap();
    let over = json.replace("\"shadow_layers\": [", "\"shadow_layers\": [{\"x\":0,\"y\":0,\"blur\":0,\"color\":{\"r\":0,\"g\":0,\"b\":0,\"a\":0}},");
    assert_ne!(json, over);
    assert!(Scene::from_json(&over).is_err());
    let mut layer = serde_json::to_value(Shadow::new(0., 0., 0., Color::BLACK)).unwrap();
    layer["spread"] = serde_json::json!(1);
    assert!(serde_json::from_value::<Style>(serde_json::json!({"shadow_layers":[layer]})).is_err());
}

#[test]
fn multiple_shadow_bounds_union_projections_without_chaining() {
    let o = DisplayObject::new("r", display::Rect::new(10., 10.)).with_style(Style {
        shadow: Some(Shadow::new(20., 0., 0., Color::BLACK)),
        shadow_layers: vec![
            Shadow::new(40., 0., 0., Color::BLACK),
            Shadow::new(-10., -5., 2., Color::BLACK),
        ],
        ..Default::default()
    });
    assert_eq!(
        o.effect_bounds(),
        Some(Bounds::new(Point::new(-14., -9.), Size::new(64., 19.)))
    );
    assert_eq!(
        o.paint_bounds(),
        Some(Bounds::new(Point::ZERO, Size::new(10., 10.)))
    );
}

#[test]
fn additional_layer_bounds_do_not_swallow_invalid_halo() {
    let bounds = Bounds::new(Point::ZERO, Size::new(1e-20, 1e-20));
    let style = Style {
        shadow_layers: vec![Shadow::new(0., 0., 256., Color::BLACK)],
        ..Default::default()
    };
    assert!(
        !style
            .expand_shadow_bounds(bounds, Affine::scale(1e38, 1e38))
            .is_valid()
    );
}
