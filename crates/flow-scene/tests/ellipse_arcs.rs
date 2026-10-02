use gpui_flow_scene::{Color, FillRule, Point, Shape, Stroke, StrokeAlignment, display::Ellipse};
use serde_json::json;
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.025, "{a} != {b}");
}
#[test]
fn old_json_and_constructor_keep_full_oval_defaults() {
    let old: Ellipse = serde_json::from_value(json!({"size":{"width":40.,"height":20.}})).unwrap();
    assert_eq!(old, Ellipse::new(40., 20.));
    assert!(old.closed);
    assert!(old.contains(Point::new(20., 10.)));
    assert!(!old.contains(Point::ZERO));
    assert_eq!(old.geometry_bounds(), Some(old.bounds()));
    assert_eq!(old.flatten(0.1).len(), 1);
    assert!(old.flatten(0.1)[0].closed);
}
#[test]
fn quarter_sector_has_real_tight_geometry_bounds_and_radial_fill() {
    let e = Ellipse::new(40., 20.).sector(0., 90.);
    let bounds = e.geometry_bounds().unwrap();
    near(bounds.min_x(), 20.);
    near(bounds.max_x(), 40.);
    near(bounds.min_y(), 10.);
    near(bounds.max_y(), 20.);
    assert!(e.contains(Point::new(24., 12.)));
    assert!(!e.contains(Point::new(16., 12.)));
    assert!(!e.contains(Point::new(24., 8.)));
    assert!(e.flatten(0.1)[0].closed);
    assert_eq!(Shape::from(e.clone()).local_bounds(), e.geometry_bounds());
    assert_eq!(e.bounds().origin, Point::ZERO);
}
#[test]
fn open_arc_has_no_radial_stroke_and_fill_closes_only_the_chord() {
    let e = Ellipse::new(40., 20.).arc(0., 90.);
    assert!(!e.flatten(0.1)[0].closed);
    assert!(!e.contains(Point::new(24., 12.)));
    assert!(e.contains(Point::new(34., 17.)));
    let s = Shape::from(e);
    let style = gpui_flow_scene::Style {
        fill: None,
        stroke: Some(Stroke::new(Color::BLACK, 1.)),
        ..Default::default()
    };
    assert!(!s.hit_test(Point::new(30., 10.), &style, 0.));
    assert!(s.hit_test(Point::new(40., 10.), &style, 0.));
}
#[test]
fn full_ring_returns_opposite_contours_and_a_hole_for_both_rules() {
    let e = Ellipse::circle(20.).ring(0.5);
    let contours = e.flatten(0.05);
    assert_eq!(contours.len(), 2);
    assert!(contours.iter().all(|c| c.closed));
    for rule in [FillRule::NonZero, FillRule::EvenOdd] {
        assert!(!e.contains_with_rule(Point::new(20., 20.), rule));
        assert!(!e.contains_with_rule(Point::new(25., 20.), rule));
        assert!(e.contains_with_rule(Point::new(35., 20.), rule));
    }
    let bounds = e.geometry_bounds().unwrap();
    near(bounds.min_x(), 0.);
    near(bounds.max_x(), 40.);
}
#[test]
fn ring_sector_connects_outer_and_reverse_inner_without_center_fill() {
    let e = Ellipse::circle(20.).sector(0., 90.).ring(0.5);
    let c = e.flatten(0.05);
    assert_eq!(c.len(), 1);
    assert!(c[0].closed);
    assert!(e.contains(Point::new(30., 30.)));
    assert!(!e.contains(Point::new(23., 23.)));
    assert!(!e.contains(Point::new(10., 30.)));
    assert!(!e.contains(Point::new(30., 10.)));
}
#[test]
fn inner_ring_sector_stays_closed_even_when_closed_flag_is_false() {
    let e = Ellipse::circle(20.).arc(0., 90.).ring(0.5);
    assert!(e.flatten(0.1)[0].closed);
    assert!(e.contains(Point::new(30., 30.)));
    assert!(!e.contains(Point::new(23., 23.)));
}
#[test]
fn negative_angles_and_wrapped_clockwise_spans_use_correct_quadrants() {
    let upper_right = Ellipse::circle(20.).sector(-90., 0.);
    assert!(upper_right.contains(Point::new(30., 10.)));
    assert!(!upper_right.contains(Point::new(30., 30.)));
    let wrapped = Ellipse::circle(20.).sector(90., 0.);
    assert!(wrapped.contains(Point::new(10., 30.)));
    assert!(wrapped.contains(Point::new(30., 10.)));
    assert!(!wrapped.contains(Point::new(30., 30.)));
}
#[test]
fn signed_full_revolutions_canonicalize_and_equal_nonzero_angles_are_empty() {
    for end in [360., -360.] {
        let e = Ellipse::circle(20.).arc(0., end);
        assert!(e.flatten(0.1)[0].closed);
        assert!(e.contains(Point::new(20., 20.)));
        let b = e.geometry_bounds().unwrap();
        near(b.min_x(), 0.);
        near(b.max_x(), 40.);
    }
    let empty = Ellipse::circle(20.).sector(45., 45.);
    assert!(empty.to_path().unwrap().commands.is_empty());
    assert!(empty.geometry_bounds().is_none());
    assert!(empty.flatten(0.1).is_empty());
    assert!(!empty.contains(Point::new(20., 20.)));
}
#[test]
fn unit_inner_ratio_can_express_an_open_arc_without_a_duplicate_inner_edge() {
    let e = Ellipse::circle(20.).arc(0., 90.).ring(1.);
    assert_eq!(e.flatten(0.1).len(), 1);
    assert!(!e.flatten(0.1)[0].closed);
    let closed = Ellipse::circle(20.).ring(1.);
    assert_eq!(closed.flatten(0.1).len(), 2);
    assert!(!closed.contains(Point::new(20., 20.)));
}
#[test]
fn new_fields_roundtrip_and_invalid_geometry_rejects() {
    let e = Ellipse::new(40., 20.).sector(-45., 180.).ring(0.25);
    assert_eq!(serde_json::from_value::<Ellipse>(json!(e)).unwrap(), e);
    for invalid in [
        Ellipse::new(0., 20.),
        Ellipse::new(f32::INFINITY, 20.),
        Ellipse::circle(20.).ring(-0.1),
        Ellipse::circle(20.).ring(1.1),
        Ellipse::circle(20.).ring(f32::NAN),
        Ellipse::circle(20.).arc(f32::NAN, 90.),
        Ellipse::circle(20.).arc(0., 361.),
        Ellipse::circle(20.).arc(1_000_001., 1_000_001.),
        Ellipse::new(1_000_001., 20.).ring(0.5),
    ] {
        assert!(invalid.validate().is_err());
        assert!(Shape::from(invalid).validate().is_err());
    }
}
#[test]
fn large_default_oval_compatibility_is_preserved_but_curve_conversion_is_bounded() {
    let e = Ellipse::new(2_000_000., 100.);
    assert!(e.validate().is_ok());
    assert!(e.to_path().is_err());
    assert!(e.flatten(0.1)[0].points.len() <= 4096);
}
#[test]
fn worst_case_ellipse_contours_have_a_fixed_point_budget() {
    let e = Ellipse::new(1_000_000., 1_000_000.).ring(0.5);
    let path = e.to_path().unwrap();
    assert!(path.commands.len() <= 64);
    let c = e.flatten(1e-12);
    assert!(c.iter().map(|c| c.points.len()).sum::<usize>() <= 8192);
}
#[test]
fn open_arc_and_closed_ring_support_alignment_without_chord_stroke() {
    let arc = Shape::from(Ellipse::circle(20.).arc(0., 90.));
    let ring = Shape::from(Ellipse::circle(20.).ring(0.5));
    for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
        let stroke = Stroke::new(Color::BLACK, 2.).with_alignment(alignment);
        let mesh = arc.stroke_mesh(&stroke).unwrap();
        assert!(!mesh.triangles().is_empty());
        assert!(
            !mesh.hit_test(Point::new(30., 30.), 0.),
            "implicit chord has no ink"
        );
        assert!(ring.stroke_mesh(&stroke).is_ok());
    }
}
