use gpui_flow_editor::{
    MAX_TEXT_BOX_DIMENSION, ResizeHandle, ResizeOptions, TextBoxResizeGeometry,
};
use gpui_flow_scene::{Affine, Bounds, Point, Size, display::Text};
fn text() -> Text {
    let mut t = Text::new("fixed font", Size::new(120., 60.));
    t.padding = [4., 8., 6., 7.];
    t
}
#[test]
fn text_box_all_handles_fix_opposite_anchor_without_scaling_linear_terms() {
    let t = text();
    let g = TextBoxResizeGeometry::new(&t).unwrap();
    let parent = Affine::translation(30., 40.) * Affine::rotation(-19.) * Affine::scale(0.8, 1.2);
    for original in [
        Affine::translation(20., 30.) * Affine::rotation(35.),
        Affine::translation(20., 30.) * Affine::scale(-1., 1.),
        Affine::skew(20., 10.),
    ] {
        for h in ResizeHandle::ALL {
            for uniform in [false, true] {
                let b = t.bounds();
                let start = h.position(b) + Point::new(3., -2.);
                let current = start + (h.position(b) - h.anchor(b)) * 0.4;
                let n = g
                    .resize(
                        original,
                        start,
                        current,
                        ResizeOptions {
                            handle: h,
                            uniform,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                assert_eq!(
                    [n.transform.a, n.transform.b, n.transform.c, n.transform.d],
                    [original.a, original.b, original.c, original.d]
                );
                let anchor = h.anchor(Bounds::new(Point::ZERO, n.size));
                assert!(
                    (parent * n.transform)
                        .transform_point(anchor)
                        .distance((parent * original).transform_point(h.anchor(b)))
                        < 0.004
                );
            }
        }
    }
}
#[test]
fn text_box_side_resize_reflows_width_without_changing_height_unless_shift() {
    let t = text();
    let g = TextBoxResizeGeometry::new(&t).unwrap();
    let p = ResizeHandle::Right.position(t.bounds());
    for uniform in [false, true] {
        let n = g
            .resize(
                Affine::IDENTITY,
                p,
                p + Point::new(60., 0.),
                ResizeOptions {
                    handle: ResizeHandle::Right,
                    uniform,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(n.size, Size::new(180., if uniform { 90. } else { 60. }));
        assert_eq!(n.transform.a, 1.);
        assert_eq!(n.transform.d, 1.);
    }
}
#[test]
fn text_box_padding_minimum_and_already_tiny_content_remain_valid() {
    let t = text();
    let g = TextBoxResizeGeometry::new(&t).unwrap();
    let p = ResizeHandle::BottomRight.position(t.bounds());
    let n = g
        .resize(
            Affine::IDENTITY,
            p,
            p - Point::new(1000., 1000.),
            ResizeOptions {
                handle: ResizeHandle::BottomRight,
                ..Default::default()
            },
        )
        .unwrap();
    assert!((n.size.width - 16.).abs() < 0.0001);
    assert!((n.size.height - 11.).abs() < 0.0001);
    let mut tiny = Text::new("", Size::new(10., 10.));
    tiny.padding = [4.75; 4];
    let g = TextBoxResizeGeometry::new(&tiny).unwrap();
    let n = g
        .resize(
            Affine::IDENTITY,
            Point::new(10., 10.),
            Point::ZERO,
            ResizeOptions::default(),
        )
        .unwrap();
    assert_eq!(n.size, tiny.size);
}
#[test]
fn text_box_noop_is_exact_and_invalid_geometry_or_budgets_reject() {
    let t = text();
    let g = TextBoxResizeGeometry::new(&t).unwrap();
    let m = Affine::translation(7., -3.) * Affine::rotation(13.);
    let p = Point::new(120., 60.);
    let n = g.resize(m, p, p, ResizeOptions::default()).unwrap();
    assert_eq!(n.transform, m);
    assert_eq!(n.size, t.size);
    assert!(
        g.resize(Affine::scale(0., 1.), p, p, ResizeOptions::default())
            .is_none()
    );
    assert!(
        g.resize(m, p, Point::new(f32::NAN, 1.), ResizeOptions::default())
            .is_none()
    );
    assert!(
        g.resize(
            m,
            p,
            p,
            ResizeOptions {
                minimum_scale: Point::new(f32::NAN, 0.1),
                ..Default::default()
            }
        )
        .is_none()
    );
    let huge = Text::new("", Size::new(MAX_TEXT_BOX_DIMENSION, 20.));
    let g = TextBoxResizeGeometry::new(&huge).unwrap();
    assert!(
        g.resize(
            Affine::IDENTITY,
            Point::ZERO,
            Point::new(MAX_TEXT_BOX_DIMENSION, 0.),
            ResizeOptions {
                handle: ResizeHandle::Right,
                ..Default::default()
            }
        )
        .is_none()
    );
    let mut invalid = t;
    invalid.padding = [1000.; 4];
    assert!(TextBoxResizeGeometry::new(&invalid).is_err());
}
