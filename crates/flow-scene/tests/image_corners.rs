use gpui_flow_scene::{
    Affine, Point, Shape, Size, Style,
    display::{Image, ImageFit},
};
use serde_json::json;
#[test]
fn legacy_image_json_and_corner_builder_precedence_remain_explicit() {
    let old = json!({"source":"local.png","size":{"width":40.,"height":40.},"natural_size":null,"fit":"contain"});
    let image: Image = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(json!(image), old);
    let image = image.rounded(8.).with_corners([30., 0., 10., 0.]);
    assert_eq!(image.effective_corners().0, [20., 0., 10., 0.]);
    assert_eq!(
        serde_json::from_value::<Image>(json!(image)).unwrap(),
        image
    );
    let image = image.rounded(0.);
    assert!(image.corner_radii.is_none());
    assert!(!image.is_rounded());
    assert_eq!(json!(image), old);
}
#[test]
fn rounded_image_geometry_and_painted_fit_hit_use_the_actual_outer_shape() {
    let mut image = Image::new("x", Size::new(40., 40.)).with_corners([20., 0., 15., 0.]);
    image.fit = ImageFit::Contain;
    image.natural_size = Some(Size::new(80., 40.));
    assert!(!image.contains(Point::new(1., 1.)));
    assert!(image.contains(Point::new(39., 1.)));
    let shape = Shape::from(image);
    assert!(!shape.hit_test(Point::new(1., 11.), &Style::default(), 0.));
    assert!(shape.hit_test(Point::new(10., 11.), &Style::default(), 0.));
    assert!(!shape.hit_test(Point::new(20., 5.), &Style::default(), 0.));
    assert_eq!(shape.local_bounds().unwrap().size, Size::new(40., 40.));
}
#[test]
fn invalid_image_corners_reject_instead_of_becoming_rectangular() {
    for corners in [
        [-1., 0., 0., 0.],
        [f32::NAN, 0., 0., 0.],
        [f32::INFINITY, 0., 0., 0.],
    ] {
        let image = Image::new("x", Size::new(40., 40.)).with_corners(corners);
        assert!(image.validate().is_err());
        assert!(Shape::from(image).validate().is_err());
    }
    let image = Image::new("x", Size::new(40., 40.))
        .rounded(10.)
        .with_corners([0.; 4]);
    assert!(!image.is_rounded());
}
#[test]
fn positive_uniform_transform_predicate_excludes_rotation_reflection_and_skew() {
    assert!(Affine::translation(10., 20.).is_positive_uniform_scale_translation());
    assert!(Affine::scale(2., 2.).is_positive_uniform_scale_translation());
    for m in [
        Affine::rotation(30.),
        Affine::scale(-1., 1.),
        Affine::scale(2., 1.),
        Affine::skew(5., 0.),
        Affine::scale(0., 0.),
        Affine::translation(f32::NAN, 0.),
    ] {
        assert!(!m.is_positive_uniform_scale_translation());
    }
}
