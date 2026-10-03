use gpui_flow_editor::{PathControlGeometry, PathPointId, PathPointRole, SkewAxis, skew_transform};
use gpui_flow_scene::{
    Affine, Point, Shape,
    display::{Path, PathCommand, Pen},
};
fn close(a: Point, b: Point) {
    assert!(
        (a.x - b.x).abs() < 0.0001 && (a.y - b.y).abs() < 0.0001,
        "{a:?} != {b:?}"
    );
}
#[test]
fn one_quadratic_control_moves_in_frozen_affine_space_without_rewriting_other_points() {
    let shape: Shape = Path::from_svg("M0 0 Q20 40 40 0 C45 -10 55 -10 60 0 Z")
        .unwrap()
        .into();
    let world = Affine::translation(80., 90.) * Affine::rotation(30.) * Affine::scale(-2., 1.5);
    let geometry = PathControlGeometry::new(&shape, world).unwrap();
    let id = PathPointId {
        command: 1,
        role: PathPointRole::Control1,
    };
    let result = geometry
        .drag(
            id,
            world.transform_point(Point::new(23., 43.)),
            world.transform_point(Point::new(23., 63.)),
            false,
        )
        .unwrap();
    let Shape::Path(before) = shape else {
        unreachable!()
    };
    let Shape::Path(after) = result else {
        panic!("kind changed")
    };
    assert_eq!(before.commands.len(), after.commands.len());
    assert_eq!(before.commands[0], after.commands[0]);
    assert_eq!(before.commands[2..], after.commands[2..]);
    let PathCommand::QuadraticTo { control, to } = after.commands[1] else {
        unreachable!()
    };
    close(control, Point::new(20., 60.));
    assert_eq!(to, Point::new(40., 0.));
}
#[test]
fn leaf_pen_identity_and_exact_no_motion_are_preserved() {
    let shape: Shape = Pen::new()
        .move_to(Point::new(5., 7.))
        .line_to(Point::new(25., 17.))
        .into();
    let geometry = PathControlGeometry::new(&shape, Affine::IDENTITY).unwrap();
    let id = PathPointId {
        command: 1,
        role: PathPointRole::Endpoint,
    };
    assert_eq!(
        geometry
            .drag(id, Point::new(24., 16.), Point::new(24., 16.), false)
            .unwrap(),
        shape
    );
    let next = geometry
        .drag(id, Point::new(24., 16.), Point::new(34., 36.), true)
        .unwrap();
    let Shape::Pen(p) = next else {
        panic!("leaf Pen changed")
    };
    assert_eq!(
        p.path.commands[0],
        PathCommand::MoveTo {
            to: Point::new(5., 7.)
        }
    );
    assert_eq!(
        p.path.commands[1],
        PathCommand::LineTo {
            to: Point::new(25., 37.)
        }
    );
}
#[test]
fn invalid_point_roles_nonfinite_coordinates_and_over_budget_paths_are_rejected() {
    let shape: Shape = Path::from_svg("M0 0Q10 20 30 0").unwrap().into();
    let g = PathControlGeometry::new(&shape, Affine::IDENTITY).unwrap();
    for id in [
        PathPointId {
            command: 1,
            role: PathPointRole::Control2,
        },
        PathPointId {
            command: 99,
            role: PathPointRole::Endpoint,
        },
    ] {
        assert!(g.drag(id, Point::ZERO, Point::new(1., 1.), false).is_err());
    }
    assert!(
        g.drag(
            PathPointId {
                command: 1,
                role: PathPointRole::Control1
            },
            Point::ZERO,
            Point::new(f32::NAN, 1.),
            false
        )
        .is_err()
    );
    assert!(PathControlGeometry::new(&shape, Affine::scale(0., 1.)).is_err());
    let big = Path {
        commands: vec![PathCommand::MoveTo { to: Point::ZERO }; 513],
    };
    assert!(PathControlGeometry::new(&big.into(), Affine::IDENTITY).is_err());
}
#[test]
fn shear_keeps_explicit_pivot_and_uses_expected_local_geometry() {
    let original = Affine::translation(30., 40.) * Affine::rotation(30.) * Affine::scale(-2., 1.5);
    let pivot = Point::new(25., 20.);
    let result = skew_transform(
        original,
        pivot,
        Point::new(25., -20.),
        Point::new(15., 0.),
        SkewAxis::X,
    )
    .unwrap();
    close(
        result.transform_point(pivot),
        original.transform_point(pivot),
    );
    close(
        result.transform_point(Point::new(50., 0.)),
        original.transform_point(Point::new(57.5, 0.)),
    );
    assert!(skew_transform(original, pivot, pivot, Point::new(1., 0.), SkewAxis::X).is_none());
    assert!(
        skew_transform(
            original,
            pivot,
            Point::new(25., 19.),
            Point::new(17., 0.),
            SkewAxis::X
        )
        .is_none()
    );
}

#[test]
fn every_stored_m_l_q_c_coordinate_is_independently_editable() {
    let shape: Shape = Path::from_svg("M1 2 L11 12 Q21 22 31 32 C41 42 51 52 61 62 Z")
        .unwrap()
        .into();
    let geometry = PathControlGeometry::new(&shape, Affine::IDENTITY).unwrap();
    assert_eq!(geometry.points().len(), 7);
    for point in geometry.points() {
        let result = geometry
            .drag(point.id, Point::ZERO, Point::new(3., 4.), false)
            .unwrap();
        let next = PathControlGeometry::new(&result, Affine::IDENTITY).unwrap();
        for original in geometry.points() {
            assert_eq!(
                next.point(original.id),
                Some(if original.id == point.id {
                    original.local + Point::new(3., 4.)
                } else {
                    original.local
                })
            );
        }
    }
    assert_eq!(geometry.control_segments().len(), 4);
}

#[test]
fn rejected_public_point_patch_restores_invalid_source_shape_exactly() {
    let mut shape: Shape = Path::new(vec![PathCommand::LineTo {
        to: Point::new(3., 4.),
    }])
    .into();
    let before = shape.clone();
    assert!(
        PathControlGeometry::set_point(
            &mut shape,
            PathPointId {
                command: 0,
                role: PathPointRole::Endpoint
            },
            Point::new(20., 30.)
        )
        .is_err()
    );
    assert_eq!(shape, before);
}

#[test]
fn explicit_pivot_affects_single_selection_keyboard_rotation_and_history() {
    use gpui_flow_scene::{DisplayObject, Scene, display::Rect};
    let mut scene = Scene::new();
    let original = Affine::translation(50., 70.) * Affine::rotation(20.);
    let pivot = Point::new(-20., 35.);
    scene
        .insert(
            DisplayObject::new("shape", Rect::new(100., 80.))
                .with_transform(original)
                .with_pivot(pivot),
            None,
            None,
        )
        .unwrap();
    let mut editor = gpui_flow_editor::SceneEditor::new(scene.clone()).unwrap();
    editor.selection.select("shape".into(), false);
    editor.rotate_selection(90.).unwrap();
    close(
        editor
            .scene()
            .get(&"shape".into())
            .unwrap()
            .object
            .transform
            .transform_point(pivot),
        original.transform_point(pivot),
    );
    assert!(editor.undo());
    assert_eq!(editor.scene(), &scene);
    editor
        .engine
        .transaction(|scene| {
            scene.insert(DisplayObject::new("other", Rect::new(10., 10.)), None, None)
        })
        .unwrap();
    editor.selection.select("other".into(), true);
    editor.rotate_selection(90.).unwrap();
    assert_eq!(
        editor
            .scene()
            .get(&"shape".into())
            .unwrap()
            .object
            .transform,
        original * Affine::rotation(90.)
    );
}
