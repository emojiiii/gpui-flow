use gpui_flow_core::{Point, Rect, Size};
use gpui_flow_editor::{
    ResizeHandle, ResizeOptions, resize_transform, resize_transform_with_options,
};
use gpui_flow_scene::Affine;

fn bounds() -> Rect {
    Rect::new(Point::new(-40., -25.), Size::new(120., 50.))
}
fn options(handle: ResizeHandle, uniform: bool) -> ResizeOptions {
    ResizeOptions {
        handle,
        uniform,
        ..Default::default()
    }
}
fn close(a: Point, b: Point) {
    assert!(a.distance(b) < 0.002, "{a:?} != {b:?}");
}

#[test]
fn all_eight_handles_fix_the_opposite_anchor_under_nested_affines() {
    let b = bounds();
    let parent = Affine::translation(100., 40.) * Affine::rotation(33.) * Affine::scale(2., 0.6);
    let original = Affine::translation(30., 60.) * Affine::rotation(-18.) * Affine::skew(12., 0.);
    for handle in ResizeHandle::ALL {
        for uniform in [false, true] {
            let start = handle.position(b) + Point::new(2., -3.);
            let current = start + (handle.position(b) - handle.anchor(b)) * 0.3;
            let next = resize_transform_with_options(
                original,
                b,
                start,
                current,
                options(handle, uniform),
            )
            .unwrap();
            close(
                (parent * original).transform_point(handle.anchor(b)),
                (parent * next).transform_point(handle.anchor(b)),
            );
            close(
                next.transform_point(handle.position(b)),
                original.transform_point(
                    handle.position(b) + (handle.position(b) - handle.anchor(b)) * 0.3,
                ),
            );
        }
    }
}

#[test]
fn sides_change_one_axis_and_uniform_sides_keep_opposite_midpoint() {
    let b = bounds();
    for handle in [
        ResizeHandle::Top,
        ResizeHandle::Right,
        ResizeHandle::Bottom,
        ResizeHandle::Left,
    ] {
        let start = handle.position(b);
        let current = start + (start - handle.anchor(b)) * 0.5;
        for uniform in [false, true] {
            let next = resize_transform_with_options(
                Affine::IDENTITY,
                b,
                start,
                current,
                options(handle, uniform),
            )
            .unwrap();
            close(next.transform_point(handle.anchor(b)), handle.anchor(b));
            close(
                next.transform_point(b.center()),
                b.center() + (b.center() - handle.anchor(b)) * 0.5,
            );
            if uniform {
                assert_eq!((next.a, next.d), (1.5, 1.5));
            } else if matches!(handle, ResizeHandle::Top | ResizeHandle::Bottom) {
                assert_eq!((next.a, next.d), (1., 1.5));
            } else {
                assert_eq!((next.a, next.d), (1.5, 1.));
            }
        }
    }
}

#[test]
fn uniform_non_square_corner_projects_signed_single_axis_motion() {
    let b = Rect::new(Point::ZERO, Size::new(100., 50.));
    for (handle, delta) in [
        (ResizeHandle::BottomRight, Point::new(-50., 0.)),
        (ResizeHandle::TopLeft, Point::new(50., 0.)),
    ] {
        let start = handle.position(b);
        let next = resize_transform_with_options(
            Affine::IDENTITY,
            b,
            start,
            start + delta,
            options(handle, true),
        )
        .unwrap();
        assert!((next.a - 0.6).abs() < 0.00001);
        assert_eq!(next.a, next.d);
        close(next.transform_point(handle.anchor(b)), handle.anchor(b));
    }
}

#[test]
fn no_motion_and_tangential_side_motion_are_exact_noops() {
    let original = Affine::translation(35., 12.) * Affine::rotation(24.);
    for handle in ResizeHandle::ALL {
        let start = handle.position(bounds()) + Point::new(-2., 3.);
        assert_eq!(
            resize_transform_with_options(original, bounds(), start, start, options(handle, true)),
            Some(original)
        );
    }
    let start = ResizeHandle::Right.position(bounds());
    assert_eq!(
        resize_transform_with_options(
            original,
            bounds(),
            start,
            start + Point::new(0., 20.),
            options(ResizeHandle::Right, true)
        ),
        Some(original)
    );
}

#[test]
fn crossing_clamps_without_new_flips_and_uniform_uses_stricter_floor() {
    for handle in ResizeHandle::ALL {
        for uniform in [false, true] {
            let b = bounds();
            let start = handle.position(b);
            let next = resize_transform_with_options(
                Affine::scale(-2., 3.),
                b,
                start,
                start - (start - handle.anchor(b)) * 10.,
                ResizeOptions {
                    handle,
                    uniform,
                    minimum_scale: Point::new(0.1, 0.2),
                },
            )
            .unwrap();
            assert!(next.determinant() < 0.);
            if uniform {
                assert_eq!((next.a, next.d), (-0.4, 0.6));
            }
            close(
                next.transform_point(handle.anchor(b)),
                Affine::scale(-2., 3.).transform_point(handle.anchor(b)),
            );
        }
    }
}

#[test]
fn invalid_degenerate_and_overflowing_inputs_are_rejected() {
    let b = bounds();
    let opts = options(ResizeHandle::BottomRight, false);
    for bad in [
        Rect::new(Point::ZERO, Size::new(0., 30.)),
        Rect::new(Point::ZERO, Size::new(10., f32::NAN)),
    ] {
        assert!(
            resize_transform_with_options(Affine::IDENTITY, bad, Point::ZERO, Point::ZERO, opts)
                .is_none()
        );
    }
    assert!(
        resize_transform_with_options(Affine::scale(0., 1.), b, Point::ZERO, Point::ZERO, opts)
            .is_none()
    );
    assert!(
        resize_transform_with_options(
            Affine::IDENTITY,
            b,
            Point::ZERO,
            Point::new(f32::INFINITY, 1.),
            opts
        )
        .is_none()
    );
    assert!(
        resize_transform_with_options(
            Affine::scale(f32::MAX, 1.),
            b,
            Point::ZERO,
            Point::new(120., 50.),
            opts
        )
        .is_none()
    );
}

#[test]
fn legacy_bottom_right_entry_point_keeps_its_original_contract() {
    let b = bounds();
    let start = ResizeHandle::BottomRight.position(b);
    let legacy = resize_transform(Affine::IDENTITY, b, start, start + Point::new(60., 25.));
    assert_eq!((legacy.a, legacy.d), (1.5, 1.5));
    close(legacy.transform_point(b.origin), b.origin);
}
