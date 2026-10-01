use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{display, *};
#[test]
fn recursive_shadow_edit_undo_redo_preserves_geometry_selection_and_identity() {
    for shape in [
        Shape::Group(display::Group::new()),
        Shape::Box(display::Box::new(20., 20.)),
        Shape::Frame(display::Frame::new(20., 20.)),
    ] {
        let mut s = Scene::new();
        s.insert(DisplayObject::new("g", shape), None, None)
            .unwrap();
        s.insert(
            DisplayObject::new("r", display::Rect::new(10., 10.)).with_style(Style {
                shadow: Some(Shadow::new(20., 0., 0., Color::BLACK)),
                ..Default::default()
            }),
            Some("g".into()),
            None,
        )
        .unwrap();
        let mut e = SceneEditor::new(s).unwrap();
        e.selection.select("g".into(), false);
        let before = e.scene().clone();
        let selection = e.selection.clone();
        let geometry = e.engine.world_bounds(&"g".into()).unwrap();
        let handle = e.engine.handle(&"g".into()).unwrap();
        let identity = e.engine.instance(handle).unwrap();
        e.set_property(
            handle,
            PropertyKey::Shadow,
            serde_json::to_value(Shadow::new(40., 0., 2., Color::BLACK)).unwrap(),
        )
        .unwrap();
        let after = e.scene().clone();
        assert_eq!(Scene::from_json(&after.to_json().unwrap()).unwrap(), after);
        assert_eq!(e.selection, selection);
        assert_eq!(e.engine.world_bounds(&"g".into()).unwrap(), geometry);
        assert!(
            e.engine
                .effect_bounds(&"g".into())
                .unwrap()
                .unwrap()
                .max_x()
                >= 64.
        );
        assert_eq!(e.engine.instance(handle).unwrap(), identity);
        assert!(e.undo());
        assert_eq!(e.scene(), &before);
        let revision = e.engine.revision();
        assert!(
            e.set_property(
                handle,
                PropertyKey::Shadow,
                serde_json::json!({"x":0,"y":0,"blur":-1,"color":{"r":0,"g":0,"b":0,"a":255}})
            )
            .is_err()
        );
        assert_eq!(e.engine.revision(), revision);
        assert!(e.redo());
        assert_eq!(e.scene(), &after);
        assert_eq!(e.selection, selection);
        assert_eq!(e.engine.instance(handle).unwrap(), identity);
        assert!(
            e.engine
                .hit_test(Point::new(55., 5.), Default::default())
                .unwrap()
                .is_none()
        );
    }
}
