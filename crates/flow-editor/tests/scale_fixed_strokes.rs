use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::{display, *};
#[test]
fn fixed_policy_transform_and_current_view_admission_share_atomic_history() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("r", display::Rect::new(40., 30.)).with_style(Style {
            fill: None,
            stroke: Some(Stroke::new(Color::BLACK, 4.).with_alignment(StrokeAlignment::Outside)),
            ..Default::default()
        }),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    let before = e.scene().clone();
    e.transaction(|s| {
        s.update_object(&"r".into(), |o| {
            o.style.stroke.as_mut().unwrap().scale_fixed = StrokeScaleFixed::Threshold(2.);
            o.transform = Affine::scale(2., 3.);
        })
    })
    .unwrap();
    let after = e.scene().clone();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
    e.viewport.zoom = 2.;
    let revision = e.engine.revision();
    assert!(
        e.transaction(|s| s.update_object(&"r".into(), |o| {
            o.style.stroke.as_mut().unwrap().width = MIN_ALIGNED_STROKE_WIDTH * 2.;
            o.style.stroke.as_mut().unwrap().scale_fixed = StrokeScaleFixed::Fixed;
        }))
        .is_err()
    );
    assert_eq!(e.scene(), &after);
    assert_eq!(e.engine.revision(), revision);
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
}
