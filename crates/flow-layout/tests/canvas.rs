use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::*;
use gpui_flow_scene::{DisplayObject, Scene, Shape, Size, display};
fn setup() -> (Scene, FlowLayout) {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("root", display::Frame::new(100., 100.)),
        None,
        None,
    )
    .unwrap();
    let mut c = display::Canvas::new(20., 10.);
    c.pixel_ratio = 2.;
    c.safe_resize = true;
    c.smooth = false;
    s.insert(DisplayObject::new("c", c), Some("root".into()), None)
        .unwrap();
    let mut layout = FlowLayout::default();
    layout
        .containers
        .insert("root".into(), FlowStyle::default());
    layout.items.insert(
        "c".into(),
        FlowItem {
            grow: 1.,
            lock_ratio: true,
            ..Default::default()
        },
    );
    (s, layout)
}
#[test]
fn canvas_layout_grow_and_reactive_resize_preserve_configuration_in_one_undo() {
    let (mut scene, layout) = setup();
    layout.apply_to_scene(&mut scene).unwrap();
    let Shape::Canvas(c) = &scene.get(&"c".into()).unwrap().object.shape else {
        unreachable!()
    };
    assert_eq!(c.size, Size::new(100., 50.));
    assert_eq!(c.pixel_ratio, 2.);
    assert!(c.safe_resize);
    assert!(!c.smooth);
    let mut e = SceneEditor::new(scene).unwrap();
    layout.install_reactive(&mut e.engine).unwrap();
    let before = e.scene().clone();
    e.transaction(|s| {
        s.update_object(&"root".into(), |o| {
            o.shape = display::Frame::new(200., 100.).into()
        })
    })
    .unwrap();
    assert_eq!(
        e.scene()
            .get(&"c".into())
            .unwrap()
            .object
            .shape
            .local_bounds()
            .unwrap()
            .size,
        Size::new(200., 100.)
    );
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
}
#[test]
fn canvas_oversized_derived_backing_store_rejects_whole_layout_transaction() {
    let (mut scene, layout) = setup();
    layout.apply_to_scene(&mut scene).unwrap();
    let mut e = SceneEditor::new(scene).unwrap();
    layout.install_reactive(&mut e.engine).unwrap();
    let before = e.scene().clone();
    let rev = e.engine.revision();
    assert!(
        e.transaction(|s| s.update_object(&"root".into(), |o| o.shape =
            display::Frame::new(4000., 100.).into()))
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.engine.revision(), rev);
}
