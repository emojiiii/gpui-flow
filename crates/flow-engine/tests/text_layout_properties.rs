use gpui_flow_engine::{
    DirtyFlags, PropertyKey, ResourceCompletion, ResourceOutcome, SceneEngine, SceneResources,
};
use gpui_flow_scene::{Point, Scene, Shape, display::TextWritingMode};
use serde_json::json;

#[test]
fn factory_registers_layout_properties_and_preserves_legacy_defaults() {
    let mut engine = SceneEngine::new(Scene::new()).unwrap();
    let (handle, _) = engine
        .create("text", "default", json!({}), None, None)
        .unwrap();
    let Shape::Text(text) = &engine.object(handle).unwrap().shape else {
        panic!()
    };
    assert_eq!(
        (text.letter_spacing, text.word_spacing, text.para_indent),
        (0., 0., 0.)
    );
    assert_eq!(text.writing_mode, TextWritingMode::X);
    assert!(!text.auto_size_align);
    assert_eq!(text.layout_origin, Point::ZERO);
    let (handle, _) = engine
        .create(
            "text",
            "styled",
            json!({
                "content":"MiXeD\n世界", "letter_spacing":1.5, "word_spacing":4.,
                "para_indent":12., "writing_mode":"x", "auto_size_align":true,
                "layout_origin":{"x":-50.,"y":-15.}
            }),
            None,
            None,
        )
        .unwrap();
    let Shape::Text(text) = &engine.object(handle).unwrap().shape else {
        panic!()
    };
    assert_eq!(
        (text.letter_spacing, text.word_spacing, text.para_indent),
        (1.5, 4., 12.)
    );
    assert_eq!(text.writing_mode, TextWritingMode::X);
    assert!(text.auto_size_align);
    assert_eq!(text.layout_origin, Point::new(-50., -15.));
    assert_eq!(text.content, "MiXeD\n世界");
}

#[test]
fn invalid_factory_and_shape_updates_leave_scene_layout_revision_and_resources_unchanged() {
    let mut engine = SceneEngine::new(Scene::new()).unwrap();
    let (handle, _) = engine
        .create("text", "t", json!({"content":"original"}), None, None)
        .unwrap();
    let mut resources = SceneResources::new(&mut engine).unwrap();
    resources.synchronize(&mut engine).unwrap();
    let ticket = resources.ticket(&engine, handle).unwrap();
    let before = engine.scene().clone();
    let layout = engine.layout().clone();
    let revision = engine.revision();
    for (key, value) in [
        ("letter_spacing", json!(-1.)),
        ("word_spacing", json!(-1.)),
        ("para_indent", json!(-1.)),
        ("letter_spacing", json!(1_000_001.)),
        ("word_spacing", json!(1_000_001.)),
        ("para_indent", json!(1_000_001.)),
        ("letter_spacing", json!({"type":"percent","value":0.1})),
        ("word_spacing", json!("10%")),
        ("para_indent", json!("10")),
        ("writing_mode", json!("y")),
        ("writing_mode", json!("x-reverse")),
        ("writing_mode", json!("y-reverse")),
        ("writing_mode", json!("unknown")),
        ("auto_size_align", json!("true")),
        ("layout_origin", json!({"x":-1_000_001.,"y":0.})),
        ("layout_origin", json!({"x":0.,"y":1_000_001.})),
    ] {
        let mut properties = json!({});
        properties[key] = value.clone();
        assert!(
            engine
                .create("text", "bad", properties, None, None)
                .is_err(),
            "{key}: {value}"
        );
        let mut shape = json!(engine.object(handle).unwrap().shape);
        shape["geometry"][key] = value;
        assert!(
            engine
                .set_property(handle, PropertyKey::Shape, shape)
                .is_err(),
            "{key}"
        );
        assert_eq!(engine.scene(), &before);
        assert_eq!(engine.layout(), &layout);
        assert_eq!(engine.revision(), revision);
        assert_eq!(resources.ticket(&engine, handle), Some(ticket));
    }
}

#[test]
fn each_layout_edit_invalidates_geometry_paint_hit_and_old_resource_identity() {
    let mut engine = SceneEngine::new(Scene::new()).unwrap();
    let (handle, _) = engine
        .create("text", "t", json!({"content":"original\n世界"}), None, None)
        .unwrap();
    let mut resources = SceneResources::new(&mut engine).unwrap();
    for (key, value) in [
        ("letter_spacing", json!(2.)),
        ("word_spacing", json!(5.)),
        ("para_indent", json!(10.)),
        ("auto_size_align", json!(true)),
        ("layout_origin", json!({"x":-50.,"y":-15.})),
    ] {
        resources.synchronize(&mut engine).unwrap();
        let old = resources.ticket(&engine, handle).unwrap();
        let mut shape = json!(engine.object(handle).unwrap().shape);
        shape["geometry"][key] = value;
        let changes = engine
            .set_property(handle, PropertyKey::Shape, shape)
            .unwrap();
        let flags = changes.dirty.get(&"t".into()).unwrap();
        assert!(
            flags.contains(DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT),
            "{key}"
        );
        assert!(resources.ticket(&engine, handle).is_none());
        assert_eq!(
            resources
                .complete(&mut engine, old, ResourceOutcome::Loaded)
                .unwrap()
                .0,
            ResourceCompletion::Stale
        );
        let Shape::Text(text) = &engine.object(handle).unwrap().shape else {
            panic!()
        };
        assert_eq!(text.content, "original\n世界");
    }
    assert_eq!(
        engine.world_bounds(&"t".into()).unwrap().unwrap().origin,
        Point::new(-50., -15.)
    );
    let before = engine.scene().clone();
    let revision = engine.revision();
    assert!(
        engine
            .set_property(handle, PropertyKey::TextContent, json!("a".repeat(65_537)))
            .is_err()
    );
    assert_eq!(engine.scene(), &before);
    assert_eq!(engine.revision(), revision);
}
