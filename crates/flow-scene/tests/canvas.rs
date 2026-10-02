use gpui_flow_scene::{
    Color, DisplayObject, HitPolicy, HitTestOptions, Point, Scene, Shape, Stroke, Style,
    display::Canvas,
};
#[test]
fn canvas_defaults_and_factory_friendly_json_do_not_embed_pixels() {
    let c = Canvas::default();
    assert_eq!(c.pixel_size().unwrap(), [100, 100]);
    assert!(c.smooth);
    assert!(!c.safe_resize);
    let value = serde_json::to_value(Shape::from(c.clone())).unwrap();
    assert_eq!(value["type"], "canvas");
    assert!(value["geometry"].get("pixels").is_none());
    assert_eq!(
        serde_json::from_value::<Shape>(value).unwrap(),
        Shape::Canvas(c)
    );
}
#[test]
fn invalid_ratios_dimensions_and_pixel_budgets_reject_before_allocation() {
    for (w, h, r) in [
        (0., 1., 1.),
        (1., 1., 0.),
        (1., 1., f32::NAN),
        (1., 1., f32::INFINITY),
        (4097., 1., 1.),
        (4096., 4096., 1.),
        (1., 1., 9.),
        (1e-30, 1., 1.),
    ] {
        let mut c = Canvas::new(w, h);
        c.pixel_ratio = r;
        assert!(c.validate().is_err(), "{w} {h} {r}");
    }
    let mut c = Canvas::new(10.25, 3.);
    c.pixel_ratio = 1.5;
    assert_eq!(c.pixel_size().unwrap(), [16, 5]);
}
#[test]
fn canvas_has_real_rounded_bounds_picking_and_visible_stroke_geometry() {
    let c = Canvas::new(20., 20.).with_corners([8., 0., 0., 0.]);
    assert!(!c.contains(Point::new(1., 1.)));
    assert!(c.contains(Point::new(19., 1.)));
    let shape = Shape::from(c);
    assert!(!shape.allows_children());
    let style = Style {
        fill: None,
        stroke: Some(Stroke::new(Color::WHITE, 4.)),
        ..Default::default()
    };
    assert!(shape.hit_test(Point::new(10., 10.), &style, 0.));
    assert!(shape.hit_test(Point::new(21., 10.), &style, 0.));
    let mut scene = Scene::new();
    let mut object = DisplayObject::new("c", shape).with_style(style);
    object.hit_policy = HitPolicy::Painted;
    scene.insert(object, None, None).unwrap();
    assert!(
        scene
            .hit_test(Point::new(10., 10.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    assert!(
        scene
            .insert(
                DisplayObject::new("child", Canvas::default()),
                Some("c".into()),
                None
            )
            .is_err()
    );
}
