use gpui_flow_scene::{Affine, Bounds, DisplayObject, Point, Scene, Size, display as d};
use serde_json::json;

fn quadratic([from, control, to]: [Point; 3]) -> d::Path {
    d::Pen::new()
        .move_to(from)
        .quadratic_to(control, to)
        .into_path()
}

fn cubic([from, control1, control2, to]: [Point; 4]) -> d::Path {
    d::Pen::new()
        .move_to(from)
        .cubic_to(control1, control2, to)
        .into_path()
}

fn close(actual: f32, expected: f32) {
    let tolerance = (expected.abs() * 2e-6).max(1e-6);
    assert!(
        (actual - expected).abs() <= tolerance,
        "{actual} != {expected} (tolerance {tolerance})"
    );
}

#[test]
fn quadratic_bounds_include_independent_axis_extrema() {
    let controls = [
        Point::new(0.0, 4.0),
        Point::new(12.0, -8.0),
        Point::new(4.0, 4.0),
    ];
    let bounds = quadratic(controls).bounds().unwrap();
    assert!(bounds.is_valid());
    close(bounds.min_x(), 0.0);
    close(bounds.max_x(), 7.2);
    close(bounds.min_y(), -2.0);
    close(bounds.max_y(), 4.0);

    let reversed = quadratic([controls[2], controls[1], controls[0]])
        .bounds()
        .unwrap();
    close(reversed.min_x(), bounds.min_x());
    close(reversed.max_x(), bounds.max_x());
    close(reversed.min_y(), bounds.min_y());
    close(reversed.max_y(), bounds.max_y());
}

#[test]
fn cubic_bounds_include_both_interior_extrema() {
    let path = cubic([
        Point::ZERO,
        Point::new(12.0, 12.0),
        Point::new(-12.0, -12.0),
        Point::ZERO,
    ]);
    let bounds = path.bounds().unwrap();
    let extent = 2.0 * 3.0_f32.sqrt();
    close(bounds.min_x(), -extent);
    close(bounds.max_x(), extent);
    close(bounds.min_y(), -extent);
    close(bounds.max_y(), extent);
    assert!(bounds.is_valid());
    assert!(bounds.size.width < 24.0);
}

#[test]
fn almost_quadratic_cubic_does_not_lose_its_interior_maximum() {
    // A naive quadratic formula loses the small root when its discriminant
    // rounds to b*b. A tiny cubic coefficient must not reduce this to endpoints.
    for end_y in [0.0, 1e-20, -1e-20] {
        let path = cubic([
            Point::ZERO,
            Point::new(1.0, 1.0),
            Point::new(2.0, 1.0),
            Point::new(3.0, end_y),
        ]);
        let bounds = path.bounds().unwrap();
        close(bounds.max_y(), 0.75);
        close(bounds.min_x(), 0.0);
        close(bounds.max_x(), 3.0);
        assert!(path.validate().is_ok());
    }
}

#[test]
fn tiny_cubic_keeps_two_extrema_without_an_absolute_epsilon_cutoff() {
    let scale = 1e-20;
    let path = cubic([
        Point::ZERO,
        Point::new(scale, scale),
        Point::new(-scale, -scale),
        Point::ZERO,
    ]);
    let bounds = path.bounds().unwrap();
    let extent = 1.0 / (2.0 * 3.0_f32.sqrt());
    // Normalize the assertion rather than allowing an absolute tolerance to
    // hide missing extrema at this scale.
    close(bounds.min_x() / scale, -extent);
    close(bounds.max_x() / scale, extent);
    close(bounds.min_y() / scale, -extent);
    close(bounds.max_y() / scale, extent);
    assert!(path.validate().is_ok());
}

#[test]
fn degenerate_curves_keep_point_line_and_endpoint_bounds() {
    for point in [
        Point::ZERO,
        Point::new(-3.0, 9.0),
        Point::new(f32::MAX, f32::MAX),
    ] {
        for path in [quadratic([point; 3]), cubic([point; 4])] {
            assert_eq!(path.bounds(), Some(Bounds::new(point, Size::new(0.0, 0.0))));
            assert!(path.validate().is_ok());
        }
    }
    let from = Point::new(-4.0, 2.0);
    let to = Point::new(8.0, 2.0);
    for path in [
        quadratic([from, Point::new(2.0, 2.0), to]),
        quadratic([from, from, to]),
        quadratic([from, to, to]),
        cubic([from, Point::new(0.0, 2.0), Point::new(4.0, 2.0), to]),
        cubic([from, from, from, to]),
        cubic([from, to, to, to]),
        // A repeated derivative root is stationary but introduces no new bound.
        cubic([
            Point::new(-1.0, 2.0),
            Point::new(1.0, 2.0),
            Point::new(-1.0, 2.0),
            Point::new(1.0, 2.0),
        ]),
    ] {
        let first = match path.commands[0] {
            d::PathCommand::MoveTo { to } => to,
            _ => unreachable!(),
        };
        let end = match path.commands[1] {
            d::PathCommand::QuadraticTo { to, .. } | d::PathCommand::CubicTo { to, .. } => to,
            _ => unreachable!(),
        };
        assert_eq!(path.bounds(), Some(Bounds::from_points(first, end)));
        assert!(path.validate().is_ok());
    }
}

#[test]
fn finite_large_controls_do_not_overflow_the_curve_calculation() {
    let quadratic = quadratic([Point::ZERO, Point::new(f32::MAX, f32::MAX), Point::ZERO]);
    let cubic = cubic([
        Point::ZERO,
        Point::new(f32::MAX, f32::MAX),
        Point::new(f32::MAX, f32::MAX),
        Point::ZERO,
    ]);
    for (path, fraction) in [(quadratic, 0.5), (cubic, 0.75)] {
        let bounds = path.bounds().unwrap();
        assert!(bounds.is_valid());
        close(bounds.max_x() / f32::MAX, fraction);
        close(bounds.max_y() / f32::MAX, fraction);
        assert!(path.validate().is_ok());
    }
}

#[test]
fn overflowing_control_hull_does_not_reject_representable_tight_bounds() {
    let path = cubic([
        Point::ZERO,
        Point::new(f32::MAX, f32::MAX),
        Point::new(-f32::MAX, -f32::MAX),
        Point::ZERO,
    ]);
    let bounds = path.bounds().unwrap();
    assert!(bounds.is_valid());
    close(bounds.size.width / f32::MAX, 1.0 / 3.0_f32.sqrt());
    close(bounds.size.height / f32::MAX, 1.0 / 3.0_f32.sqrt());
    assert!(path.validate().is_ok());
}

#[test]
fn unrepresentable_curve_or_aggregate_extents_still_reject() {
    let from = Point::new(-f32::MAX, 0.0);
    let to = Point::new(f32::MAX, 0.0);
    for path in [
        quadratic([from, Point::ZERO, to]),
        cubic([from, from, to, to]),
        d::Pen::new().move_to(from).move_to(to).into_path(),
    ] {
        assert!(!path.bounds().unwrap().is_valid());
        assert_eq!(path.validate().unwrap_err(), "path bounds overflow");
    }
}

#[test]
fn nonfinite_endpoints_and_controls_never_produce_valid_bounds() {
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        for bad_point in [Point::new(value, 0.0), Point::new(0.0, value)] {
            for index in 0..3 {
                let mut controls = [Point::ZERO; 3];
                controls[index] = bad_point;
                let path = quadratic(controls);
                assert!(!path.bounds().unwrap().is_valid());
                assert_eq!(
                    path.validate().unwrap_err(),
                    "path coordinates must be finite"
                );
            }
            for index in 0..4 {
                let mut controls = [Point::ZERO; 4];
                controls[index] = bad_point;
                let path = cubic(controls);
                assert!(!path.bounds().unwrap().is_valid());
                assert_eq!(
                    path.validate().unwrap_err(),
                    "path coordinates must be finite"
                );
            }
        }
    }
}

#[test]
fn empty_moves_lines_and_close_keep_the_existing_path_boundaries() {
    let empty = d::Path::default();
    assert_eq!(empty.bounds(), None);
    assert!(empty.validate().is_ok());
    let point = Point::new(9.0, -3.0);
    assert_eq!(
        d::Pen::new().move_to(point).bounds(),
        Some(Bounds::new(point, Size::new(0.0, 0.0)))
    );

    let path = d::Pen::new()
        .move_to(Point::new(10.0, 20.0))
        .line_to(Point::new(40.0, 20.0))
        .close()
        .quadratic_to(Point::new(100.0, 60.0), Point::new(10.0, 20.0))
        .move_to(Point::new(-5.0, -7.0))
        .line_to(Point::new(-3.0, -2.0))
        .into_path();
    assert_eq!(
        path.bounds(),
        Some(Bounds::new(Point::new(-5.0, -7.0), Size::new(60.0, 47.0)))
    );
    assert!(path.validate().is_ok());

    for command in [
        d::PathCommand::LineTo { to: point },
        d::PathCommand::QuadraticTo {
            control: point,
            to: point,
        },
        d::PathCommand::CubicTo {
            control1: point,
            control2: point,
            to: point,
        },
        d::PathCommand::Close,
    ] {
        assert_eq!(
            d::Path::new(vec![command]).validate().unwrap_err(),
            "path must begin with MoveTo"
        );
    }
}

#[test]
fn affine_world_bounds_still_transform_the_local_aabb() {
    let path = cubic([
        Point::ZERO,
        Point::new(0.0, 100.0),
        Point::new(100.0, 100.0),
        Point::new(100.0, 0.0),
    ]);
    let local = Bounds::new(Point::ZERO, Size::new(100.0, 75.0));
    assert_eq!(path.bounds(), Some(local));
    let transform = Affine::new(-2.0, 1.0, 3.0, 0.5, 11.0, -7.0);
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("curve", path).with_transform(transform),
            None,
            None,
        )
        .unwrap();
    assert_eq!(scene.local_bounds(&"curve".into()).unwrap(), Some(local));
    let world = scene.world_bounds(&"curve".into()).unwrap().unwrap();
    assert_eq!(world, transform.transform_bounds(local));
    assert_eq!(
        world,
        Bounds::new(Point::new(-189.0, -7.0), Size::new(425.0, 137.5))
    );
}

#[test]
fn public_path_command_json_remains_scene_owned() {
    let value = json!({ "commands": [
        { "command": "move_to", "to": { "x": 0.0, "y": 0.0 } },
        { "command": "quadratic_to", "control": { "x": 4.0, "y": 8.0 },
          "to": { "x": 8.0, "y": 0.0 } },
        { "command": "cubic_to", "control1": { "x": 10.0, "y": -6.0 },
          "control2": { "x": 12.0, "y": 6.0 }, "to": { "x": 14.0, "y": 0.0 } },
        { "command": "close" }
    ] });
    let path: d::Path = serde_json::from_value(value.clone()).unwrap();
    assert!(path.validate().is_ok());
    assert_eq!(serde_json::to_value(&path).unwrap(), value);
    let roundtrip: d::Path = serde_json::from_str(&serde_json::to_string(&path).unwrap()).unwrap();
    assert_eq!(roundtrip, path);
    assert_eq!(roundtrip.bounds(), path.bounds());
}
