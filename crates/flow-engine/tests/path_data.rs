use gpui_flow_engine::*;
use gpui_flow_scene::{
    Affine, DisplayObject, HitTestOptions, Point, Scene, Shape,
    display::{self, Path, PathCommand as C},
};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

fn rectangle(x: f32) -> Path {
    Path::from_svg(&format!("M{x} 0h10v10h-10z")).unwrap()
}
fn engine(pen: bool) -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("group", display::Group::new())
                .with_transform(Affine::translation(50., 20.)),
            None,
            None,
        )
        .unwrap();
    let shape = if pen {
        Shape::Pen(display::Pen {
            path: rectangle(0.),
        })
    } else {
        Shape::Path(rectangle(0.))
    };
    let mut object = DisplayObject::new("path", shape).with_transform(Affine::scale(2., 2.));
    object.name = "retained name".into();
    object.data = json!({"keep": true});
    scene.insert(object, Some("group".into()), None).unwrap();
    scene
        .insert(
            DisplayObject::new("rect", display::Rect::new(5., 5.)),
            None,
            None,
        )
        .unwrap();
    SceneEngine::new(scene).unwrap()
}
fn handle(e: &SceneEngine) -> ElementHandle {
    e.handle(&"path".into()).unwrap()
}
fn data(e: &SceneEngine) -> &Path {
    match &e.object(handle(e)).unwrap().shape {
        Shape::Path(p) => p,
        Shape::Pen(p) => &p.path,
        _ => panic!(),
    }
}

#[test]
fn data_updates_preserve_path_and_pen_kind_identity_and_other_fields() {
    for pen in [false, true] {
        let mut e = engine(pen);
        let h = handle(&e);
        let mut expected = e.object(h).unwrap().clone();
        match &mut expected.shape {
            Shape::Path(p) => *p = rectangle(20.),
            Shape::Pen(p) => p.path = rectangle(20.),
            _ => panic!(),
        }
        let change = e.set_path_data(h, rectangle(20.)).unwrap();
        assert_eq!(e.object(h).unwrap(), &expected);
        assert_eq!(handle(&e), h);
        assert_eq!(
            e.scene().get(&"path".into()).unwrap().parent,
            Some("group".into())
        );
        assert_eq!(change.properties.len(), 1);
        assert_eq!(change.properties[0].property, "path.data");
        assert_eq!(change.properties[0].old, json!(rectangle(0.)));
        assert_eq!(change.properties[0].new, json!(rectangle(20.)));
        assert!(
            change.dirty[&"path".into()]
                .contains(DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT)
        );
        let descriptor = e.property_descriptor(h, &PropertyKey::PathData).unwrap();
        assert_eq!(descriptor.name, "path.data");
        assert_eq!(descriptor.value_type, PropertyType::Object);
    }
}

#[test]
fn updated_bounds_damage_and_hit_use_new_geometry_and_ancestors() {
    let mut e = engine(false);
    let h = handle(&e);
    let old = e.world_bounds(&"path".into()).unwrap().unwrap();
    assert_eq!(
        e.hit_test(Point::new(60., 30.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id,
        "path".into()
    );
    let change = e.set_path_data(h, rectangle(20.)).unwrap();
    let new = e.world_bounds(&"path".into()).unwrap().unwrap();
    assert_ne!(old, new);
    assert_eq!(e.world_bounds(&"group".into()).unwrap(), Some(new));
    let damage = change.damage_bounds().unwrap();
    assert_eq!(damage.union(old), damage);
    assert_eq!(damage.union(new), damage);
    assert!(
        e.hit_test(Point::new(60., 30.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        e.hit_test(Point::new(100., 30.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id,
        "path".into()
    );
}

#[test]
fn noops_have_no_revision_notification_or_dirty_work() {
    let mut e = engine(false);
    let h = handle(&e);
    let calls = Rc::new(RefCell::new(0));
    let count = calls.clone();
    e.on_change(None, move |_| *count.borrow_mut() += 1)
        .unwrap();
    let before = e.layout().clone();
    let change = e.set_path_data(h, rectangle(0.)).unwrap();
    assert!(!change.changed());
    assert!(change.properties.is_empty());
    assert!(change.dirty.is_empty());
    assert_eq!(e.revision(), 0);
    assert_eq!(*calls.borrow(), 0);
    assert_eq!(e.layout(), &before);
}

#[test]
fn empty_data_clears_bounds_and_hits_without_replacing_kind() {
    for pen in [false, true] {
        let mut e = engine(pen);
        let h = handle(&e);
        let change = e.set_path_data(h, Path::default()).unwrap();
        assert!(change.changed());
        assert!(data(&e).commands.is_empty());
        assert!(e.world_bounds(&"path".into()).unwrap().is_none());
        assert!(
            e.hit_test(Point::new(60., 30.), HitTestOptions::default())
                .unwrap()
                .is_none()
        );
        assert_eq!(
            e.object(h).unwrap().shape.tag(),
            if pen { "pen" } else { "path" }
        );
    }
}

#[test]
fn malformed_nonfinite_and_oversized_typed_data_are_atomic() {
    let mut e = engine(false);
    let h = handle(&e);
    let before = e.scene().clone();
    let layout = e.layout().clone();
    for path in [
        Path::new(vec![C::Close]),
        Path::new(vec![C::MoveTo {
            to: Point::new(f32::NAN, 0.),
        }]),
        Path::new(vec![C::MoveTo {
            to: Point::new(1_000_001., 0.),
        }]),
        Path::new(vec![C::MoveTo { to: Point::ZERO }; 16_385]),
    ] {
        assert!(e.set_path_data(h, path).is_err());
        assert_eq!(e.scene(), &before);
        assert_eq!(e.layout(), &layout);
        assert_eq!(e.revision(), 0);
    }
}

#[test]
fn invalid_json_target_and_mixed_batches_reject_atomically() {
    let mut e = engine(false);
    let h = handle(&e);
    let rect = e.handle(&"rect".into()).unwrap();
    let before = e.scene().clone();
    for value in [
        json!(null),
        json!("M0 0"),
        json!([]),
        json!({}),
        json!({"commands":[{"command":"move_to", "to":{"x":null,"y":0}}]}),
    ] {
        assert!(e.set_property(h, PropertyKey::PathData, value).is_err());
    }
    assert!(e.set_path_data(rect, rectangle(20.)).is_err());
    assert!(
        e.set_properties_batch(
            ChangeOrigin::Edit,
            [
                (h, PropertyKey::PathData, json!(rectangle(20.))),
                (rect, PropertyKey::PathData, json!(rectangle(30.)))
            ]
        )
        .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), 0);
}

#[test]
fn stale_foreign_and_detached_handles_cannot_update_path_data() {
    let mut e = engine(false);
    let foreign = engine(false);
    let h = handle(&e);
    assert!(e.set_path_data(handle(&foreign), rectangle(20.)).is_err());
    let (detached, _) = e.detach(h).unwrap();
    let revision = e.revision();
    assert!(e.set_path_data(h, rectangle(20.)).is_err());
    assert_eq!(e.revision(), revision);
    e.attach(&detached, None, None).unwrap();
    e.destroy(h).unwrap();
    assert!(e.set_path_data(h, rectangle(20.)).is_err());
}

#[test]
fn path_updates_roundtrip_through_scene_json() {
    for pen in [false, true] {
        let mut e = engine(pen);
        let h = handle(&e);
        e.set_path_data(
            h,
            Path::from_svg("M0 0Q4 5 10 20C30 40 50 60 70 80Z").unwrap(),
        )
        .unwrap();
        let decoded: Scene = serde_json::from_value(json!(e.scene())).unwrap();
        assert_eq!(&decoded, e.scene());
        assert_eq!(SceneEngine::new(decoded).unwrap().scene(), e.scene());
    }
}

#[test]
fn equal_bounds_geometry_change_still_invalidates_hit_data() {
    let mut e = engine(false);
    let h = handle(&e);
    let before = e.world_bounds(&"path".into()).unwrap();
    let point = Point::new(68., 38.);
    assert_eq!(
        e.hit_test(point, HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id,
        "path".into()
    );
    let change = e
        .set_path_data(h, Path::from_svg("M0 0L10 0L0 10Z").unwrap())
        .unwrap();
    assert_eq!(e.world_bounds(&"path".into()).unwrap(), before);
    assert!(change.dirty[&"path".into()].contains(DirtyFlags::HIT));
    assert!(
        !e.hit_test_all(point, HitTestOptions::default())
            .unwrap()
            .iter()
            .any(|hit| hit.id == "path".into())
    );
}

#[test]
fn rejected_layout_derivation_does_not_publish_data_or_notifications() {
    struct RejectMovedPath;
    impl LayoutProvider for RejectMovedPath {
        fn derive(&self, _: &SceneEngine, scene: &Scene) -> Result<Scene, EngineError> {
            let Shape::Path(path) = &scene.get(&"path".into()).unwrap().object.shape else {
                panic!()
            };
            if path.bounds().is_some_and(|bounds| bounds.min_x() > 0.) {
                return Err(EngineError::Layout(
                    "test provider rejects moved data".into(),
                ));
            }
            Ok(scene.clone())
        }
    }
    let mut e = engine(false);
    let h = handle(&e);
    e.install_layout_provider(RejectMovedPath).unwrap();
    let before = e.scene().clone();
    let layout = e.layout().clone();
    let calls = Rc::new(RefCell::new(0));
    let count = calls.clone();
    e.on_change(None, move |_| *count.borrow_mut() += 1)
        .unwrap();
    assert!(e.set_path_data(h, rectangle(20.)).is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.layout(), &layout);
    assert_eq!(e.revision(), 0);
    assert_eq!(*calls.borrow(), 0);
}
