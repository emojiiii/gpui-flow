use gpui_flow_scene::{display, *};
use serde_json::json;

fn rect(style: Style) -> DisplayObject {
    DisplayObject::new("layers", display::Rect::new(20.0, 10.0)).with_style(style)
}
fn linear(color: Color) -> Paint {
    Paint::LinearGradient {
        start: Point::ZERO,
        end: Point::new(20.0, 0.0),
        stops: vec![
            GradientStop { offset: 0.0, color },
            GradientStop { offset: 1.0, color },
        ],
    }
}
#[test]
fn legacy_styles_default_to_empty_stacks_and_roundtrip() {
    let style: Style = serde_json::from_value(json!({
        "fill": {"type":"solid", "color":{"r":12,"g":34,"b":56,"a":255}},
        "stroke": {"paint":{"type":"solid", "color":{"r":0,"g":0,"b":0,"a":255}}, "width":4.0},
        "opacity":0.75, "fill_rule":"even_odd"
    }))
    .unwrap();
    assert!(!style.has_layers());
    assert_eq!(style.fills().count(), 1);
    assert_eq!(style.strokes().count(), 1);
    assert_eq!(style.stroke_extent(), 2.0);
    assert_eq!(
        serde_json::from_value::<Style>(serde_json::to_value(&style).unwrap()).unwrap(),
        style
    );
    let mut scene = Scene::new();
    scene.insert(rect(style), None, None).unwrap();
    let mut old = serde_json::to_value(&scene).unwrap();
    // Remove every stack field from an otherwise versioned scene document.
    fn remove_stacks(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Object(map) => {
                map.remove("fill_layers");
                map.remove("stroke_layers");
                for value in map.values_mut() {
                    remove_stacks(value);
                }
            }
            serde_json::Value::Array(items) => {
                for value in items {
                    remove_stacks(value);
                }
            }
            _ => {}
        }
    }
    remove_stacks(&mut old);
    assert_eq!(
        Scene::from_json(&serde_json::to_string(&old).unwrap()).unwrap(),
        scene
    );
}
#[test]
fn stacks_preserve_order_and_gradient_data_across_scene_roundtrip() {
    let style = Style {
        fill_layers: vec![linear(Color::rgba(255, 0, 0, 128)), Color::WHITE.into()],
        stroke: Some(Stroke::new(Color::BLACK, 8.0)),
        stroke_layers: vec![Stroke {
            paint: linear(Color::WHITE),
            width: 2.0,
            ..Stroke::default()
        }],
        ..Style::default()
    };
    assert_eq!(
        style.fills().cloned().collect::<Vec<_>>(),
        vec![
            Color::BLACK.into(),
            linear(Color::rgba(255, 0, 0, 128)),
            Color::WHITE.into()
        ]
    );
    assert_eq!(
        style.strokes().map(|s| s.width).collect::<Vec<_>>(),
        vec![8.0, 2.0]
    );
    let mut scene = Scene::new();
    scene.insert(rect(style), None, None).unwrap();
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
}
#[test]
fn bounds_and_picking_use_widest_visible_stroke_and_every_fill() {
    let style = Style {
        fill: None,
        stroke: Some(Stroke::new(Color::TRANSPARENT, 100.0)),
        stroke_layers: vec![
            Stroke::new(Color::BLACK, 8.0),
            Stroke {
                paint: linear(Color::TRANSPARENT),
                width: 200.0,
                ..Stroke::default()
            },
            Stroke::new(Color::WHITE, 0.0),
            Stroke::new(Color::WHITE, 2.0),
        ],
        ..Style::default()
    };
    let mut object = rect(style);
    assert_eq!(
        object.paint_bounds().unwrap(),
        Bounds::from_points(Point::new(-4.0, -4.0), Point::new(24.0, 14.0))
    );
    assert!(
        object
            .shape
            .hit_test(Point::new(-3.5, 5.0), &object.style, 0.0)
    );
    assert!(
        !object
            .shape
            .hit_test(Point::new(-4.5, 5.0), &object.style, 0.0)
    );
    assert!(
        !object
            .shape
            .hit_test(Point::new(10.0, 5.0), &object.style, 0.0)
    );
    object.style.fill_layers.push(linear(Color::TRANSPARENT));
    assert!(
        !object
            .shape
            .hit_test(Point::new(10.0, 5.0), &object.style, 0.0)
    );
    object.style.fill_layers.push(linear(Color::WHITE));
    assert!(
        object
            .shape
            .hit_test(Point::new(10.0, 5.0), &object.style, 0.0)
    );
    object.style.stroke_layers.clear();
    assert_eq!(object.paint_bounds(), object.shape.local_bounds());
}
#[test]
fn transformed_layer_strokes_obey_rounded_parent_clips() {
    let mut scene = Scene::new();
    let mut frame = DisplayObject::new("frame", display::Frame::new(40.0, 30.0));
    frame.hit_policy = HitPolicy::None;
    frame.transform = Affine::translation(50.0, 20.0) * Affine::scale(2.0, 1.0);
    scene.insert(frame, None, None).unwrap();
    let style = Style {
        fill: None,
        stroke_layers: vec![Stroke::new(Color::WHITE, 8.0)],
        ..Style::default()
    };
    scene
        .insert(
            rect(style).with_transform(Affine::translation(2.0, 10.0)),
            Some("frame".into()),
            None,
        )
        .unwrap();
    assert!(
        scene
            .hit_test(Point::new(49.0, 35.0), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        scene
            .hit_test(Point::new(51.0, 35.0), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "layers"
    );
}
#[test]
fn invalid_stack_inputs_are_atomic_and_bounded() {
    let mut scene = Scene::new();
    scene.insert(rect(Style::default()), None, None).unwrap();
    let before = scene.clone();
    for width in [-1.0, f32::INFINITY, f32::NAN] {
        assert!(
            scene
                .update_object(&"layers".into(), |o| o
                    .style
                    .stroke_layers
                    .push(Stroke::new(Color::BLACK, width)))
                .is_err()
        );
        assert_eq!(scene, before);
    }
    assert!(
        scene
            .update_object(&"layers".into(), |o| o.style.fill_layers.push(
                Paint::RadialGradient {
                    center: Point::ZERO,
                    radius: -1.0,
                    stops: vec![]
                }
            ))
            .is_err()
    );
    assert_eq!(scene, before);
    assert!(
        scene
            .update_object(&"layers".into(), |o| o.style.fill_layers =
                vec![Color::WHITE.into(); MAX_PAINT_LAYERS])
            .is_err()
    );
    assert_eq!(scene, before);
    scene
        .update_object(&"layers".into(), |o| {
            o.style.fill_layers = vec![Color::WHITE.into(); MAX_PAINT_LAYERS - 1]
        })
        .unwrap();
    let at_cap = scene.clone();
    assert!(
        scene
            .update_object(&"layers".into(), |o| o.style.stroke =
                Some(Stroke::new(Color::BLACK, 1.0)))
            .is_err()
    );
    assert_eq!(scene, at_cap);
}
#[test]
fn unsupported_nonvector_layers_are_rejected_without_losing_legacy_support() {
    for shape in [
        Shape::Text(display::Text::new("text", Size::new(20.0, 10.0))),
        Shape::Image(display::Image::new("test.png", Size::new(20.0, 10.0))),
        Shape::Group(display::Group::new()),
    ] {
        let mut object = DisplayObject::new("unsupported", shape);
        object.validate().unwrap();
        object.style.fill_layers.push(Color::WHITE.into());
        assert!(
            object
                .validate()
                .unwrap_err()
                .contains("require vector geometry")
        );
        object.style.fill_layers.clear();
        object
            .style
            .stroke_layers
            .push(Stroke::new(Color::WHITE, 2.0));
        assert!(object.validate().is_err());
    }
}
