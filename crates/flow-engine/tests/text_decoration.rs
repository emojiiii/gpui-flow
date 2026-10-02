use gpui_flow_engine::{
    PropertyKey, ResourceCompletion, ResourceOutcome, SceneEngine, SceneResources,
};
use gpui_flow_scene::{Scene, Shape, display::TextDecoration as D};
use serde_json::json;
#[test]
fn decoration_factory_and_updates_validate_atomically_and_invalidate_text_tickets() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h, _) = e
        .create(
            "text",
            "t",
            json!({"decoration":"underline_strikethrough"}),
            None,
            None,
        )
        .unwrap();
    let Shape::Text(mut t) = e.object(h).unwrap().shape.clone() else {
        unreachable!()
    };
    assert_eq!(t.decoration, D::UnderlineStrikethrough);
    let mut r = SceneResources::new(&mut e).unwrap();
    r.synchronize(&mut e).unwrap();
    let old = r.ticket(&e, h).unwrap();
    t.decoration = D::None;
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(t)))
        .unwrap();
    assert_eq!(
        r.complete(&mut e, old, ResourceOutcome::Loaded).unwrap().0,
        ResourceCompletion::Stale
    );
    let before = e.scene().clone();
    assert!(
        e.create("text", "bad", json!({"decoration":"wavy"}), None, None)
            .is_err()
    );
    assert_eq!(e.scene(), &before);
}
