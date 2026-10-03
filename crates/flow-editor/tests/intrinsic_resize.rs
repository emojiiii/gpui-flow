use gpui_flow_editor::{BuiltinResizeGeometry, BuiltinResizePolicy, ResizeHandle, ResizeOptions};
use gpui_flow_scene::{Affine, Bounds, DisplayObject, Point, Scene, Shape, Size, display};

fn scene(shape: impl Into<Shape>) -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("s", shape), None, None)
        .unwrap();
    scene
}
fn resized(scene: &Scene, handle: ResizeHandle, delta: Point) -> DisplayObject {
    let node = scene.get(&"s".into()).unwrap();
    let g = BuiltinResizeGeometry::new(scene, &"s".into()).unwrap();
    let start = handle.position(g.bounds());
    g.resize(
        node.object.transform,
        start,
        start + delta,
        ResizeOptions {
            handle,
            ..Default::default()
        },
    )
    .unwrap()
    .apply_to(&node.object)
    .unwrap()
}
fn nominal(shape: &Shape) -> Bounds {
    if let Shape::Ellipse(s) = shape {
        s.bounds()
    } else {
        shape.local_bounds().unwrap()
    }
}
#[test]
fn size_policies_preserve_all_eight_anchors_and_affine_linear_terms() {
    let kinds: Vec<Shape> = vec![
        display::Rect::new(120., 60.).rounded(12.).into(),
        display::Ellipse::new(120., 60.)
            .sector(15., 100.)
            .ring(0.4)
            .into(),
        display::Image::new("local-image", Size::new(120., 60.)).into(),
        display::Canvas::new(120., 60.).into(),
        display::Box::new(120., 60.)
            .rounded(9.)
            .clipped(true)
            .into(),
        display::Frame::new(120., 60.).rounded(9.).into(),
    ];
    for shape in kinds {
        for handle in ResizeHandle::ALL {
            let mut scene = scene(shape.clone());
            let original =
                Affine::translation(100., -20.) * Affine::rotation(35.) * Affine::scale(-1.3, 0.7);
            scene
                .update_object(&"s".into(), |o| o.transform = original)
                .unwrap();
            let bounds = nominal(&shape);
            let result = resized(
                &scene,
                handle,
                (handle.position(bounds) - handle.anchor(bounds)) * 0.25,
            );
            let after = nominal(&result.shape);
            assert!(
                original
                    .transform_point(handle.anchor(bounds))
                    .distance(result.transform.transform_point(handle.anchor(after)))
                    < 0.001,
                "{} {handle:?}",
                shape.tag()
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
            assert_eq!(result.style, scene.get(&"s".into()).unwrap().object.style);
        }
    }
}
#[test]
fn coordinate_policies_keep_nonzero_origin_and_actual_curve_controls() {
    let path = display::Path::new(vec![
        display::PathCommand::MoveTo {
            to: Point::new(-20., 10.),
        },
        display::PathCommand::CubicTo {
            control1: Point::new(0., 90.),
            control2: Point::new(80., -30.),
            to: Point::new(100., 70.),
        },
        display::PathCommand::Close,
    ]);
    let kinds: Vec<Shape> = vec![
        display::Line::new(Point::new(-20., 10.), Point::new(100., 70.)).into(),
        display::Polygon::polyline(vec![
            Point::new(-20., 10.),
            Point::new(10., 70.),
            Point::new(100., 40.),
        ])
        .into(),
        path.clone().into(),
        display::Pen { path }.into(),
    ];
    for shape in kinds {
        let scene = scene(shape.clone());
        let old = shape.local_bounds().unwrap();
        let result = resized(&scene, ResizeHandle::TopLeft, Point::new(-60., -30.));
        let new = result.shape.local_bounds().unwrap();
        assert!((new.size.width - (old.size.width + 60.)).abs() < 0.001);
        assert!((new.size.height - (old.size.height + 30.)).abs() < 0.001);
        assert!(
            result
                .transform
                .transform_point(Point::new(new.max_x(), new.max_y()))
                .distance(Point::new(old.max_x(), old.max_y()))
                < 0.001
        );
        assert_eq!(shape.tag(), result.shape.tag());
        if let Shape::Path(path) = &result.shape {
            assert_eq!(path.commands.len(), 3);
            assert!(matches!(
                path.commands[1],
                display::PathCommand::CubicTo { .. }
            ));
        }
        if let Shape::Polygon(p) = &result.shape {
            assert!(!p.closed);
        }
    }
}
#[test]
fn star_is_uniform_and_preserves_radial_parameters() {
    let scene = scene(display::Star::new(7, 50., 18.));
    let g = BuiltinResizeGeometry::new(&scene, &"s".into()).unwrap();
    assert_eq!(g.policy(), BuiltinResizePolicy::Radial);
    let old = match &scene.get(&"s".into()).unwrap().object.shape {
        Shape::Star(s) => s,
        _ => unreachable!(),
    };
    let object = resized(
        &scene,
        ResizeHandle::Right,
        Point::new(g.bounds().size.width, 99.),
    );
    let Shape::Star(new) = object.shape else {
        panic!()
    };
    assert_eq!(new.outer_radius, 100.);
    assert_eq!(new.inner_radius, 36.);
    assert_eq!((new.points, new.start_angle), (old.points, old.start_angle));
}
#[test]
fn patch_keeps_current_resource_and_configuration_fields() {
    for shape in [
        Shape::Image(display::Image::new("old", Size::new(120., 60.))),
        Shape::Canvas(display::Canvas::new(120., 60.)),
    ] {
        let mut scene = scene(shape);
        let g = BuiltinResizeGeometry::new(&scene, &"s".into()).unwrap();
        let patch = g
            .resize(
                Affine::IDENTITY,
                Point::ZERO,
                Point::new(60., 30.),
                ResizeOptions::default(),
            )
            .unwrap();
        scene
            .update_object(&"s".into(), |o| match &mut o.shape {
                Shape::Image(i) => {
                    i.source = "current".into();
                    i.fit = display::ImageFit::Cover;
                    i.source_crop = Some(Bounds::new(Point::new(0.1, 0.2), Size::new(0.5, 0.6)));
                }
                Shape::Canvas(c) => {
                    c.pixel_ratio = 2.;
                    c.safe_resize = true;
                    c.smooth = false;
                }
                _ => unreachable!(),
            })
            .unwrap();
        let result = patch
            .apply_to(&scene.get(&"s".into()).unwrap().object)
            .unwrap();
        match result.shape {
            Shape::Image(i) => {
                assert_eq!(i.source, "current");
                assert_eq!(i.fit, display::ImageFit::Cover);
                assert!(i.source_crop.is_some());
            }
            Shape::Canvas(c) => {
                assert_eq!(c.pixel_ratio, 2.);
                assert!(c.safe_resize);
                assert!(!c.smooth);
            }
            _ => unreachable!(),
        }
    }
}
#[test]
fn box_changes_own_size_and_group_scales_children_without_changing_own_matrix() {
    for shape in [
        Shape::Group(display::Group::new()),
        Shape::Box(display::Box::new(120., 60.)),
    ] {
        let mut scene = scene(shape);
        let child = DisplayObject::new("child", display::Rect::new(30., 20.))
            .with_transform(Affine::translation(10., 15.));
        scene.insert(child.clone(), Some("s".into()), None).unwrap();
        let g = BuiltinResizeGeometry::new(&scene, &"s".into()).unwrap();
        let patch = g
            .resize(
                Affine::IDENTITY,
                Point::ZERO,
                Point::new(g.bounds().size.width, g.bounds().size.height),
                ResizeOptions::default(),
            )
            .unwrap();
        if g.policy() == BuiltinResizePolicy::Children {
            assert_eq!(patch.transform, Affine::IDENTITY);
            assert_eq!(patch.child_transforms().len(), 1);
            assert_eq!(
                patch.child_transforms()[0].1,
                Affine::translation(10., 15.) * Affine::scale(2., 2.)
            );
        } else {
            assert!(patch.child_transforms().is_empty());
        }
        assert_eq!(scene.get(&"child".into()).unwrap().object, child);
    }
}
#[test]
fn budget_failure_degenerate_geometry_and_text_never_fall_back_to_intrinsic() {
    let canvas = scene(display::Canvas::new(1024., 1024.));
    let g = BuiltinResizeGeometry::new(&canvas, &"s".into()).unwrap();
    assert!(
        g.resize(
            Affine::IDENTITY,
            Point::ZERO,
            Point::new(4096., 4096.),
            ResizeOptions::default()
        )
        .is_none()
    );
    assert!(
        g.resize(
            Affine::IDENTITY,
            Point::ZERO,
            Point::new(f32::NAN, 1.),
            ResizeOptions::default()
        )
        .is_none()
    );
    for shape in [
        Shape::Line(display::Line::new(Point::ZERO, Point::new(10., 0.))),
        Shape::Text(display::Text::new("text", Size::new(120., 60.))),
    ] {
        assert!(BuiltinResizeGeometry::new(&scene(shape), &"s".into()).is_err());
    }
    let mut group = scene(display::Group::new());
    group
        .insert(
            DisplayObject::new("child", display::Rect::new(800_000., 800_000.)),
            Some("s".into()),
            None,
        )
        .unwrap();
    let g = BuiltinResizeGeometry::new(&group, &"s".into()).unwrap();
    assert!(
        g.resize(
            Affine::IDENTITY,
            Point::ZERO,
            Point::new(800_000., 800_000.),
            ResizeOptions::default()
        )
        .is_none()
    );
}
