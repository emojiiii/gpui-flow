use gpui_flow_engine::{
    PropertyKey, ResourceCompletion, ResourceOutcome, SceneEngine, SceneResources,
};
use gpui_flow_scene::{Scene, Shape};
use serde_json::json;
#[test]
fn factory_accepts_real_font_weight_and_italic_with_original_defaults() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h, _) = e
        .create(
            "text",
            "styled",
            json!({"font_weight":700,"italic":true}),
            None,
            None,
        )
        .unwrap();
    let Shape::Text(t) = &e.object(h).unwrap().shape else {
        unreachable!()
    };
    assert!(t.italic);
    assert_eq!(t.font_weight, 700);
    let (h, _) = e.create("text", "default", json!({}), None, None).unwrap();
    let Shape::Text(t) = &e.object(h).unwrap().shape else {
        unreachable!()
    };
    assert!(!t.italic);
    assert_eq!(t.font_weight, 400);
    let before = e.scene().clone();
    for properties in [
        json!({"font_weight":0}),
        json!({"font_weight":1001}),
        json!({"italic":"true"}),
    ] {
        assert!(e.create("text", "bad", properties, None, None).is_err());
        assert_eq!(e.scene(), &before);
    }
}
#[test]
fn italic_edits_invalidate_resource_tickets_without_replacing_content_or_geometry() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h, _) = e
        .create("text", "t", json!({"content":"original"}), None, None)
        .unwrap();
    let mut r = SceneResources::new(&mut e).unwrap();
    r.synchronize(&mut e).unwrap();
    let old = r.ticket(&e, h).unwrap();
    let bounds = e.world_bounds(&"t".into()).unwrap();
    let Shape::Text(mut t) = e.object(h).unwrap().shape.clone() else {
        unreachable!()
    };
    t.italic = true;
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(t)))
        .unwrap();
    assert!(r.ticket(&e, h).is_none());
    assert_eq!(
        r.complete(&mut e, old, ResourceOutcome::Loaded).unwrap().0,
        ResourceCompletion::Stale
    );
    assert_eq!(e.world_bounds(&"t".into()).unwrap(), bounds);
    let Shape::Text(t) = &e.object(h).unwrap().shape else {
        unreachable!()
    };
    assert_eq!(t.content, "original");
}
