use gpui_flow_engine::*;
use gpui_flow_scene::{
    Affine, DisplayObject, Point, Scene, Shape,
    display::{Group, Path, Pen, Rect},
};
use serde_json::json;
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.003, "{a} != {b}");
}
fn point(a: Point, b: Point) {
    near(a.x, b.x);
    near(a.y, b.y);
}
fn engine(pen: bool) -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("route-parent", Group::new()).with_transform(
                Affine::translation(50., 20.) * Affine::skew(15., 0.) * Affine::scale(2., 3.),
            ),
            None,
            None,
        )
        .unwrap();
    let path = Path::from_svg("M0 0L30 40").unwrap();
    scene
        .insert(
            DisplayObject::new(
                "route",
                if pen {
                    Shape::Pen(Pen { path })
                } else {
                    Shape::Path(path)
                },
            ),
            Some("route-parent".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("target-parent", Group::new()).with_transform(
                Affine::translation(-50., 5.)
                    * Affine::rotation(-30.)
                    * Affine::scale(0.75, 1.5)
                    * Affine::skew(20., -5.),
            ),
            None,
            None,
        )
        .unwrap();
    let mut target = DisplayObject::new("target", Rect::new(10., 20.)).with_transform(
        Affine::translation(4., 3.) * Affine::rotation(40.) * Affine::scale(2., -3.),
    );
    target.name = "retained".into();
    target.data = json!({"keep":true});
    scene
        .insert(target, Some("target-parent".into()), None)
        .unwrap();
    SceneEngine::new(scene).unwrap()
}
fn handle(e: &SceneEngine, id: &str) -> ElementHandle {
    e.handle(&id.into()).unwrap()
}
fn gram(a: Affine) -> [f32; 3] {
    [
        a.a * a.a + a.b * a.b,
        a.c * a.c + a.d * a.d,
        a.a * a.c + a.b * a.d,
    ]
}

#[test]
fn placement_preserves_world_linear_transform_and_other_fields_for_path_and_pen() {
    for pen in [false, true] {
        let mut e = engine(pen);
        let t = handle(&e, "target");
        let p = handle(&e, "route");
        let source = e.object(p).unwrap().clone();
        let mut expected = e.object(t).unwrap().clone();
        let before = e.world_transform(&"target".into()).unwrap();
        let sample = e.motion_point(p, 25.).unwrap().unwrap();
        let options = PathPlacement {
            anchor: Point::new(5., 10.),
            ..PathPlacement::new(25.)
        };
        let changes = e.place_on_path(t, p, options).unwrap();
        point(
            e.local_to_world(&"target".into(), options.anchor).unwrap(),
            sample.point,
        );
        let after = e.world_transform(&"target".into()).unwrap();
        for (a, b) in [before.a, before.b, before.c, before.d]
            .into_iter()
            .zip([after.a, after.b, after.c, after.d])
        {
            near(a, b);
        }
        expected.transform = e.object(t).unwrap().transform;
        assert_eq!(e.object(t).unwrap(), &expected);
        assert_eq!(e.object(p).unwrap(), &source);
        assert_eq!(handle(&e, "target"), t);
        assert_eq!(changes.properties.len(), 1);
        assert_eq!(changes.properties[0].property, "transform");
        assert!(changes.dirty[&"target".into()].contains(DirtyFlags::TRANSFORM | DirtyFlags::HIT));
    }
}
#[test]
fn tangent_rotation_preserves_world_scale_shear_and_handedness() {
    let mut e = engine(false);
    let t = handle(&e, "target");
    let p = handle(&e, "route");
    let before = e.world_transform(&"target".into()).unwrap();
    let expected = e.motion_point(p, 25.).unwrap().unwrap();
    let options = PathPlacement {
        distance: 25.,
        anchor: Point::new(5., 10.),
        rotation: Some(90.),
    };
    e.place_on_path(t, p, options).unwrap();
    let after = e.world_transform(&"target".into()).unwrap();
    point(
        e.local_to_world(&"target".into(), options.anchor).unwrap(),
        expected.point,
    );
    for (a, b) in gram(before).into_iter().zip(gram(after)) {
        near(a, b);
    }
    assert_eq!(before.determinant().signum(), after.determinant().signum());
    let desired = (expected.rotation_degrees + 90.).to_radians();
    let length = after.a.hypot(after.b);
    point(
        Point::new(after.a / length, after.b / length),
        Point::new(desired.cos(), desired.sin()),
    );
}
#[test]
fn repeated_placement_is_noop_and_emits_one_initial_notification() {
    let mut e = engine(false);
    let t = handle(&e, "target");
    let p = handle(&e, "route");
    let calls = Rc::new(RefCell::new(0));
    let count = calls.clone();
    e.on_change(None, move |_| *count.borrow_mut() += 1)
        .unwrap();
    let options = PathPlacement {
        distance: 25.,
        anchor: Point::new(5., 10.),
        rotation: Some(-30.),
    };
    e.place_on_path(t, p, options).unwrap();
    let before = e.scene().clone();
    let revision = e.revision();
    for _ in 0..10 {
        assert!(!e.place_on_path(t, p, options).unwrap().changed());
    }
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
    assert_eq!(*calls.borrow(), 1);
}
#[test]
fn self_path_and_ancestor_dependency_reject_without_mutation() {
    let mut e = engine(false);
    let p = handle(&e, "route");
    let ancestor = handle(&e, "route-parent");
    let before = e.scene().clone();
    assert!(e.place_on_path(p, p, PathPlacement::new(5.)).is_err());
    assert!(
        e.place_on_path(ancestor, p, PathPlacement::new(5.))
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), 0);
}
#[test]
fn nonfinite_options_wrong_source_and_foreign_handles_are_atomic() {
    let mut e = engine(false);
    let other = engine(false);
    let t = handle(&e, "target");
    let p = handle(&e, "route");
    let before = e.scene().clone();
    for options in [
        PathPlacement::new(f64::NAN),
        PathPlacement {
            anchor: Point::new(f32::INFINITY, 0.),
            ..PathPlacement::new(2.)
        },
        PathPlacement {
            rotation: Some(f32::NAN),
            ..PathPlacement::new(2.)
        },
    ] {
        assert!(e.place_on_path(t, p, options).is_err());
    }
    assert!(e.place_on_path(t, t, PathPlacement::new(2.)).is_err());
    assert!(e.place_on_path(p, t, PathPlacement::new(2.)).is_err());
    assert!(
        e.place_on_path(handle(&other, "target"), p, PathPlacement::new(2.))
            .is_err()
    );
    assert!(
        e.place_on_path(t, handle(&other, "route"), PathPlacement::new(2.))
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), 0);
}
#[test]
fn detached_and_destroyed_target_or_path_never_retargets_reused_ids() {
    for id in ["target", "route"] {
        let mut e = engine(false);
        let t = handle(&e, "target");
        let p = handle(&e, "route");
        let h = handle(&e, id);
        let (detached, _) = e.detach(h).unwrap();
        let before = e.scene().clone();
        assert!(e.place_on_path(t, p, PathPlacement::new(5.)).is_err());
        assert_eq!(e.scene(), &before);
        e.attach(&detached, None, None).unwrap();
        e.destroy(h).unwrap();
        e.transaction(|s| {
            s.insert(
                DisplayObject::new(id, Path::from_svg("M0 0L20 0").unwrap()),
                None,
                None,
            )
        })
        .unwrap();
        let before = e.scene().clone();
        assert!(e.place_on_path(t, p, PathPlacement::new(5.)).is_err());
        assert_eq!(e.scene(), &before);
    }
}
#[test]
fn singular_target_parent_and_collapsed_rotation_axis_reject() {
    let mut e = engine(false);
    let t = handle(&e, "target");
    let p = handle(&e, "route");
    let parent = handle(&e, "target-parent");
    e.set_property(parent, PropertyKey::Transform, json!(Affine::scale(0., 1.)))
        .unwrap();
    let before = e.scene().clone();
    assert!(e.place_on_path(t, p, PathPlacement::new(5.)).is_err());
    assert_eq!(e.scene(), &before);
    e.set_property(parent, PropertyKey::Transform, json!(Affine::IDENTITY))
        .unwrap();
    e.set_property(t, PropertyKey::Transform, json!(Affine::scale(0., 1.)))
        .unwrap();
    let before = e.scene().clone();
    assert!(
        e.place_on_path(
            t,
            p,
            PathPlacement {
                rotation: Some(0.),
                ..PathPlacement::new(5.)
            }
        )
        .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert!(e.place_on_path(t, p, PathPlacement::new(5.)).is_ok());
}
#[test]
fn empty_or_world_collapsed_source_rejects_placement() {
    let mut e = engine(false);
    let t = handle(&e, "target");
    let p = handle(&e, "route");
    e.set_path_data(p, Path::default()).unwrap();
    let before = e.scene().clone();
    assert!(e.place_on_path(t, p, PathPlacement::new(5.)).is_err());
    assert_eq!(e.scene(), &before);
    e.set_path_data(p, Path::from_svg("M0 0L10 0").unwrap())
        .unwrap();
    e.set_property(p, PropertyKey::Transform, json!(Affine::scale(0., 0.)))
        .unwrap();
    let before = e.scene().clone();
    assert!(e.place_on_path(t, p, PathPlacement::new(5.)).is_err());
    assert_eq!(e.scene(), &before);
}
#[test]
fn finite_out_of_range_distance_clamps_in_source_local_units() {
    let mut e = engine(false);
    let t = handle(&e, "target");
    let p = handle(&e, "route");
    for distance in [-10., 1000.] {
        let expected = e.motion_point(p, distance).unwrap().unwrap();
        e.place_on_path(t, p, PathPlacement::new(distance)).unwrap();
        point(
            e.local_to_world(&"target".into(), Point::ZERO).unwrap(),
            expected.point,
        );
    }
}
#[test]
fn layout_cannot_override_the_pose_or_move_the_source_during_commit() {
    struct Override {
        enabled: Arc<AtomicBool>,
        id: &'static str,
    }
    impl LayoutProvider for Override {
        fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
            let mut next = candidate.clone();
            if self.enabled.load(Ordering::SeqCst) {
                next.update_object(&self.id.into(), |o| o.transform.tx = 999.)?;
            }
            Ok(next)
        }
    }
    for id in ["target", "route-parent", "target-parent"] {
        let mut e = engine(false);
        let t = handle(&e, "target");
        let p = handle(&e, "route");
        let enabled = Arc::new(AtomicBool::new(false));
        e.install_layout_provider(Override {
            enabled: enabled.clone(),
            id,
        })
        .unwrap();
        enabled.store(true, Ordering::SeqCst);
        let before = e.scene().clone();
        let layout = e.layout().clone();
        let calls = Rc::new(RefCell::new(0));
        let count = calls.clone();
        e.on_change(None, move |_| *count.borrow_mut() += 1)
            .unwrap();
        assert!(e.place_on_path(t, p, PathPlacement::new(25.)).is_err());
        assert_eq!(e.scene(), &before);
        assert_eq!(e.layout(), &layout);
        assert_eq!(e.revision(), 0);
        assert_eq!(*calls.borrow(), 0);
    }
}

#[test]
fn huge_finite_rotation_offset_does_not_erase_the_sampled_tangent_angle() {
    let mut e = engine(false);
    let t = handle(&e, "target");
    let p = handle(&e, "route");
    let sample = e.motion_point(p, 25.).unwrap().unwrap();
    e.place_on_path(
        t,
        p,
        PathPlacement {
            rotation: Some(f32::MAX),
            ..PathPlacement::new(25.)
        },
    )
    .unwrap();
    let world = e.world_transform(&"target".into()).unwrap();
    let expected =
        (sample.rotation_degrees as f64 + (f32::MAX as f64).rem_euclid(360.)).to_radians();
    let norm = world.a.hypot(world.b);
    near(world.a / norm, expected.cos() as f32);
    near(world.b / norm, expected.sin() as f32);
}

#[test]
fn tiny_nonzero_world_axes_still_rotate_instead_of_becoming_a_noop() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("route", Path::from_svg("M0 0L0 10").unwrap()),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("target", Rect::new(1., 1.))
                .with_transform(Affine::scale(1e-20, 2e-20)),
            None,
            None,
        )
        .unwrap();
    let mut e = SceneEngine::new(scene).unwrap();
    let t = handle(&e, "target");
    let p = handle(&e, "route");
    assert!(
        e.place_on_path(
            t,
            p,
            PathPlacement {
                rotation: Some(0.),
                ..PathPlacement::new(0.)
            }
        )
        .unwrap()
        .changed()
    );
    let world = e.world_transform(&"target".into()).unwrap();
    assert!((world.a as f64).abs() < 1e-25);
    assert!((world.b as f64 - 1e-20).abs() < 1e-25);
}
