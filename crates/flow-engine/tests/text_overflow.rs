use gpui_flow_engine::{
    PropertyKey, ResourceCompletion, ResourceOutcome, SceneEngine, SceneResources,
};
use gpui_flow_scene::{Scene, Shape, display::TextOverflow};
use serde_json::json;
#[test]
fn text_ellipsis_factory_and_ticket_updates_preserve_bounds_and_identity() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h, _) = e
        .create(
            "text",
            "t",
            json!({"content":"original long text","overflow":"ellipsis"}),
            None,
            None,
        )
        .unwrap();
    let Shape::Text(mut t) = e.object(h).unwrap().shape.clone() else {
        unreachable!()
    };
    assert_eq!(t.overflow, TextOverflow::Ellipsis);
    let bounds = e.world_bounds(&"t".into()).unwrap();
    let mut resources = SceneResources::new(&mut e).unwrap();
    resources.synchronize(&mut e).unwrap();
    let old = resources.ticket(&e, h).unwrap();
    t.overflow = TextOverflow::Clip;
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(t)))
        .unwrap();
    assert_eq!(e.world_bounds(&"t".into()).unwrap(), bounds);
    assert!(resources.ticket(&e, h).is_none());
    assert_eq!(
        resources
            .complete(&mut e, old, ResourceOutcome::Loaded)
            .unwrap()
            .0,
        ResourceCompletion::Stale
    );
    assert_eq!(e.handle(&"t".into()), Some(h));
}
#[test]
fn invalid_overflow_factory_and_oversized_update_leave_scene_atomic() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    assert!(
        e.create("text", "bad", json!({"overflow":"show"}), None, None)
            .is_err()
    );
    let (h, _) = e
        .create("text", "t", json!({"overflow":"ellipsis"}), None, None)
        .unwrap();
    let before = e.scene().clone();
    let revision = e.revision();
    assert!(
        e.set_property(h, PropertyKey::TextContent, json!("x".repeat(65537)))
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
}
