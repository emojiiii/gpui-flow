use gpui_flow_engine::{
    PropertyKey, ResourceCompletion, ResourceKind, ResourceOutcome, SceneEngine, SceneResources,
};
use gpui_flow_scene::{Scene, Shape, display::Canvas};
use serde_json::json;
#[test]
fn canvas_factory_config_edits_are_validated_atomically() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h, _) = e.create("canvas", "c", json!({}), None, None).unwrap();
    assert!(matches!(&e.object(h).unwrap().shape,Shape::Canvas(c) if c==&Canvas::default()));
    let before = e.scene().clone();
    let revision = e.revision();
    assert!(
        e.set_property(
            h,
            PropertyKey::Shape,
            json!({"type":"canvas","geometry":{"size":{"width":4096.,"height":4096.}}})
        )
        .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
    let change = e
        .set_property(
            h,
            PropertyKey::Shape,
            json!(Shape::from(Canvas::new(40., 20.))),
        )
        .unwrap();
    assert!(!change.properties.is_empty());
    assert_eq!(
        e.object(h)
            .unwrap()
            .shape
            .local_bounds()
            .unwrap()
            .size
            .width,
        40.
    );
}
#[test]
fn canvas_resource_tickets_reject_unobserved_a_b_a_and_old_attachment() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h, _) = e.create("canvas", "c", json!({}), None, None).unwrap();
    let mut r = SceneResources::new(&mut e).unwrap();
    r.synchronize(&mut e).unwrap();
    let old = r.ticket(&e, h).unwrap();
    assert_eq!(old.kind, ResourceKind::Canvas);
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(Canvas::new(40., 20.))),
    )
    .unwrap();
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(Canvas::default())))
        .unwrap();
    assert!(r.ticket(&e, h).is_none());
    assert_eq!(
        r.complete(&mut e, old, ResourceOutcome::Loaded).unwrap().0,
        ResourceCompletion::Stale
    );
    r.synchronize(&mut e).unwrap();
    let current = r.ticket(&e, h).unwrap();
    assert_ne!(current, old);
    let (sub, _) = e.detach(h).unwrap();
    e.attach(&sub, None, None).unwrap();
    r.synchronize(&mut e).unwrap();
    assert_ne!(
        r.ticket(&e, h).unwrap().attachment_epoch,
        current.attachment_epoch
    );
}
