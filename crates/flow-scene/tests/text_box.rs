use gpui_flow_scene::{
    Bounds, Point, Shape, Size,
    display::{Text, TextVerticalAlign},
};
#[test]
fn legacy_text_json_and_zero_padding_are_unchanged() {
    let t = Text::new("legacy", Size::new(100., 60.));
    let value = serde_json::to_value(&t).unwrap();
    assert!(value.get("padding").is_none());
    assert!(value.get("vertical_align").is_none());
    assert_eq!(serde_json::from_value::<Text>(value).unwrap(), t);
    assert_eq!(t.content_bounds(), t.bounds());
    for align in [
        TextVerticalAlign::Top,
        TextVerticalAlign::Middle,
        TextVerticalAlign::Bottom,
    ] {
        let mut t = t.clone();
        t.padding = [3., 5., 7., 11.];
        t.vertical_align = align;
        assert_eq!(
            serde_json::from_value::<Text>(serde_json::to_value(&t).unwrap()).unwrap(),
            t
        );
    }
}
#[test]
fn four_sided_padding_has_real_inner_box_without_changing_outer_geometry() {
    let mut t = Text::new("padded", Size::new(100., 60.));
    t.padding = [3., 5., 7., 11.];
    t.validate().unwrap();
    assert_eq!(
        t.content_bounds(),
        Bounds::new(Point::new(11., 3.), Size::new(84., 50.))
    );
    assert_eq!(
        Shape::from(t).local_bounds().unwrap().size,
        Size::new(100., 60.)
    );
}
#[test]
fn invalid_or_overconstrained_padding_rejects_in_component_and_shape_validation() {
    for padding in [
        [-1., 0., 0., 0.],
        [f32::NAN, 0., 0., 0.],
        [0., f32::INFINITY, 0., 0.],
        [1_000_001., 0., 0., 0.],
        [30., 0., 30., 0.],
        [0., 50., 0., 50.],
    ] {
        let mut t = Text::new("bad", Size::new(100., 60.));
        t.padding = padding;
        assert!(t.validate().is_err());
        assert!(Shape::from(t).validate().is_err());
    }
    let mut t = Text::new("font", Size::new(100., 60.));
    t.font_size = f32::NAN;
    assert!(t.validate().is_err());
}
