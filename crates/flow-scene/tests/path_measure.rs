use gpui_flow_scene::{
    Affine, Point,
    display::{Path, PathCommand as C},
};
fn near(a: f64, b: f64) {
    assert!((a - b).abs() < 0.002, "{a} != {b}");
}
fn point(a: Point, b: Point) {
    near(a.x as f64, b.x as f64);
    near(a.y as f64, b.y as f64);
}

#[test]
fn line_length_distance_and_fraction_are_exact() {
    let measure = Path::from_svg("M0 0L3 4").unwrap().measure().unwrap();
    near(measure.total_length(), 5.);
    let half = measure.sample(2.5).unwrap().unwrap();
    point(half.point, Point::new(1.5, 2.));
    point(half.tangent, Point::new(0.6, 0.8));
    near(half.rotation_degrees as f64, 53.130102);
    assert_eq!(measure.sample_fraction(0.5).unwrap(), Some(half));
}
#[test]
fn quadratic_uses_analytic_curve_distance_not_its_control_polygon() {
    let measure = Path::from_svg("M0 0Q1 1 2 0").unwrap().measure().unwrap();
    near(measure.total_length(), 2f64.sqrt() + 1f64.asinh());
    let half = measure.sample_fraction(0.5).unwrap().unwrap();
    point(half.point, Point::new(1., 0.5));
    point(half.tangent, Point::new(1., 0.));
}
#[test]
fn cubic_uses_arc_length_and_retains_endpoint_tangents() {
    let measure = Path::from_svg("M0 0C0 10 10 10 10 0")
        .unwrap()
        .measure()
        .unwrap();
    near(measure.total_length(), 20.);
    point(
        measure.sample(10.).unwrap().unwrap().point,
        Point::new(5., 7.5),
    );
    point(
        measure.sample(0.).unwrap().unwrap().tangent,
        Point::new(0., 1.),
    );
    point(
        measure.sample(20.).unwrap().unwrap().tangent,
        Point::new(0., -1.),
    );
}
#[test]
fn closes_include_the_real_edge_and_continue_from_subpath_start() {
    let measure = Path::from_svg("M0 0L3 0L3 4Zl2 0")
        .unwrap()
        .measure()
        .unwrap();
    near(measure.total_length(), 14.);
    let close = measure.sample(10.).unwrap().unwrap();
    point(close.point, Point::new(1.2, 1.6));
    point(close.tangent, Point::new(-0.6, -0.8));
    point(
        measure.sample(13.).unwrap().unwrap().point,
        Point::new(1., 0.),
    );
}
#[test]
fn disjoint_subpaths_add_no_move_distance_and_keep_boundary_policy() {
    let measure = Path::from_svg("M0 0L10 0M100 10L110 10")
        .unwrap()
        .measure()
        .unwrap();
    near(measure.total_length(), 20.);
    point(
        measure.sample(10.).unwrap().unwrap().point,
        Point::new(10., 0.),
    );
    point(
        measure.sample(10.5).unwrap().unwrap().point,
        Point::new(100.5, 10.),
    );
}
#[test]
fn empty_moves_and_zero_length_segments_do_not_invent_a_direction() {
    for data in ["", "M4 5", "M0 0L0 0Q0 0 0 0C0 0 0 0 0 0ZM9 9"] {
        let measure = Path::from_svg(data).unwrap().measure().unwrap();
        assert_eq!(measure.total_length(), 0.);
        assert_eq!(measure.sample(0.).unwrap(), None);
    }
    let measure = Path::from_svg("M0 0L0 0L10 0L10 0M99 99")
        .unwrap()
        .measure()
        .unwrap();
    point(
        measure.sample(99.).unwrap().unwrap().point,
        Point::new(10., 0.),
    );
}
#[test]
fn distance_and_fraction_clamp_but_nonfinite_inputs_reject() {
    let measure = Path::from_svg("M0 0L10 0").unwrap().measure().unwrap();
    point(measure.sample(-50.).unwrap().unwrap().point, Point::ZERO);
    point(
        measure.sample(50.).unwrap().unwrap().point,
        Point::new(10., 0.),
    );
    assert_eq!(
        measure.sample_fraction(-1.).unwrap(),
        measure.sample(0.).unwrap()
    );
    assert_eq!(
        measure.sample_fraction(2.).unwrap(),
        measure.sample(10.).unwrap()
    );
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(measure.sample(invalid).is_err());
        assert!(measure.sample_fraction(invalid).is_err());
        assert!(Path::default().measure().unwrap().sample(invalid).is_err());
    }
}
#[test]
fn degenerate_curve_end_controls_keep_forward_direction() {
    for data in [
        "M0 0Q0 0 10 0",
        "M0 0Q10 0 10 0",
        "M0 0C0 0 0 0 10 0",
        "M0 0C10 0 10 0 10 0",
    ] {
        let measure = Path::from_svg(data).unwrap().measure().unwrap();
        near(measure.total_length(), 10.);
        for distance in [0., 5., 10.] {
            point(
                measure.sample(distance).unwrap().unwrap().tangent,
                Point::new(1., 0.),
            );
        }
    }
}
#[test]
fn stationary_cusp_has_explicit_incoming_and_outgoing_directions() {
    let measure = Path::from_svg("M0 0Q10 0 0 0").unwrap().measure().unwrap();
    near(measure.total_length(), 10.);
    let sample = measure.sample(5.).unwrap().unwrap();
    point(sample.point, Point::new(5., 0.));
    point(sample.tangent, Point::new(1., 0.));
    point(
        measure.sample(5.1).unwrap().unwrap().tangent,
        Point::new(-1., 0.),
    );
}
#[test]
fn transformed_samples_use_full_affine_direction_and_reject_collapse() {
    let measure = Path::from_svg("M0 0L10 10").unwrap().measure().unwrap();
    let sample = measure.sample_fraction(0.5).unwrap().unwrap();
    let transformed = sample
        .transformed(Affine::new(-2., 1., 0., 3., 10., 20.))
        .unwrap();
    point(transformed.point, Point::new(0., 40.));
    point(
        transformed.tangent,
        Point::new(-1. / 5f32.sqrt(), 2. / 5f32.sqrt()),
    );
    near(transformed.rotation_degrees as f64, 116.56505);
    assert!(sample.transformed(Affine::scale(0., 0.)).is_err());
    assert!(
        sample
            .transformed(Affine::translation(f32::NAN, 0.))
            .is_err()
    );
}
#[test]
fn snapshots_are_immutable_and_measurement_limits_are_explicit() {
    let mut path = Path::from_svg("M0 0L10 0").unwrap();
    let before = path.measure().unwrap();
    path.commands.push(C::LineTo {
        to: Point::new(20., 0.),
    });
    near(before.total_length(), 10.);
    near(path.measure().unwrap().total_length(), 20.);
    assert!(Path::new(vec![C::Close]).measure().is_err());
    assert!(
        Path::new(vec![C::MoveTo { to: Point::ZERO }; 16_385])
            .measure()
            .is_err()
    );
    assert!(
        Path::new(vec![C::MoveTo {
            to: Point::new(1_000_001., 0.)
        }])
        .measure()
        .is_err()
    );
}
#[test]
fn unrepresentable_accumulated_length_is_rejected_instead_of_dropping_a_segment() {
    let path = Path::new(vec![
        C::MoveTo { to: Point::ZERO },
        C::LineTo {
            to: Point::new(1_000_000., 0.),
        },
        C::MoveTo { to: Point::ZERO },
        C::LineTo {
            to: Point::new(f32::from_bits(1), 0.),
        },
    ]);
    assert!(path.measure().unwrap_err().contains("numeric precision"));
}
