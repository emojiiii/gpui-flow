use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{
    DisplayObject, HitPolicy, HitTestOptions, Point, Scene, Shape,
    display::{Frame, Rect},
};
use serde_json::json;
fn editor() -> SceneEditor {
    let mut s = Scene::new();
    let mut frame = DisplayObject::new("frame", Frame::new(60., 40.));
    frame.hit_policy = HitPolicy::None;
    s.insert(frame, None, None).unwrap();
    s.insert(
        DisplayObject::new("child", Rect::new(60., 40.)),
        Some("frame".into()),
        None,
    )
    .unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    e.selection.select("child".into(), false);
    e
}
#[test]
fn corner_clip_edit_undo_redo_restores_cached_hit_and_identity() {
    let mut e = editor();
    let h = e.engine.handle(&"frame".into()).unwrap();
    let child = e.engine.handle(&"child".into()).unwrap();
    let before = e.scene().clone();
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(
            Frame::new(60., 40.).with_corners([15., 0., 0., 0.])
        )),
    )
    .unwrap();
    let after = e.scene().clone();
    assert!(
        e.engine
            .hit_test(Point::new(1., 1.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(
        e.engine
            .hit_test(Point::new(1., 1.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert_eq!(e.engine.handle(&"child".into()), Some(child));
    assert!(e.selection.ids.contains(&"child".into()));
}
#[test]
fn cancelled_or_invalid_corner_edits_preserve_redo() {
    let mut e = editor();
    let h = e.engine.handle(&"frame".into()).unwrap();
    let before = e.scene().clone();
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(
            Frame::new(60., 40.).with_corners([15., 0., 0., 0.])
        )),
    )
    .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert!(e.begin_edit());
    e.set_property(
        h,
        PropertyKey::Shape,
        json!(Shape::from(
            Frame::new(60., 40.).with_corners([0., 15., 0., 0.])
        )),
    )
    .unwrap();
    assert!(e.cancel_edit());
    assert_eq!(e.scene(), &before);
    assert!(
        e.set_property(
            h,
            PropertyKey::Shape,
            json!(Shape::from(Frame::new(60., 40.).with_corners([-1.; 4])))
        )
        .is_err()
    );
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
}
