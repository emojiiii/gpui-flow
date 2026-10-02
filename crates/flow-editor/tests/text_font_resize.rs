use gpui_flow_editor::{ResizeHandle, ResizeOptions, TextFontResizeGeometry};
use gpui_flow_scene::{Affine, Bounds, Point, Size, display::Text};
fn text() -> Text {
    let mut t = Text::new("same content", Size::new(200., 100.));
    t.font_size = 20.;
    t.padding = [5., 10., 15., 20.];
    t
}
fn near(a: Point, b: Point) {
    assert!(
        (a.x - b.x).abs() < 0.001 && (a.y - b.y).abs() < 0.001,
        "{a:?} {b:?}"
    );
}
#[test]
fn font_resize_all_handles_preserve_opposite_anchor_and_affine_linear_terms() {
    let t = text();
    let g = TextFontResizeGeometry::new(&t).unwrap();
    for handle in ResizeHandle::ALL {
        for original in [
            Affine::IDENTITY,
            Affine::translation(11., -5.) * Affine::rotation(25.) * Affine::scale(-1.2, 0.7),
        ] {
            let options = ResizeOptions {
                handle,
                ..Default::default()
            };
            let start = handle.position(Bounds::new(Point::ZERO, t.size));
            let current = Point::new(start.x + 20., start.y + 10.);
            let result = g.resize(original, start, current, options).unwrap();
            near(
                original.transform_point(handle.anchor(Bounds::new(Point::ZERO, t.size))),
                result
                    .transform
                    .transform_point(handle.anchor(Bounds::new(Point::ZERO, result.size))),
            );
            assert_eq!(
                [original.a, original.b, original.c, original.d],
                [
                    result.transform.a,
                    result.transform.b,
                    result.transform.c,
                    result.transform.d
                ]
            );
            let scale = result.font_size / t.font_size;
            assert_eq!(result.padding, t.padding.map(|v| v * scale));
            assert_eq!(
                result.size,
                Size::new(t.size.width * scale, t.size.height * scale)
            );
        }
    }
}
#[test]
fn font_resize_uses_y_for_vertical_sides_and_x_for_corners_regardless_of_shift() {
    let t = text();
    let g = TextFontResizeGeometry::new(&t).unwrap();
    for uniform in [false, true] {
        let bottom = g
            .resize(
                Affine::IDENTITY,
                Point::ZERO,
                Point::new(80., 50.),
                ResizeOptions {
                    handle: ResizeHandle::Bottom,
                    uniform,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(bottom.font_size, 30.);
        assert_eq!(bottom.size, Size::new(300., 150.));
        let corner = g
            .resize(
                Affine::IDENTITY,
                Point::ZERO,
                Point::new(40., 90.),
                ResizeOptions {
                    handle: ResizeHandle::BottomRight,
                    uniform,
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(corner.font_size, 24.);
        near(
            Point::new(corner.size.width, corner.size.height),
            Point::new(240., 120.),
        );
    }
}
#[test]
fn font_resize_noop_is_exact_and_invalid_geometry_or_budget_rejects() {
    let t = text();
    let g = TextFontResizeGeometry::new(&t).unwrap();
    let pose = Affine::rotation(17.) * Affine::translation(5., -3.);
    let r = g
        .resize(
            pose,
            Point::new(1., 2.),
            Point::new(1., 2.),
            ResizeOptions::default(),
        )
        .unwrap();
    assert_eq!(r.transform, pose);
    assert_eq!(r.size, t.size);
    assert_eq!(r.padding, t.padding);
    assert_eq!(r.font_size, t.font_size);
    assert!(
        g.resize(
            Affine::scale(0., 1.),
            Point::ZERO,
            Point::ZERO,
            ResizeOptions::default()
        )
        .is_none()
    );
    assert!(
        g.resize(
            pose,
            Point::ZERO,
            Point::new(f32::NAN, 0.),
            ResizeOptions::default()
        )
        .is_none()
    );
    assert!(
        g.resize(
            pose,
            Point::ZERO,
            Point::new(20000., 0.),
            ResizeOptions::default()
        )
        .is_none()
    );
}
