use gpui_flow_engine::{
    DirtyFlags, PropertyKey, ResourceCompletion, ResourceOutcome, SceneEngine, SceneResources,
};
use gpui_flow_scene::{Scene, Shape, display::TextCase};
use serde_json::json;
#[test]
fn factory_styles_and_defaults_are_explicit_and_unsupported_values_are_atomic() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h, _) = e
        .create(
            "text",
            "t",
            json!({"text_case":"upper","para_spacing":9.,"content":"original"}),
            None,
            None,
        )
        .unwrap();
    let Shape::Text(t) = &e.object(h).unwrap().shape else {
        panic!()
    };
    assert_eq!(t.text_case, TextCase::Upper);
    assert_eq!(t.para_spacing, 9.);
    assert_eq!(t.content, "original");
    let (h, _) = e.create("text", "default", json!({}), None, None).unwrap();
    let Shape::Text(t) = &e.object(h).unwrap().shape else {
        panic!()
    };
    assert_eq!(t.text_case, TextCase::None);
    assert_eq!(t.para_spacing, 0.);
    let before = e.scene().clone();
    let revision = e.revision();
    for properties in [
        json!({"text_case":"title"}),
        json!({"text_case":"small-caps"}),
        json!({"para_spacing":-1.}),
        json!({"para_spacing":"10"}),
        json!({"text_case":"lower", "content":"İ".repeat(32_768)}),
    ] {
        assert!(e.create("text", "bad", properties, None, None).is_err());
        assert_eq!(e.scene(), &before);
        assert_eq!(e.revision(), revision);
    }
}
#[test]
fn each_style_edit_invalidates_resource_identity_geometry_paint_and_hit() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h, _) = e
        .create("text", "t", json!({"content":"original\ntext"}), None, None)
        .unwrap();
    let mut resources = SceneResources::new(&mut e).unwrap();
    for change_case in [true, false] {
        resources.synchronize(&mut e).unwrap();
        let old = resources.ticket(&e, h).unwrap();
        let Shape::Text(mut text) = e.object(h).unwrap().shape.clone() else {
            panic!()
        };
        if change_case {
            text.text_case = TextCase::Upper;
        } else {
            text.para_spacing = 10.;
        }
        let changes = e
            .set_property(h, PropertyKey::Shape, json!(Shape::from(text)))
            .unwrap();
        let flags = changes.dirty.get(&"t".into()).unwrap();
        assert!(flags.contains(DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT));
        assert!(resources.ticket(&e, h).is_none());
        assert_eq!(
            resources
                .complete(&mut e, old, ResourceOutcome::Loaded)
                .unwrap()
                .0,
            ResourceCompletion::Stale
        );
        let Shape::Text(text) = &e.object(h).unwrap().shape else {
            panic!()
        };
        assert_eq!(text.content, "original\ntext");
    }
}
