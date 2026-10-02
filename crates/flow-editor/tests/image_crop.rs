use gpui_flow_editor::{ImageCropGeometry, ImageCropPart, ResizeHandle, SceneEditor};
use gpui_flow_engine::PropertyKey;
use gpui_flow_scene::{Bounds, DisplayObject, Point, Scene, Size, display::Image};
fn crop(x: f32, y: f32, w: f32, h: f32) -> Bounds {
    Bounds::new(Point::new(x, y), Size::new(w, h))
}
#[test]
fn crop_frames_move_clamp_and_resize_source_edges_without_flipping() {
    let mut image = Image::new("test", Size::new(200., 100.));
    image.source_crop = Some(crop(0.25, 0.25, 0.5, 0.5));
    let g = ImageCropGeometry::new(&image).unwrap();
    assert_eq!(g.frame(), crop(50., 25., 100., 50.));
    assert_eq!(
        g.drag(
            ImageCropPart::Move,
            Point::ZERO,
            Point::new(400., -200.),
            false
        ),
        Some(crop(0.5, 0., 0.5, 0.5))
    );
    assert_eq!(
        g.drag(
            ImageCropPart::Handle(ResizeHandle::Right),
            Point::ZERO,
            Point::new(100., 0.),
            false
        ),
        Some(crop(0.25, 0.25, 0.75, 0.5))
    );
    let small = g
        .drag(
            ImageCropPart::Handle(ResizeHandle::TopLeft),
            Point::ZERO,
            Point::new(999., 999.),
            false,
        )
        .unwrap();
    assert!(small.size.is_positive());
    assert!(small.size.width < 0.001);
    assert_eq!(
        g.drag(
            ImageCropPart::Move,
            Point::ZERO,
            Point::new(f32::NAN, 0.),
            false
        ),
        None
    );
    let uniform = g
        .drag(
            ImageCropPart::Handle(ResizeHandle::BottomRight),
            Point::ZERO,
            Point::new(30., 5.),
            true,
        )
        .unwrap();
    assert_eq!(uniform.size.width, uniform.size.height);
}
#[test]
fn crop_typed_property_has_atomic_validation_and_history() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("i", Image::new("test", Size::new(100., 100.))),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    let h = e.engine.handle(&"i".into()).unwrap();
    let before = e.scene().clone();
    e.set_property(
        h,
        PropertyKey::ImageSourceCrop,
        serde_json::json!(crop(0.25, 0., 0.75, 1.)),
    )
    .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
    assert!(
        e.set_property(
            h,
            PropertyKey::ImageSourceCrop,
            serde_json::json!(crop(-1., 0., 1., 1.))
        )
        .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    assert_eq!(e.engine.handle(&"i".into()), Some(h));
}
