use gpui_flow_scene::{
    Bounds, DisplayObject, FillRule, Point, Scene, Shape, Size,
    display::{Text, TextWritingMode},
};
use serde_json::{Value, json};

const SOURCE: &str = "MiXeD Straße\n世界 words\n";

fn text() -> Text {
    Text::new(SOURCE, Size::new(120., 80.))
}
fn leafer() -> Value {
    json!({"tag":"Text","id":"t","text":SOURCE,"width":120,"height":80,"textOverflow":"hide"})
}
fn scene_text(scene: &Scene) -> &Text {
    let Shape::Text(text) = &scene.get(&"t".into()).unwrap().object.shape else {
        panic!("expected text")
    };
    text
}

#[test]
fn legacy_defaults_and_nonzero_layout_properties_roundtrip_with_canonical_content() {
    let mut text = text();
    let old = json!(text);
    for key in [
        "letter_spacing",
        "word_spacing",
        "para_indent",
        "writing_mode",
        "auto_size_align",
        "layout_origin",
    ] {
        assert!(old.get(key).is_none(), "{key}");
    }
    assert_eq!(serde_json::from_value::<Text>(old).unwrap(), text);
    assert_eq!(text.writing_mode, TextWritingMode::X);
    assert_eq!(json!(TextWritingMode::X), "x");
    text.letter_spacing = 1.5;
    text.word_spacing = 4.;
    text.para_indent = 12.;
    text.auto_size_align = true;
    text.layout_origin = Point::new(-60., -40.);
    text.validate().unwrap();
    let value = json!(text);
    assert_eq!(value["content"], SOURCE);
    assert_eq!(serde_json::from_value::<Text>(value).unwrap(), text);
}

#[test]
fn spacing_is_bounded_and_nonzero_layout_properties_enforce_styled_content_budget() {
    let setters: [fn(&mut Text, f32); 3] = [
        |text, value| text.letter_spacing = value,
        |text, value| text.word_spacing = value,
        |text, value| text.para_indent = value,
    ];
    for set in setters {
        for value in [-1., f32::NAN, f32::INFINITY, 1_000_001.] {
            let mut text = text();
            set(&mut text, value);
            assert!(text.validate().is_err());
        }
        for value in [0., 0.5, 1_000_000.] {
            let mut text = text();
            set(&mut text, value);
            text.validate().unwrap();
        }
        let mut text = Text::new("a".repeat(65_537), Size::new(120., 80.));
        text.validate().unwrap();
        set(&mut text, 1.);
        let mut scene = Scene::new();
        assert!(
            scene
                .insert(DisplayObject::new("t", text), None, None)
                .is_err()
        );
        assert!(scene.is_empty());
    }
}

#[test]
fn measured_origin_moves_bounds_contours_and_hits_together() {
    let mut text = text();
    text.padding = [3., 5., 7., 11.];
    let original = Shape::from(text.clone()).flatten(0.1);
    text.layout_origin = Point::new(-200., -100.);
    text.validate().unwrap();
    assert_eq!(text.bounds(), Bounds::new(text.layout_origin, text.size));
    assert_eq!(
        text.content_bounds(),
        Bounds::new(Point::new(-189., -97.), Size::new(104., 70.))
    );
    let shape = Shape::from(text.clone());
    assert!(shape.contains_geometry(Point::new(-150., -50.), FillRule::NonZero));
    assert!(!shape.contains_geometry(Point::ZERO, FillRule::NonZero));
    let shifted = shape.flatten(0.1);
    assert_eq!(original[0].points.len(), shifted[0].points.len());
    for (before, after) in original[0].points.iter().zip(&shifted[0].points) {
        assert_eq!(*before + text.layout_origin, *after);
    }
    for origin in [
        Point::new(f32::NAN, 0.),
        Point::new(0., f32::INFINITY),
        Point::new(-1_000_001., 0.),
        Point::new(0., 1_000_001.),
    ] {
        text.layout_origin = origin;
        assert!(text.validate().is_err());
    }
    text.layout_origin = Point::new(-1_000_000., 1_000_000.);
    text.validate().unwrap();
}

#[test]
fn leafer_defaults_and_numeric_layout_properties_roundtrip() {
    let scene = Scene::from_leafer_json(&leafer().to_string()).unwrap();
    let imported = scene_text(&scene);
    assert!(imported.auto_size_align);
    assert_eq!(imported.writing_mode, TextWritingMode::X);
    assert_eq!(
        (
            imported.letter_spacing,
            imported.word_spacing,
            imported.para_indent
        ),
        (0., 0., 0.)
    );
    assert_eq!(
        Scene::from_leafer_json(&scene.to_leafer_json().unwrap()).unwrap(),
        scene
    );
    let mut source = leafer();
    for (key, value) in [
        ("letterSpacing", json!(1.5)),
        ("wordSpacing", json!(4.)),
        ("paraIndent", json!(12.)),
        ("writingMode", json!("x")),
        ("autoSizeAlign", json!(false)),
    ] {
        source[key] = value;
    }
    let scene = Scene::from_leafer_json(&source.to_string()).unwrap();
    assert!(!scene_text(&scene).auto_size_align);
    let exported = scene.to_leafer_json().unwrap();
    let value: Value = serde_json::from_str(&exported).unwrap();
    for key in [
        "letterSpacing",
        "wordSpacing",
        "paraIndent",
        "writingMode",
        "autoSizeAlign",
        "text",
    ] {
        assert_eq!(value[0][key], source[key], "{key}");
    }
    assert_eq!(Scene::from_leafer_json(&exported).unwrap(), scene);
    assert_eq!(scene_text(&scene).content, SOURCE);
}

#[test]
fn unsupported_modes_units_and_invalid_layout_values_reject_atomically() {
    for mode in ["horizontal", "x_reverse", "y_reverse", "X", "unknown"] {
        let mut value = json!(text());
        value["writing_mode"] = json!(mode);
        assert!(serde_json::from_value::<Text>(value).is_err(), "{mode}");
        let mut value = leafer();
        value["writingMode"] = json!(mode);
        assert!(
            Scene::from_leafer_json(&value.to_string()).is_err(),
            "{mode}"
        );
    }
    let mut scene = Scene::from_leafer_json(&leafer().to_string()).unwrap();
    let before = scene.clone();
    for key in ["letterSpacing", "wordSpacing", "paraIndent"] {
        for value in [
            json!(-1),
            json!(-1e-100),
            json!(1_000_001),
            json!("4"),
            json!("10%"),
            json!({"type":"percent","value":0.1}),
            Value::Null,
        ] {
            let mut source = leafer();
            source[key] = value;
            assert!(
                scene.replace_from_leafer_json(&source.to_string()).is_err(),
                "{source}"
            );
            assert_eq!(scene, before);
        }
    }
    let mut source = leafer();
    source["autoSizeAlign"] = json!("true");
    assert!(scene.replace_from_leafer_json(&source.to_string()).is_err());
    assert_eq!(scene, before);
}

#[test]
fn computed_origin_cannot_be_lost_through_explicit_size_leafer_export() {
    let mut scene = Scene::new();
    let mut text = text();
    text.layout_origin = Point::new(-60., 0.);
    scene
        .insert(DisplayObject::new("t", text), None, None)
        .unwrap();
    let error = scene.to_leafer_json().unwrap_err();
    assert_eq!(error.path, "t");
    assert!(error.message.contains("layout_origin"));
}

#[test]
fn flow_axis_modes_roundtrip_and_reject_unavailable_style_combinations_atomically() {
    for (mode, spelling) in [
        (TextWritingMode::XReverse, "x-reverse"),
        (TextWritingMode::Y, "y"),
        (TextWritingMode::YReverse, "y-reverse"),
    ] {
        let mut source = leafer();
        source["writingMode"] = json!(spelling);
        let scene = Scene::from_leafer_json(&source.to_string()).unwrap();
        assert_eq!(scene_text(&scene).writing_mode, mode);
        assert_eq!(
            Scene::from_leafer_json(&scene.to_leafer_json().unwrap()).unwrap(),
            scene
        );
        let mut value = text();
        value.writing_mode = mode;
        value.validate().unwrap();
        assert_eq!(serde_json::from_value::<Text>(json!(value)).unwrap(), value);
        let mut scene = scene;
        let before = scene.clone();
        assert!(
            scene
                .update_object(&"t".into(), |o| {
                    let Shape::Text(text) = &mut o.shape else {
                        unreachable!()
                    };
                    text.overflow = gpui_flow_scene::display::TextOverflow::Ellipsis;
                })
                .is_err()
        );
        assert_eq!(scene, before);
        assert!(
            scene
                .update_object(&"t".into(), |o| {
                    let Shape::Text(text) = &mut o.shape else {
                        unreachable!()
                    };
                    text.decoration = gpui_flow_scene::display::TextDecoration::Underline;
                })
                .is_err()
        );
        assert_eq!(scene, before);
        value.content = "a".repeat(65_537);
        assert!(value.validate().is_err());
    }
}
