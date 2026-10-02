use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{
    DisplayObject, Point, Scene, Shape,
    display::{Path, PathCommand as C, Pen},
};
use serde_json::json;

fn path(x: f32) -> Path {
    Path::from_svg(&format!("M{x} 0h10v10z")).unwrap()
}
fn editor(pen: bool) -> SceneEditor {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new(
                "path",
                if pen {
                    Shape::Pen(Pen { path: path(0.) })
                } else {
                    Shape::Path(path(0.))
                },
            ),
            None,
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    editor.selection.select("path".into(), false);
    editor
}

#[test]
fn path_and_pen_edits_undo_redo_exactly_once_preserving_selection_and_identity() {
    for pen in [false, true] {
        let mut e = editor(pen);
        let h = e.engine.handle(&"path".into()).unwrap();
        let before = e.scene().clone();
        e.set_path_data(h, path(20.)).unwrap();
        let after = e.scene().clone();
        assert!(e.undo());
        assert_eq!(e.scene(), &before);
        assert!(!e.undo());
        assert!(e.redo());
        assert_eq!(e.scene(), &after);
        assert!(!e.redo());
        assert_eq!(e.engine.handle(&"path".into()), Some(h));
        assert!(e.selection.ids.contains(&"path".into()));
    }
}

#[test]
fn noop_and_invalid_edits_preserve_redo() {
    let mut e = editor(false);
    let h = e.engine.handle(&"path".into()).unwrap();
    e.set_path_data(h, path(20.)).unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    e.set_path_data(h, path(0.)).unwrap();
    assert!(
        e.set_path_data(
            h,
            Path::new(vec![C::MoveTo {
                to: Point::new(f32::NAN, 0.)
            }])
        )
        .is_err()
    );
    assert!(
        e.set_property(h, PropertyKey::PathData, json!("bad"))
            .is_err()
    );
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert!(!e.redo());
}

#[test]
fn generic_edit_groups_multiple_updates_into_one_undo() {
    let mut e = editor(false);
    let h = e.engine.handle(&"path".into()).unwrap();
    let before = e.scene().clone();
    assert!(e.begin_edit());
    for x in [10., 20., 30.] {
        e.set_path_data(h, path(x)).unwrap();
    }
    e.commit_edit();
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
}

#[test]
fn cancel_edit_restores_geometry_and_preserves_prior_redo() {
    let mut e = editor(true);
    let h = e.engine.handle(&"path".into()).unwrap();
    let before = e.scene().clone();
    e.set_path_data(h, path(40.)).unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert!(e.begin_edit());
    e.set_path_data(h, path(20.)).unwrap();
    assert!(e.cancel_edit());
    assert_eq!(e.scene(), &before);
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
}

#[test]
fn owned_session_path_property_supports_cancel_and_single_undo() {
    let mut e = editor(false);
    let h = e.engine.handle(&"path".into()).unwrap();
    let before = e.scene().clone();
    let token = e.begin_edit_session().unwrap();
    e.set_edit_session_property(token, h, PropertyKey::PathData, json!(path(10.)))
        .unwrap();
    assert!(e.cancel_edit_session(token));
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
    let token = e.begin_edit_session().unwrap();
    for x in [10., 20., 30.] {
        e.set_edit_session_property(token, h, PropertyKey::PathData, json!(path(x)))
            .unwrap();
    }
    assert!(e.finish_edit_session(token));
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
}

#[test]
fn failed_typed_edit_keeps_owned_session_active() {
    let mut e = editor(false);
    let h = e.engine.handle(&"path".into()).unwrap();
    let before = e.scene().clone();
    let token = e.begin_edit_session().unwrap();
    e.set_edit_session_property(token, h, PropertyKey::PathData, json!(path(10.)))
        .unwrap();
    assert!(e.set_path_data(h, Path::new(vec![C::Close])).is_err());
    assert!(e.is_edit_session_active(token));
    assert!(e.cancel_edit_session(token));
    assert_eq!(e.scene(), &before);
}
