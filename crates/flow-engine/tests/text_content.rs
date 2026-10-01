use gpui_flow_engine::*;
use gpui_flow_scene::{DisplayObject, Scene, Shape, Size, display};
use serde_json::json;

fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    let mut text = display::Text::new("before", Size::new(200., 70.));
    text.font_family = "serif".into();
    text.font_weight = 700;
    text.font_size = 23.;
    text.wrap = false;
    scene
        .insert(DisplayObject::new("text", text), None, None)
        .unwrap();
    scene
        .insert(
            DisplayObject::new("rect", display::Rect::new(10., 10.)),
            None,
            None,
        )
        .unwrap();
    SceneEngine::new(scene).unwrap()
}
#[test]
fn content_property_preserves_typography_and_emits_precise_changes() {
    let mut engine = engine();
    let handle = engine.handle(&"text".into()).unwrap();
    let mut expected = engine.object(handle).unwrap().clone();
    let Shape::Text(text) = &mut expected.shape else {
        panic!()
    };
    text.content = "🙂\nمرحبا\n".into();
    let changes = engine
        .set_property(handle, PropertyKey::TextContent, json!(text.content))
        .unwrap();
    assert_eq!(engine.object(handle).unwrap(), &expected);
    assert_eq!(changes.properties.len(), 1);
    assert_eq!(changes.properties[0].property, "text.content");
    assert_eq!(changes.properties[0].old, json!("before"));
    assert!(
        changes.dirty[&"text".into()]
            .contains(DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT)
    );
    let descriptor = engine
        .property_descriptor(handle, &PropertyKey::TextContent)
        .unwrap();
    assert_eq!(descriptor.value_type, PropertyType::String);
    assert_eq!(descriptor.name, "text.content");
}
#[test]
fn bad_value_or_shape_is_atomic_and_noop_does_not_increment_revision() {
    let mut engine = engine();
    let text = engine.handle(&"text".into()).unwrap();
    let rect = engine.handle(&"rect".into()).unwrap();
    let before = engine.scene().clone();
    let revision = engine.revision();
    for value in [json!(null), json!(9), json!({}), json!([])] {
        assert!(
            engine
                .set_property(text, PropertyKey::TextContent, value)
                .is_err()
        );
    }
    assert!(
        engine
            .set_property(rect, PropertyKey::TextContent, json!("no"))
            .is_err()
    );
    assert_eq!(engine.scene(), &before);
    assert_eq!(engine.revision(), revision);
    assert!(
        !engine
            .set_property(text, PropertyKey::TextContent, json!("before"))
            .unwrap()
            .changed()
    );
    assert_eq!(engine.revision(), revision);
}
#[test]
fn text_content_batch_with_invalid_target_is_atomic() {
    let mut engine = engine();
    let text = engine.handle(&"text".into()).unwrap();
    let rect = engine.handle(&"rect".into()).unwrap();
    let before = engine.scene().clone();
    assert!(
        engine
            .set_properties_batch(
                ChangeOrigin::Edit,
                [
                    (text, PropertyKey::TextContent, json!("after")),
                    (rect, PropertyKey::TextContent, json!("invalid")),
                ]
            )
            .is_err()
    );
    assert_eq!(engine.scene(), &before);
}
#[test]
fn precise_content_changes_invalidate_old_resource_ticket_even_after_a_b_a() {
    let mut engine = engine();
    let handle = engine.handle(&"text".into()).unwrap();
    let mut resources = SceneResources::new(&mut engine).unwrap();
    resources.synchronize(&mut engine).unwrap();
    let old = resources.ticket(&engine, handle).unwrap();
    engine
        .set_property(handle, PropertyKey::TextContent, json!("B"))
        .unwrap();
    engine
        .set_property(handle, PropertyKey::TextContent, json!("before"))
        .unwrap();
    assert_eq!(
        resources
            .complete(&mut engine, old, ResourceOutcome::Loaded)
            .unwrap()
            .0,
        ResourceCompletion::Stale
    );
    resources.synchronize(&mut engine).unwrap();
    let new = resources.ticket(&engine, handle).unwrap();
    assert_ne!(new.generation, old.generation);
}
