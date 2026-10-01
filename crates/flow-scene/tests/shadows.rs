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
