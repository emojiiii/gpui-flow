use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{display, *};

#[test]
fn inner_edit_undo_redo_preserves_selection_bounds_and_identity() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(20., 20.)),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    e.selection.select("r".into(), false);
    let before = e.scene().clone();
    let selection = e.selection.clone();
    let handle = e.engine.handle(&"r".into()).unwrap();
    let identity = e.engine.instance(handle).unwrap();
    let bounds = e.engine.effect_bounds(&"r".into()).unwrap();
    e.set_property(
        handle,
        PropertyKey::InnerShadowLayers,
        serde_json::to_value([
            Shadow::new(5., 0., 2., Color::BLACK),
            Shadow::new(-3., 0., 0., Color::WHITE),
        ])
        .unwrap(),
    )
    .unwrap();
    let after = e.scene().clone();
    assert_eq!(e.selection, selection);
    assert_eq!(e.engine.instance(handle).unwrap(), identity);
    assert_eq!(e.engine.effect_bounds(&"r".into()).unwrap(), bounds);
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(
        e.set_property(
            handle,
            PropertyKey::InnerShadowLayers,
            serde_json::to_value(vec![
                Shadow::new(0., 0., 0., Color::BLACK);
                MAX_INNER_SHADOW_LAYERS + 1
            ])
            .unwrap()
        )
        .is_err()
    );
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert_eq!(e.selection, selection);
}
