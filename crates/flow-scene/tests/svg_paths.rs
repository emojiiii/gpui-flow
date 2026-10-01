use gpui_flow_scene::{
    display::{Path, PathCommand as C},
    *,
};

#[test]
fn relative_implicit_lines_horizontal_vertical_and_close_are_normalized() {
    let p = Path::from_svg("m10 20 5-5 h10 v10 z l2 3").unwrap();
    assert_eq!(
        p.commands,
        vec![
            C::MoveTo {
                to: Point::new(10., 20.)
            },
            C::LineTo {
                to: Point::new(15., 15.)
            },
            C::LineTo {
                to: Point::new(25., 15.)
            },
            C::LineTo {
                to: Point::new(25., 25.)
            },
            C::Close,
            C::MoveTo {
                to: Point::new(10., 20.)
            },
            C::LineTo {
                to: Point::new(12., 23.)
            },
        ]
    );
}

#[test]
fn smooth_controls_reflect_only_the_matching_curve_family() {
    let p = Path::from_svg("M0 0 Q10 20 20 0 T40 0 S50 10 60 0 T80 0").unwrap();
    assert_eq!(
        p.commands[2],
        C::QuadraticTo {
            control: Point::new(30., -20.),
            to: Point::new(40., 0.)
        }
    );
    assert_eq!(
        p.commands[3],
        C::CubicTo {
            control1: Point::new(40., 0.),
            control2: Point::new(50., 10.),
            to: Point::new(60., 0.)
        }
    );
    assert_eq!(
        p.commands[4],
        C::QuadraticTo {
            control: Point::new(60., 0.),
            to: Point::new(80., 0.)
        }
    );
    let p = Path::from_svg("M0 0 C5 10 10 10 15 0 s10 -10 15 0").unwrap();
    assert_eq!(
        p.commands[2],
        C::CubicTo {
            control1: Point::new(20., -10.),
            control2: Point::new(25., -10.),
            to: Point::new(30., 0.)
        }
    );
}

#[test]
fn arc_circle_has_real_bounds_pick_and_serializable_curves() {
    let p = Path::from_svg("M40 20 A20 20 0 1 1 0 20 A20 20 0 1 1 40 20Z").unwrap();
    let bounds = p.bounds().unwrap();
    assert!(bounds.min_x().abs() < 0.001 && bounds.min_y().abs() < 0.001);
    assert!((bounds.max_x() - 40.).abs() < 0.001 && (bounds.max_y() - 40.).abs() < 0.001);
    assert!(p.commands.iter().any(|c| matches!(c, C::CubicTo { .. })));
    let json = serde_json::to_string(&p).unwrap();
    assert_eq!(serde_json::from_str::<Path>(&json).unwrap(), p);
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("circle", p), None, None)
        .unwrap();
    assert!(
        scene
            .hit_test(Point::new(20., 20.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    assert!(
        scene
            .hit_test(Point::new(1., 1.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
}

#[test]
fn rotated_relative_arcs_preserve_endpoint_and_reset_smooth_control() {
    for large in [0, 1] {
        for sweep in [0, 1] {
            let p = Path::from_svg(&format!("M10 10 a30 15 37 {large} {sweep} 40 20 s5 5 10 0"))
                .unwrap();
            let n = p.commands.len();
            assert!(matches!(p.commands[n-2],C::CubicTo {to,..} if to==Point::new(50.,30.)));
            assert!(
                matches!(p.commands[n-1],C::CubicTo {control1,to,..} if control1==Point::new(50.,30.) && to==Point::new(60.,30.))
            );
            p.validate().unwrap();
        }
    }
}

#[test]
fn degenerate_arcs_and_radius_correction_follow_svg_rules() {
    let p = Path::from_svg("M0 0 A0 10 0 0 1 20 0 A10 10 0 1 1 20 0").unwrap();
    assert_eq!(p.commands.len(), 2);
    assert!(matches!(p.commands[1],C::LineTo {to} if to==Point::new(20.,0.)));
    let positive = Path::from_svg("M0 0 A1 1 0 0 1 20 0").unwrap();
    let negative = Path::from_svg("M0 0 A-1 -1 0 0 1 20 0").unwrap();
    assert_eq!(positive, negative);
    assert!(
        positive
            .commands
            .iter()
            .any(|c| matches!(c, C::CubicTo { .. }))
    );
}

#[test]
fn malformed_or_nonfinite_input_never_returns_a_partial_path() {
    for s in [
        "L1 2",
        "M1",
        "M0 0 L2",
        "M0 0 nope",
        "M0 0 A1 1 0 2 0 2 2",
        "M1e999 0",
        "M1000001 0",
        "M999999 0 l2 0",
        "M0 0 A1e-300 1000000 0 0 0 100 100",
    ] {
        assert!(Path::from_svg(s).is_err(), "accepted {s}");
    }
    assert!(Path::from_svg("").unwrap().commands.is_empty());
}

#[test]
fn input_and_command_limits_are_enforced_without_recursive_close_processing() {
    assert!(Path::from_svg(&" ".repeat(1_048_577)).is_err());
    assert!(Path::from_svg(&format!("M0 0{}", "L1 1".repeat(16_384))).is_err());
    let p = Path::from_svg(&format!("M0 0{}", "Z".repeat(16_000))).unwrap();
    assert_eq!(p.commands.len(), 2);
}

#[test]
fn subpaths_and_compact_exponents_are_preserved() {
    let p = Path::from_svg("M1e1-2e1L.5.5z m5 5h5v5z").unwrap();
    assert_eq!(
        p.commands[0],
        C::MoveTo {
            to: Point::new(10., -20.)
        }
    );
    assert_eq!(
        p.commands[3],
        C::MoveTo {
            to: Point::new(15., -15.)
        }
    );
    assert_eq!(p.flatten(0.1).len(), 2);
}
