use gpui_flow_scene::{
    Color, Point, Shape, Stroke, StrokeAlignment,
    display::{CornerRadii, Rect},
};
use serde_json::json;
#[test]
fn old_scalar_json_and_serialization_remain_unchanged() {
    let old = json!({"size":{"width":60.,"height":40.},"corner_radius":7.});
    let rect: Rect = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(rect, Rect::new(60., 40.).rounded(7.));
    assert_eq!(json!(rect), old);
    assert_eq!(rect.corner_radii, None);
    assert_eq!(
        rect.flatten(0.1),
        Rect::new(60., 40.).rounded(7.).flatten(0.1)
    );
}
#[test]
fn four_corners_are_clockwise_and_independent() {
    let corners = [
        Point::new(1., 1.),
        Point::new(59., 1.),
        Point::new(59., 39.),
        Point::new(1., 39.),
    ];
    for rounded in 0..4 {
        let mut radii = [0.; 4];
        radii[rounded] = 15.;
        let rect = Rect::new(60., 40.).with_corners(radii);
        for (index, point) in corners.into_iter().enumerate() {
            assert_eq!(
                rect.contains(point),
                index != rounded,
                "corner {rounded}, point {index}"
            );
        }
        assert!(rect.contains(Point::new(30., 20.)));
        assert!(
            !Shape::from(rect.clone())
                .contains_geometry(corners[rounded], gpui_flow_scene::FillRule::NonZero)
        );
    }
}
#[test]
fn each_radius_clamps_to_half_shortest_side_like_pinned_fallback() {
    let big = Rect::new(40., 20.).with_corners([100., 0., 50., 0.]);
    let clamped = Rect::new(40., 20.).with_corners([10., 0., 10., 0.]);
    assert_eq!(big.to_path().unwrap(), clamped.to_path().unwrap());
    assert_eq!(big.flatten(0.1), clamped.flatten(0.1));
    assert!(!big.contains(Point::new(1., 1.)));
    assert!(big.contains(Point::new(15., 1.)));
}
#[test]
fn optional_uniform_corners_and_scalar_have_same_analytic_hits() {
    let scalar = Rect::new(60., 40.).rounded(9.);
    let corners = Rect::new(60., 40.).with_corners([9.; 4]);
    for x in 0..=60 {
        for y in 0..=40 {
            let p = Point::new(x as f32, y as f32);
            assert_eq!(scalar.contains(p), corners.contains(p), "{p:?}");
        }
    }
    assert_eq!(scalar.bounds(), corners.bounds());
}
#[test]
fn builder_precedence_is_explicit_and_optional_data_roundtrips() {
    let rect = Rect::new(60., 40.)
        .rounded(5.)
        .with_corners(CornerRadii::new(1., 2., 3., 4.));
    assert_eq!(rect.corner_radius, 5.);
    assert_eq!(json!(rect)["corner_radii"], json!([1., 2., 3., 4.]));
    assert_eq!(serde_json::from_value::<Rect>(json!(rect)).unwrap(), rect);
    let scalar = rect.rounded(8.);
    assert_eq!(scalar.corner_radii, None);
    assert_eq!(scalar.corner_radius, 8.);
}
#[test]
fn invalid_radius_or_size_rejects_instead_of_abs_or_partial_clamp() {
    for rect in [
        Rect::new(60., 40.).with_corners([-1., 0., 0., 0.]),
        Rect::new(60., 40.).with_corners([f32::NAN, 0., 0., 0.]),
        Rect::new(60., 40.).with_corners([0., f32::INFINITY, 0., 0.]),
        Rect::new(0., 40.).with_corners([1.; 4]),
        Rect::new(1_000_001., 40.).with_corners([1.; 4]),
    ] {
        assert!(rect.validate().is_err());
        assert!(rect.to_path().is_err());
        assert!(Shape::from(rect).validate().is_err());
    }
    assert!(CornerRadii::uniform(-1.).validate().is_err());
}
#[test]
fn legacy_large_rectangles_remain_valid_but_new_conversion_is_bounded() {
    let rect = Rect::new(2_000_000., 100.).rounded(5.);
    assert!(rect.validate().is_ok());
    assert!(Shape::from(rect.clone()).validate().is_ok());
    assert!(rect.to_path().is_err());
}
#[test]
fn path_contour_is_closed_bounded_and_keeps_real_component_bounds() {
    let rect = Rect::new(1_000_000., 1_000_000.).with_corners([500_000., 0., 100_000., 300_000.]);
    let path = rect.to_path().unwrap();
    assert!(path.commands.len() <= 64);
    let contour = rect.flatten(1e-12);
    assert!(contour.closed);
    assert!(contour.points.len() <= 8192);
    for p in contour.points {
        assert!(p.x >= -0.05 && p.y >= -0.05 && p.x <= 1_000_001. && p.y <= 1_000_001.);
    }
    assert_eq!(
        Shape::from(rect.clone()).local_bounds(),
        Some(rect.bounds())
    );
}
#[test]
fn sharp_optional_corners_still_support_advanced_aligned_strokes() {
    for radii in [[0.; 4], [15., 0., 8., 3.]] {
        let shape = Shape::from(Rect::new(60., 40.).with_corners(radii));
        for alignment in [
            StrokeAlignment::Center,
            StrokeAlignment::Inside,
            StrokeAlignment::Outside,
        ] {
            let stroke = Stroke::new(Color::BLACK, 2.).with_alignment(alignment);
            assert!(!shape.stroke_mesh(&stroke).unwrap().triangles().is_empty());
        }
    }
}
