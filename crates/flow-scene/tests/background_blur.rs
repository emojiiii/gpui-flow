use gpui_flow_scene::{display, *};
use serde_json::json;

fn scene() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("glass", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    scene
}

#[test]
fn background_blur_defaults_and_old_documents_preserve_other_effects() {
    let default: Style = serde_json::from_str("{}").unwrap();
    assert_eq!(default.background_blur, 0.);
    assert!(!default.has_background_blur());
    assert_eq!(
        serde_json::to_value(&default).unwrap()["background_blur"],
        json!(0.)
    );

    let mut original = scene();
    original
        .update_object(&"glass".into(), |object| {
            object.style.blur = 2.;
            object.style.grayscale = 0.25;
        })
        .unwrap();
    let encoded = original.to_json().unwrap();
    let old = encoded
        .lines()
        .filter(|line| !line.contains("\"background_blur\":"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(old, encoded);
    assert_eq!(Scene::from_json(&old).unwrap(), original);

    for amount in [0., -0., 0.001, 0.25, 256.] {
        original
            .update_object(&"glass".into(), |object| {
                object.style.background_blur = amount;
            })
            .unwrap();
        let style = &original.get(&"glass".into()).unwrap().object.style;
        assert_eq!(style.has_background_blur(), amount > 0.);
        assert_eq!(style.blur, 2.);
        assert_eq!(style.grayscale, 0.25);
        assert_eq!(
            Scene::from_json(&original.to_json().unwrap()).unwrap(),
            original
        );
    }
}

#[test]
fn invalid_background_blur_is_atomic_even_when_hidden_or_transparent() {
    for (visible, opacity) in [(true, 1.), (false, 1.), (true, 0.), (false, 0.)] {
        let mut scene = scene();
        scene
            .update_object(&"glass".into(), |object| {
                object.visible = visible;
                object.style.opacity = opacity;
            })
            .unwrap();
        let before = scene.clone();
        for amount in [-0.001, 256.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            assert!(
                scene
                    .update_object(&"glass".into(), |object| {
                        object.style.background_blur = amount;
                    })
                    .is_err()
            );
            assert_eq!(scene, before);

            let mut invalid = DisplayObject::new("invalid", display::Rect::new(2., 2.));
            invalid.visible = visible;
            invalid.style.opacity = opacity;
            invalid.style.background_blur = amount;
            assert!(invalid.validate().is_err());
            assert!(scene.insert(invalid, None, None).is_err());
            assert_eq!(scene, before);
        }
    }
}

#[test]
fn background_blur_canonical_json_accepts_only_bounded_scalars() {
    let encoded = scene().to_json().unwrap();
    for value in [
        "-1",
        "256.01",
        "null",
        "true",
        "\"2\"",
        "{\"blur\":2}",
        "[]",
    ] {
        let invalid = encoded.replace(
            "\"background_blur\": 0.0",
            &format!("\"background_blur\": {value}"),
        );
        assert_ne!(invalid, encoded);
        assert!(Scene::from_json(&invalid).is_err(), "accepted {value}");
    }
    for value in [
        json!(null),
        json!(true),
        json!("2"),
        json!({"blur": 2}),
        json!([]),
    ] {
        assert!(serde_json::from_value::<Style>(json!({"background_blur": value})).is_err());
    }
}

#[test]
fn strict_leafer_background_blur_forms_and_retained_exports_stay_rejected() {
    for value in [
        json!(0),
        json!(-0.),
        json!(2),
        json!(null),
        json!(true),
        json!("2"),
        json!({"blur": 2, "visible": false}),
        json!([]),
    ] {
        let leaf = json!({
            "tag": "Rect", "id": "r", "width": 20, "height": 20,
            "backgroundBlur": value
        });
        for input in [leaf.clone(), json!({"tag": "Group", "children": [leaf]})] {
            let input = input.to_string();
            assert!(Scene::from_leafer_json(&input).is_err());
            let mut target = scene();
            let before = target.clone();
            assert!(target.replace_from_leafer_json(&input).is_err());
            assert_eq!(target, before);
        }
    }

    let mut scene = scene();
    for amount in [0., -0.] {
        scene
            .update_object(&"glass".into(), |object| {
                object.style.background_blur = amount
            })
            .unwrap();
        let encoded = scene.to_leafer_json().unwrap();
        assert!(!encoded.contains("background_blur"));
        assert!(!encoded.contains("backgroundBlur"));
        assert_eq!(Scene::from_leafer_json(&encoded).unwrap(), scene);
    }
    for (visible, opacity) in [(true, 1.), (false, 1.), (true, 0.), (false, 0.)] {
        scene
            .update_object(&"glass".into(), |object| {
                object.style.background_blur = 2.;
                object.visible = visible;
                object.style.opacity = opacity;
            })
            .unwrap();
        assert!(
            scene
                .to_leafer_json()
                .unwrap_err()
                .to_string()
                .contains("effects")
        );
    }
}

#[test]
fn background_sample_halo_does_not_expand_geometry_or_analytic_write_bounds() {
    for style in [
        Style::default(),
        Style {
            fill: None,
            ..Default::default()
        },
        Style {
            shadow: Some(Shadow::new(20., 0., 4., Color::BLACK)),
            blur: 2.,
            ..Default::default()
        },
    ] {
        let mut object =
            DisplayObject::new("glass", display::Rect::new(10., 10.)).with_style(style);
        let paint = object.paint_bounds();
        let effects = object.effect_bounds();
        let world = Bounds::new(Point::new(3., 4.), Size::new(20., 20.));
        let expanded = object
            .style
            .expand_effect_bounds(world, Affine::scale(2., 2.));
        object.style.background_blur = 256.;
        assert_eq!(object.paint_bounds(), paint);
        assert_eq!(object.effect_bounds(), effects);
        assert_eq!(
            object
                .style
                .expand_effect_bounds(world, Affine::scale(2., 2.)),
            expanded
        );
        let mut scene = Scene::new();
        scene.insert(object, None, None).unwrap();
        assert_eq!(scene.world_bounds(&"glass".into()).unwrap(), paint);
        assert!(
            scene
                .hit_test(Point::new(-5., 5.), Default::default())
                .unwrap()
                .is_none()
        );
    }
}
