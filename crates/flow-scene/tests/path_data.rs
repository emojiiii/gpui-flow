use gpui_flow_scene::{
    Point,
    display::{Path, PathCommand as C},
};

#[test]
fn svg_output_normalizes_all_commands_and_negative_zero() {
    let path = Path::new(vec![
        C::MoveTo {
            to: Point::new(-0., 2.),
        },
        C::LineTo {
            to: Point::new(3.5, -4.),
        },
        C::QuadraticTo {
            control: Point::new(5., 6.),
            to: Point::new(7., 8.),
        },
        C::CubicTo {
            control1: Point::new(9., 10.),
            control2: Point::new(11., 12.),
            to: Point::new(13., 14.),
        },
        C::Close,
    ]);
    let data = path.to_svg().unwrap();
    assert_eq!(data, "M0 2 L3.5 -4 Q5 6 7 8 C9 10 11 12 13 14 Z");
    assert_eq!(Path::from_svg(&data).unwrap(), path);
}

#[test]
fn svg_import_export_is_stable_for_shorthand_arcs_and_subpaths() {
    for input in [
        "m10 10h20v10z m2 3q4 5 6 7t8 9",
        "M0 0a30 20 25 0 1 40 40s4 5 6 7",
        "M0 0L10 0Zl5 6z",
        "",
    ] {
        let path = Path::from_svg(input).unwrap();
        let data = path.to_svg().unwrap();
        let again = Path::from_svg(&data).unwrap();
        assert_eq!(again, path, "{input}");
        assert_eq!(again.to_svg().unwrap(), data);
    }
}

#[test]
fn typed_close_continuation_preserves_geometry_after_svg_roundtrip() {
    let path = Path::new(vec![
        C::MoveTo {
            to: Point::new(3., 4.),
        },
        C::LineTo {
            to: Point::new(8., 4.),
        },
        C::Close,
        C::LineTo {
            to: Point::new(8., 9.),
        },
    ]);
    let again = Path::from_svg(&path.to_svg().unwrap()).unwrap();
    assert_eq!(again.flatten(0.1), path.flatten(0.1));
    assert_eq!(again.bounds(), path.bounds());
}

#[test]
fn svg_export_rejects_invalid_sequence_and_nonfinite_control_points() {
    for path in [
        Path::new(vec![C::Close]),
        Path::new(vec![C::LineTo { to: Point::ZERO }]),
        Path::new(vec![C::MoveTo {
            to: Point::new(f32::NAN, 0.),
        }]),
        Path::new(vec![
            C::MoveTo { to: Point::ZERO },
            C::QuadraticTo {
                control: Point::new(f32::INFINITY, 0.),
                to: Point::ZERO,
            },
        ]),
        Path::new(vec![
            C::MoveTo { to: Point::ZERO },
            C::CubicTo {
                control1: Point::ZERO,
                control2: Point::new(0., f32::NEG_INFINITY),
                to: Point::ZERO,
            },
        ]),
    ] {
        assert!(path.to_svg().is_err());
    }
}

#[test]
fn svg_export_enforces_coordinate_and_command_boundaries() {
    let at = Path::new(vec![
        C::MoveTo {
            to: Point::new(-1_000_000., 1_000_000.)
        };
        16_384
    ]);
    assert!(at.to_svg().is_ok());
    let mut beyond = at;
    beyond.commands.push(C::Close);
    assert!(beyond.to_svg().is_err());
    assert!(
        Path::new(vec![C::MoveTo {
            to: Point::new(1_000_001., 0.)
        }])
        .to_svg()
        .is_err()
    );
}

#[test]
fn svg_export_enforces_output_byte_budget_for_tiny_coordinates() {
    let tiny = Point::new(f32::from_bits(1), -f32::from_bits(1));
    let mut path = Path::new(vec![C::MoveTo { to: tiny }]);
    path.commands.extend(vec![
        C::CubicTo {
            control1: tiny,
            control2: tiny,
            to: tiny
        };
        8_000
    ]);
    assert!(path.to_svg().unwrap_err().contains("output limit"));
}

#[test]
fn svg_export_roundtrips_smallest_finite_values_and_empty_paths() {
    assert_eq!(Path::default().to_svg().unwrap(), "");
    let path = Path::new(vec![C::MoveTo {
        to: Point::new(f32::from_bits(1), -f32::from_bits(1)),
    }]);
    assert_eq!(Path::from_svg(&path.to_svg().unwrap()).unwrap(), path);
}
