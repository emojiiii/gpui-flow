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
fn grayscale_old_default_and_all_scalars_serialize_explicitly() {
    let old: Style = serde_json::from_str("{}").unwrap();
    assert_eq!(old.grayscale, 0.);
    assert_eq!(serde_json::to_value(&old).unwrap()["grayscale"], json!(0.));
    let original = scene();
    let current_json = original.to_json().unwrap();
    let old_json = current_json
        .lines()
        .filter(|line| !line.contains("\"grayscale\":"))
        .collect::<Vec<_>>()
        .join("\n");
    assert_ne!(old_json, current_json);
    assert_eq!(Scene::from_json(&old_json).unwrap(), original);
    for amount in [0., -0., 0.25, 1.] {
        let mut scene = scene();
        scene
            .update_object(&"r".into(), |o| o.style.grayscale = amount)
            .unwrap();
        assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    }
}

#[test]
fn grayscale_rejects_invalid_values_and_types_atomically_even_when_hidden() {
    let mut scene = scene();
    scene
        .update_object(&"r".into(), |o| {
            o.visible = false;
            o.style.opacity = 0.;
        })
        .unwrap();
    let original = scene.clone();
    for amount in [-0.01, 1.01, f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        assert!(
            scene
                .update_object(&"r".into(), |o| o.style.grayscale = amount)
                .is_err()
        );
        assert_eq!(scene, original);
        let mut object = DisplayObject::new("bad", display::Rect::new(2., 2.));
        object.style.grayscale = amount;
        assert!(object.validate().is_err());
        assert!(scene.insert(object, None, None).is_err());
        assert_eq!(scene, original);
    }
    for value in [
        json!(null),
        json!(true),
        json!("50%"),
        json!({"grayscale": 0.5}),
        json!([]),
    ] {
        assert!(serde_json::from_value::<Style>(json!({"grayscale": value})).is_err());
    }
    let encoded = original.to_json().unwrap();
    for value in ["-1", "2", "null", "true", "\"50%\"", "{\"grayscale\":0.5}"] {
        let bad = encoded.replace("\"grayscale\": 0.0", &format!("\"grayscale\": {value}"));
        assert_ne!(bad, encoded);
        assert!(Scene::from_json(&bad).is_err());
    }
}

#[test]
fn grayscale_strict_leafer_rejects_input_and_retained_nonzero_before_serialization() {
    for value in [
        json!(0),
        json!(0.5),
        json!({"grayscale": 1, "visible": true}),
    ] {
        let input =
            json!({"tag": "Rect", "id": "r", "width": 20, "height": 20, "grayscale": value});
        assert!(Scene::from_leafer_json(&input.to_string()).is_err());
        let mut target = scene();
        let before = target.clone();
        assert!(target.replace_from_leafer_json(&input.to_string()).is_err());
        assert_eq!(target, before);
    }
    let mut scene = scene();
    assert_eq!(
        Scene::from_leafer_json(&scene.to_leafer_json().unwrap()).unwrap(),
        scene
    );
    for (visible, opacity) in [(true, 1.), (false, 1.), (true, 0.)] {
        scene
            .update_object(&"r".into(), |o| {
                o.style.grayscale = 0.5;
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
