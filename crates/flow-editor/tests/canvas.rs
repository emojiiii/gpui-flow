use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{DisplayObject, Scene, Shape, display::Canvas};
use serde_json::json;
#[test]
fn canvas_config_undo_redo_and_cancel_keep_live_identity_and_atomic_validation() {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("c", Canvas::new(30., 20.)), None, None)
        .unwrap();
    let mut e = SceneEditor::new(scene).unwrap();
    let h = e.engine.handle(&"c".into()).unwrap();
    let before = e.scene().clone();
    let mut config = Canvas::new(60., 40.).with_corners([10., 0., 0., 0.]);
    config.pixel_ratio = 2.;
    config.safe_resize = true;
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(config)))
        .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(e.begin_edit());
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(Canvas::new(70., 40.))),
    )
    .unwrap();
    assert!(e.cancel_edit());
    assert_eq!(e.scene(), &before);
    assert!(
        e.set_property(
            h,
            PropertyKey::Shape,
            json!(Shape::from(Canvas::new(4096., 4096.)))
        )
        .is_err()
    );
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert_eq!(e.engine.handle(&"c".into()), Some(h));
}
