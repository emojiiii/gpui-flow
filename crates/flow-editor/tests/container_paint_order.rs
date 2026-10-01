use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{display, *};
use serde_json::json;

fn shapes() -> [Shape; 2] {
    [
        display::Box::new(30.0, 30.0).into(),
        display::Frame::new(30.0, 30.0).into(),
    ]
}

fn set_order(shape: &mut Shape, order: ContainerPaintOrder) {
    match shape {
        Shape::Box(shape) => shape.paint_order = order,
        Shape::Frame(shape) => shape.paint_order = order,
        _ => panic!("expected a painted container"),
    }
}

fn editor(shape: Shape) -> SceneEditor {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("parent", shape)
                .with_transform(Affine::translation(20.0, 20.0))
                .with_style(Style {
                    fill: Some(Color::WHITE.into()),
                    stroke: Some(Stroke::new(Color::BLACK, 8.0)),
                    ..Style::default()
                }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("child", display::Rect::new(30.0, 30.0)),
            Some("parent".into()),
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    editor.selection.select("child".into(), false);
    editor
}

#[test]
fn container_order_transaction_undo_redo_preserves_live_identity_and_picking() {
    for shape in shapes() {
        let mut editor = editor(shape);
        let before = editor.scene().clone();
        let selection = editor.selection.clone();
        let parent = editor.engine.handle(&"parent".into()).unwrap();
        let child = editor.engine.handle(&"child".into()).unwrap();
        let instances = [
            editor.engine.instance(parent).unwrap(),
            editor.engine.instance(child).unwrap(),
        ];
        let order = editor.scene().paint_order().unwrap();
        let overlap = Point::new(22.0, 35.0);
        let hits = editor
            .engine
            .hit_test_all(overlap, HitTestOptions::default())
            .unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            vec!["child", "parent"]
        );

        editor
            .transaction(|scene| {
                scene.update_object(&"parent".into(), |object| {
                    set_order(&mut object.shape, ContainerPaintOrder::StrokeAfterChildren);
                })
            })
            .unwrap();
        let after = editor.scene().clone();
        assert_ne!(after, before);
        assert_eq!(Scene::from_json(&after.to_json().unwrap()).unwrap(), after);
        assert_eq!(editor.scene().paint_order().unwrap(), order);
        assert_eq!(
            editor
                .engine
                .hit_test_all(overlap, HitTestOptions::default())
                .unwrap(),
            hits
        );
        assert_eq!(
            editor
                .scene()
                .hit_test_all(overlap, HitTestOptions::default())
                .unwrap(),
            hits
        );
        assert_eq!(editor.engine.paint_list().unwrap().len(), 2);
        assert_eq!(editor.selection, selection);
        assert_eq!(editor.engine.instance(parent).unwrap(), instances[0]);
        assert_eq!(editor.engine.instance(child).unwrap(), instances[1]);

        assert!(editor.undo());
        assert_eq!(editor.scene(), &before);
        assert!(!editor.undo());
        assert_eq!(editor.engine.instance(parent).unwrap(), instances[0]);
        assert_eq!(editor.engine.instance(child).unwrap(), instances[1]);
        assert!(editor.redo());
        assert_eq!(editor.scene(), &after);
        assert!(!editor.redo());
        assert_eq!(editor.selection, selection);
        assert_eq!(editor.engine.instance(parent).unwrap(), instances[0]);
        assert_eq!(editor.engine.instance(child).unwrap(), instances[1]);
        assert_eq!(
            editor
                .engine
                .hit_test_all(overlap, HitTestOptions::default())
                .unwrap(),
            hits
        );
    }
}

#[test]
fn shape_property_supports_order_and_invalid_values_preserve_redo() {
    for shape in shapes() {
        let mut editor = editor(shape);
        let parent = editor.engine.handle(&"parent".into()).unwrap();
        let identity = editor.engine.instance(parent).unwrap();
        let before = editor.scene().clone();
        let mut shape = editor.engine.object(parent).unwrap().shape.clone();
        set_order(&mut shape, ContainerPaintOrder::StrokeAfterChildren);
        editor
            .set_property(
                parent,
                PropertyKey::Shape,
                serde_json::to_value(&shape).unwrap(),
            )
            .unwrap();
        let after = editor.scene().clone();
        assert_eq!(editor.engine.object(parent).unwrap().shape, shape);
        assert!(editor.undo());
        assert_eq!(editor.scene(), &before);

        let revision = editor.engine.revision();
        let mut invalid = serde_json::to_value(&shape).unwrap();
        invalid["geometry"]["paint_order"] = json!("invalid_order");
        assert!(
            editor
                .set_property(parent, PropertyKey::Shape, invalid)
                .is_err()
        );
        assert_eq!(editor.engine.revision(), revision);
        assert_eq!(editor.scene(), &before);
        assert_eq!(editor.engine.instance(parent).unwrap(), identity);
        assert!(editor.redo());
        assert_eq!(editor.scene(), &after);
        assert_eq!(editor.engine.instance(parent).unwrap(), identity);
    }
}
