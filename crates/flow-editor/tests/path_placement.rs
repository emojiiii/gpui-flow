use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{PathPlacement, PropertyKey};
use gpui_flow_scene::{
    DisplayObject, Point, Scene,
    display::{Path, Rect},
};
use serde_json::json;
fn editor() -> SceneEditor {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("route", Path::from_svg("M0 0L100 100").unwrap()),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("target", Rect::new(10., 20.)),
            None,
            None,
        )
        .unwrap();
    let mut e = SceneEditor::new(scene).unwrap();
    e.selection.select("target".into(), false);
    e
}
fn place(e: &mut SceneEditor, distance: f64) {
    let t = e.engine.handle(&"target".into()).unwrap();
    let p = e.engine.handle(&"route".into()).unwrap();
    e.place_on_path(
        t,
        p,
        PathPlacement {
            distance,
            anchor: Point::new(5., 10.),
            rotation: Some(0.),
        },
    )
    .unwrap();
}
#[test]
fn explicit_placement_is_one_undo_preserving_selection_and_handles() {
    let mut e = editor();
    let h = e.engine.handle(&"target".into()).unwrap();
    let before = e.scene().clone();
    place(&mut e, 50.);
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert!(!e.redo());
    assert_eq!(e.engine.handle(&"target".into()), Some(h));
    assert!(e.selection.ids.contains(&"target".into()));
}
#[test]
fn grouped_placements_cancel_and_commit_with_one_history_entry() {
    let mut e = editor();
    let before = e.scene().clone();
    assert!(e.begin_edit());
    place(&mut e, 20.);
    place(&mut e, 40.);
    assert!(e.cancel_edit());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
    assert!(e.begin_edit());
    for d in [20., 40., 60.] {
        place(&mut e, d);
    }
    e.commit_edit();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
}
#[test]
fn noop_and_failed_placement_keep_existing_redo() {
    let mut e = editor();
    place(&mut e, 20.);
    let baseline = e.scene().clone();
    place(&mut e, 40.);
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &baseline);
    place(&mut e, 20.);
    let t = e.engine.handle(&"target".into()).unwrap();
    let p = e.engine.handle(&"route".into()).unwrap();
    assert!(e.place_on_path(t, p, PathPlacement::new(f64::NAN)).is_err());
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
}
#[test]
fn invalid_placement_does_not_end_owned_edit_session() {
    let mut e = editor();
    let t = e.engine.handle(&"target".into()).unwrap();
    let p = e.engine.handle(&"route".into()).unwrap();
    let before = e.scene().clone();
    let token = e.begin_edit_session().unwrap();
    e.set_edit_session_property(token, t, PropertyKey::Name, json!("editing"))
        .unwrap();
    assert!(
        e.place_on_path(t, p, PathPlacement::new(f64::INFINITY))
            .is_err()
    );
    assert!(e.is_edit_session_active(token));
    assert!(e.cancel_edit_session(token));
    assert_eq!(e.scene(), &before);
}
#[test]
fn successful_external_placement_seals_owned_edit_as_separate_history() {
    let mut e = editor();
    let t = e.engine.handle(&"target".into()).unwrap();
    let before = e.scene().clone();
    let token = e.begin_edit_session().unwrap();
    e.set_edit_session_property(token, t, PropertyKey::Name, json!("editing"))
        .unwrap();
    let edited = e.scene().clone();
    place(&mut e, 50.);
    assert!(!e.is_edit_session_active(token));
    assert!(e.undo());
    assert_eq!(e.scene(), &edited);
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
}
