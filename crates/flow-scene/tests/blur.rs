use gpui_flow_scene::{display, *};
use serde_json::json;

fn scene() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    scene
}

#[test]
fn blur_old_default_and_bounded_scalars_serialize_explicitly() {
    let old: Style = serde_json::from_str("{}").unwrap();
    assert_eq!(old.blur, 0.);
    assert!(!old.has_blur());
    assert_eq!(serde_json::to_value(&old).unwrap()["blur"], json!(0.));
    let original = scene();
    let current_json = original.to_json().unwrap();
    let old_json = current_json
        .lines()
        .filter(|line| !line.contains("\"blur\":"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(old_json, current_json);
    assert_eq!(Scene::from_json(&old_json).unwrap(), original);
    for amount in [0., -0., 0.25, 256.] {
        let mut scene = scene();
        scene
            .update_object(&"r".into(), |o| o.style.blur = amount)
            .unwrap();
        let object = &scene.get(&"r".into()).unwrap().object;
        assert_eq!(object.style.has_blur(), amount > 0.);
        assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    }
}

#[test]
fn blur_rejects_invalid_values_and_forms_atomically_even_when_hidden() {
    let mut scene = scene();
    scene
        .update_object(&"r".into(), |o| {
            o.visible = false;
            o.style.opacity = 0.;
        })
        .unwrap();
    let before = scene.clone();
    for amount in [-0.001, 256.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            scene
                .update_object(&"r".into(), |o| o.style.blur = amount)
                .is_err()
        );
        assert_eq!(scene, before);
        let mut object = DisplayObject::new("bad", display::Rect::new(2., 2.));
        object.visible = false;
        object.style.opacity = 0.;
        object.style.blur = amount;
        assert!(object.validate().is_err());
        assert!(scene.insert(object, None, None).is_err());
        assert_eq!(scene, before);
    }
    for value in [
        json!(null),
        json!(true),
        json!("2"),
        json!({"blur": 2}),
        json!([]),
    ] {
        assert!(serde_json::from_value::<Style>(json!({"blur": value})).is_err());
    }
    let encoded = before.to_json().unwrap();
    for value in ["-1", "257", "null", "true", "\"2\"", "{\"blur\":2}", "[]"] {
        let bad = encoded.replace("\"blur\": 0.0", &format!("\"blur\": {value}"));
        assert_ne!(bad, encoded);
        assert!(Scene::from_json(&bad).is_err());
    }
}

#[test]
fn strict_leafer_rejects_all_blur_inputs_and_nonzero_hidden_exports() {
    for field in ["blur", "backgroundBlur"] {
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
            let mut input = json!({"tag":"Rect","id":"r","width":20,"height":20});
            input[field] = value;
            assert!(Scene::from_leafer_json(&input.to_string()).is_err());
            let mut target = scene();
            let before = target.clone();
            assert!(target.replace_from_leafer_json(&input.to_string()).is_err());
            assert_eq!(target, before);
        }
    }
    let mut scene = scene();
    for amount in [0., -0.] {
        scene
            .update_object(&"r".into(), |o| o.style.blur = amount)
            .unwrap();
        let encoded = scene.to_leafer_json().unwrap();
        assert!(!encoded.contains("blur"));
        assert!(!encoded.contains("backgroundBlur"));
        assert_eq!(Scene::from_leafer_json(&encoded).unwrap(), scene);
    }
    for (visible, opacity) in [(true, 1.), (false, 1.), (true, 0.), (false, 0.)] {
        scene
            .update_object(&"r".into(), |o| {
                o.style.blur = 2.;
                o.visible = visible;
                o.style.opacity = opacity;
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
fn blur_expands_after_own_shadows_without_changing_geometry_or_hit_targets() {
    let mut scene = Scene::new();
    let object = DisplayObject::new("r", display::Rect::new(10., 10.)).with_style(Style {
        shadow: Some(Shadow::new(20., 0., 4., Color::BLACK)),
        blur: 2.,
        ..Default::default()
    });
    assert_eq!(
        object.paint_bounds().unwrap(),
        Bounds::new(Point::ZERO, Size::new(10., 10.))
    );
    assert_eq!(
        object.effect_bounds().unwrap(),
        Bounds::new(Point::new(-8., -16.), Size::new(54., 42.))
    );
    scene.insert(object, None, None).unwrap();
    assert_eq!(
        scene.world_bounds(&"r".into()).unwrap().unwrap().size,
        Size::new(10., 10.)
    );
    assert!(
        scene
            .hit_test(Point::new(-5., 5.), Default::default())
            .unwrap()
            .is_none()
    );
    scene
        .update_object(&"r".into(), |o| {
            o.style.blur = -0.;
            o.style.shadow = None;
        })
        .unwrap();
    let object = &scene.get(&"r".into()).unwrap().object;
    assert_eq!(object.effect_bounds(), object.paint_bounds());
}

#[test]
fn blur_analytic_bounds_reject_overflow_without_swallowing_invalid_shadows() {
    let bounds = Bounds::new(Point::ZERO, Size::new(1e-20, 1e-20));
    let style = Style {
        blur: 256.,
        ..Default::default()
    };
    assert!(
        !style
            .expand_effect_bounds(bounds, Affine::scale(1e38, 1e38))
            .is_valid()
    );
    let style = Style {
        blur: 1.,
        shadow: Some(Shadow::new(0., 0., 256., Color::BLACK)),
        ..Default::default()
    };
    assert!(
        !style
            .expand_effect_bounds(bounds, Affine::scale(1e38, 1e38))
            .is_valid()
    );
}
