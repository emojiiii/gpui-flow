use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{
    DisplayObject, Scene, Shape, Size,
    display::{Image, ImageFit},
};
use serde_json::json;
#[test]
fn image_corner_fit_edits_undo_redo_and_cancel_without_replacing_identity() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("i", Image::new("x.png", Size::new(40., 40.))),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    let h = e.engine.handle(&"i".into()).unwrap();
    let before = e.scene().clone();
    let Shape::Image(mut image) = e.engine.object(h).unwrap().shape.clone() else {
        unreachable!()
    };
    image.fit = ImageFit::Cover;
    image = image.with_corners([20., 0., 10., 0.]);
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(image.clone())))
        .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(e.begin_edit());
    image.fit = ImageFit::Fill;
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(image)))
        .unwrap();
    assert!(e.cancel_edit());
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert_eq!(e.engine.handle(&"i".into()), Some(h));
}
