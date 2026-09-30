//! Source-only regressions for the shared advanced-stroke geometry contract.
//! These tests have not been executed during the no-build implementation slice.
use gpui_flow_scene::{display, *};
use serde_json::json;

fn line() -> Shape {
    display::Line::new(Point::ZERO, Point::new(40.0, 0.0)).into()
}
fn style(stroke: Stroke) -> Style {
    Style {
        fill: None,
        stroke: Some(stroke),
        ..Style::default()
    }
}
fn painted(shape: &Shape, stroke: Stroke, point: Point) -> bool {
    shape.hit_test(point, &style(stroke), 0.0)
}

#[test]
fn legacy_stroke_json_and_constructor_keep_solid_round_defaults() {
    let old: Stroke = serde_json::from_value(json!({
        "paint": {"type":"solid", "color":{"r":0,"g":0,"b":0,"a":255}},
        "width": 4.0
    }))
    .unwrap();
    assert_eq!(old, Stroke::new(Color::BLACK, 4.0));
    assert_eq!(old.cap, StrokeCap::Round);
    assert_eq!(old.join, StrokeJoin::Round);
    assert_eq!(old.miter_limit, 4.0);
    assert!(old.dash_pattern.is_empty());
    assert_eq!(old.dash_offset, 0.0);
    assert!(!old.uses_advanced_geometry());
    assert!(painted(&line(), old, Point::new(-1.0, 0.0)));
}

#[test]
fn every_new_stroke_field_roundtrips_with_the_scene() {
    let stroke = Stroke::new(Color::WHITE, 3.0)
        .with_cap(StrokeCap::Square)
        .with_join(StrokeJoin::Miter)
        .with_miter_limit(6.0)
        .with_dashes(vec![5.0, 2.0, 3.0], -7.0);
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("line", line()).with_style(style(stroke)),
            None,
            None,
        )
        .unwrap();
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
}

#[test]
fn butt_round_and_square_caps_have_distinct_hits() {
    let base = Stroke::new(Color::BLACK, 4.0);
    assert!(!painted(
        &line(),
        base.clone().with_cap(StrokeCap::Butt),
        Point::new(-1.0, 0.0)
    ));
    assert!(painted(&line(), base.clone(), Point::new(-1.0, 0.0)));
    assert!(!painted(&line(), base.clone(), Point::new(-1.8, 1.8)));
    assert!(painted(
        &line(),
        base.with_cap(StrokeCap::Square),
        Point::new(-1.8, 1.8)
    ));
}

#[test]
fn dashed_caps_and_tolerance_apply_to_each_dash_not_the_solid_centerline() {
    let shape = line();
    let stroke = Stroke::new(Color::BLACK, 2.0)
        .with_cap(StrokeCap::Butt)
        .with_dashes(vec![6.0, 4.0], 0.0);
    let appearance = style(stroke.clone());
    assert!(shape.hit_test(Point::new(3.0, 0.0), &appearance, 0.0));
    assert!(!shape.hit_test(Point::new(8.0, 0.0), &appearance, 0.0));
    assert!(!shape.hit_test(Point::new(6.5, 0.0), &appearance, 0.0));
    assert!(shape.hit_test(Point::new(6.5, 0.0), &appearance, 0.6));
    assert!(!shape.hit_test(Point::new(8.0, 0.0), &appearance, 0.6));
    assert!(painted(
        &shape,
        stroke.with_cap(StrokeCap::Round),
        Point::new(6.5, 0.0)
    ));
}

#[test]
fn signed_offsets_and_odd_periods_match_svg_pattern_repetition() {
    let base = Stroke::new(Color::BLACK, 2.0).with_cap(StrokeCap::Butt);
    let positive = base.clone().with_dashes(vec![6.0, 4.0], 2.0);
    assert!(!painted(&line(), positive.clone(), Point::new(5.0, 0.0)));
    assert!(painted(&line(), positive, Point::new(9.0, 0.0)));
    let negative = base.clone().with_dashes(vec![6.0, 4.0], -2.0);
    assert!(!painted(&line(), negative.clone(), Point::new(1.0, 0.0)));
    assert!(painted(&line(), negative, Point::new(3.0, 0.0)));
    let odd = base.clone().with_dashes(vec![3.0], 3.0);
    let even = base.with_dashes(vec![3.0, 3.0], 3.0);
    assert!(!painted(&line(), odd.clone(), Point::new(1.0, 0.0)));
    assert!(painted(&line(), odd.clone(), Point::new(4.0, 0.0)));
    assert_eq!(
        line().stroke_mesh(&odd).unwrap(),
        line().stroke_mesh(&even).unwrap()
    );
}

#[test]
fn closed_dash_seam_uses_a_join_and_each_subpath_restarts_phase() {
    let rect: Shape = display::Rect::new(10.0, 10.0).into();
    let stroke = Stroke::new(Color::BLACK, 2.0)
        .with_cap(StrokeCap::Butt)
        .with_join(StrokeJoin::Miter)
        .with_dashes(vec![8.0, 4.0], 0.0);
    assert!(painted(&rect, stroke, Point::new(-0.75, -0.75)));
    let shape: Shape = display::Pen::new()
        .move_to(Point::ZERO)
        .line_to(Point::new(13.0, 0.0))
        .move_to(Point::new(0.0, 10.0))
        .line_to(Point::new(13.0, 10.0))
        .into();
    let stroke = Stroke::new(Color::BLACK, 2.0)
        .with_cap(StrokeCap::Butt)
        .with_dashes(vec![6.0, 4.0], 0.0);
    for y in [0.0, 10.0] {
        assert!(painted(&shape, stroke.clone(), Point::new(3.0, y)));
        assert!(!painted(&shape, stroke.clone(), Point::new(8.0, y)));
    }
}

#[test]
fn bevel_round_and_miter_joins_and_miter_limit_are_distinct() {
    let shape: Shape = display::Pen::new()
        .move_to(Point::new(0.0, 10.0))
        .line_to(Point::ZERO)
        .line_to(Point::new(10.0, 0.0))
        .into();
    let stroke = Stroke::new(Color::BLACK, 4.0).with_cap(StrokeCap::Butt);
    assert!(!painted(
        &shape,
        stroke.clone().with_join(StrokeJoin::Bevel),
        Point::new(-1.2, -1.2)
    ));
    assert!(painted(&shape, stroke.clone(), Point::new(-1.2, -1.2)));
    assert!(!painted(&shape, stroke.clone(), Point::new(-1.8, -1.8)));
    assert!(painted(
        &shape,
        stroke.clone().with_join(StrokeJoin::Miter),
        Point::new(-1.8, -1.8)
    ));
    let acute: Shape = display::Pen::new()
        .move_to(Point::new(-20.0, 2.0))
        .line_to(Point::ZERO)
        .line_to(Point::new(-20.0, -2.0))
        .into();
    let low = stroke
        .clone()
        .with_join(StrokeJoin::Miter)
        .with_miter_limit(1.0);
    let high = stroke.with_join(StrokeJoin::Miter).with_miter_limit(20.0);
    assert!(!painted(&acute, low, Point::new(8.0, 0.0)));
    assert!(painted(&acute, high, Point::new(8.0, 0.0)));
}

#[test]
fn bounds_enclose_diagonal_square_caps_and_long_miters_under_affine_transform() {
    for (shape, stroke) in [
        (
            Shape::from(display::Line::new(Point::ZERO, Point::new(20.0, 20.0))),
            Stroke::new(Color::BLACK, 10.0).with_cap(StrokeCap::Square),
        ),
        (
            Shape::from(
                display::Pen::new()
                    .move_to(Point::new(-20.0, 2.0))
                    .line_to(Point::ZERO)
                    .line_to(Point::new(-20.0, -2.0)),
            ),
            Stroke::new(Color::BLACK, 4.0)
                .with_join(StrokeJoin::Miter)
                .with_miter_limit(20.0),
        ),
    ] {
        let object = DisplayObject::new("shape", shape.clone()).with_style(style(stroke.clone()));
        object.validate().unwrap();
        let transform =
            Affine::translation(30.0, -10.0) * Affine::rotation(0.3) * Affine::scale(2.0, 0.7);
        let bounds = transform.transform_bounds(object.paint_bounds().unwrap());
        for p in shape
            .stroke_mesh(&stroke)
            .unwrap()
            .triangles()
            .iter()
            .flatten()
        {
            assert!(bounds.contains(transform.transform_point(*p)));
        }
    }
}

#[test]
fn mixed_layers_hit_the_union_without_turning_other_dash_gaps_solid() {
    let mut appearance = style(
        Stroke::new(Color::BLACK, 2.0)
            .with_cap(StrokeCap::Butt)
            .with_dashes(vec![6.0, 4.0], 0.0),
    );
    appearance
        .stroke_layers
        .push(Stroke::new(Color::TRANSPARENT, 100.0));
    assert!(!line().hit_test(Point::new(8.0, 0.0), &appearance, 0.0));
    appearance
        .stroke_layers
        .push(Stroke::new(Color::WHITE, 1.0));
    assert!(line().hit_test(Point::new(8.0, 0.0), &appearance, 0.0));
    assert!(!line().hit_test(Point::new(8.0, 0.8), &appearance, 0.0));
}

#[test]
fn invalid_styles_and_dash_work_limits_reject_atomic_updates_and_deserialization() {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("line", line()), None, None)
        .unwrap();
    let before = scene.clone();
    let base = Stroke::new(Color::BLACK, 2.0);
    let mut bad = vec![
        base.clone().with_dashes(vec![0.0, 1.0], 0.0),
        base.clone().with_dashes(vec![-1.0, 1.0], 0.0),
        base.clone().with_dashes(vec![f32::NAN, 1.0], 0.0),
        base.clone().with_dashes(vec![1.0, 1.0], f32::INFINITY),
        base.clone()
            .with_dashes(vec![1.0; MAX_STROKE_DASH_ENTRIES + 1], 0.0),
        base.clone().with_dashes(vec![f32::MIN_POSITIVE, 1.0], 0.0),
        base.clone().with_miter_limit(0.5),
        base.clone().with_miter_limit(f32::INFINITY),
    ];
    bad.push(
        Stroke::new(Color::BLACK, f32::MAX)
            .with_join(StrokeJoin::Miter)
            .with_miter_limit(20.0),
    );
    for stroke in bad {
        assert!(
            scene
                .update_object(&"line".into(), |object| object.style = style(stroke))
                .is_err()
        );
        assert_eq!(scene, before);
    }
    let mut serialized = serde_json::to_value(&scene).unwrap();
    serialized["nodes"]["line"]["object"]["style"]["stroke"] =
        serde_json::to_value(base.with_dashes(vec![0.0, 1.0], 0.0)).unwrap();
    assert!(Scene::from_json(&serialized.to_string()).is_err());
}

#[test]
fn source_and_flattened_work_limits_never_return_partial_meshes() {
    let stroke = Stroke::new(Color::BLACK, 2.0).with_cap(StrokeCap::Butt);
    let oversized: Shape = display::Path::new(vec![
        display::PathCommand::MoveTo { to: Point::ZERO };
        MAX_STROKE_POINTS + 1
    ])
    .into();
    assert!(
        oversized
            .stroke_mesh(&stroke)
            .unwrap_err()
            .contains("budget")
    );
    let mut pen = display::Pen::new().move_to(Point::ZERO);
    for i in 0..32 {
        pen = pen.cubic_to(
            Point::new(1e12, 1e12),
            Point::new(-1e12, 1e12),
            Point::new(i as f32, 0.0),
        );
    }
    let shape: Shape = pen.into();
    assert!(shape.stroke_mesh(&stroke).unwrap_err().contains("budget"));
}

#[test]
fn advanced_strokes_on_nonvectors_are_explicitly_rejected() {
    for shape in [
        Shape::from(display::Text::new("text", Size::new(20.0, 10.0))),
        Shape::from(display::Image::new("test.png", Size::new(20.0, 10.0))),
        Shape::from(display::Group::new()),
    ] {
        let object = DisplayObject::new("unsupported", shape).with_style(style(
            Stroke::new(Color::BLACK, 2.0).with_cap(StrokeCap::Butt),
        ));
        assert!(
            object
                .validate()
                .unwrap_err()
                .contains("require vector geometry")
        );
    }
}
