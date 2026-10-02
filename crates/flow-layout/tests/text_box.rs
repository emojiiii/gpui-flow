use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::*;
use gpui_flow_scene::{DisplayObject, Scene, Shape, Size, display::Text};
use std::sync::{Arc, Mutex};
struct Metrics(Mutex<Vec<TextMeasureRequest>>);
impl TextMeasurer for Metrics {
    fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
        self.0.lock().unwrap().push(r.clone());
        let lines = if r.wrap_width.is_some_and(|w| w < 80.) {
            2
        } else {
            1
        };
        Ok(TextMetrics {
            width: 80.,
            height: r.line_height * lines as f32,
            line_height: r.line_height,
            lines,
        })
    }
}
fn fixture(sizing: TextSizing) -> (Scene, FlowLayout, Arc<Metrics>) {
    let mut s = Scene::new();
    let mut t = Text::new("fixture", Size::new(100., 60.));
    t.font_size = 20.;
    t.line_height = 1.;
    t.padding = [5., 15., 7., 15.];
    t.wrap = sizing != TextSizing::Natural;
    s.insert(DisplayObject::new("t", t), None, None).unwrap();
    let mut layout = FlowLayout::default();
    layout.texts.insert("t".into(), sizing);
    (s, layout, Arc::new(Metrics(Mutex::new(vec![]))))
}
fn size(s: &Scene) -> Size {
    s.get(&"t".into())
        .unwrap()
        .object
        .shape
        .local_bounds()
        .unwrap()
        .size
}
#[test]
fn natural_measurement_adds_padding_once_and_converges() {
    let (mut s, l, m) = fixture(TextSizing::Natural);
    let measured = l.with_text_measurer(m.clone());
    measured.apply_to_scene(&mut s).unwrap();
    assert_eq!(size(&s), Size::new(110., 32.));
    assert!(m.0.lock().unwrap().iter().all(|r| r.wrap_width.is_none()));
    assert!(measured.arranged(&s).unwrap().1.changed.is_empty());
}
#[test]
fn auto_height_shapes_at_inner_width_and_reactive_padding_edit_is_one_undo() {
    let (mut s, l, m) = fixture(TextSizing::AutoHeight);
    let measured = l.with_text_measurer(m.clone());
    measured.apply_to_scene(&mut s).unwrap();
    assert_eq!(size(&s), Size::new(100., 52.));
    assert!(
        m.0.lock()
            .unwrap()
            .iter()
            .all(|r| r.wrap_width == Some(70.))
    );
    let mut e = SceneEditor::new(s).unwrap();
    measured.install_reactive(&mut e.engine).unwrap();
    let before = e.scene().clone();
    e.transaction(|s| {
        s.update_object(&"t".into(), |o| {
            if let Shape::Text(t) = &mut o.shape {
                t.padding = [5., 5., 7., 5.];
            }
        })
    })
    .unwrap();
    assert_eq!(size(e.scene()), Size::new(100., 32.));
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
}
#[test]
fn invalid_padding_cannot_partially_commit_reactive_measurement() {
    let (mut s, l, m) = fixture(TextSizing::AutoHeight);
    let measured = l.with_text_measurer(m);
    measured.apply_to_scene(&mut s).unwrap();
    let mut e = SceneEditor::new(s).unwrap();
    measured.install_reactive(&mut e.engine).unwrap();
    let before = e.scene().clone();
    assert!(
        e.transaction(
            |s| s.update_object(&"t".into(), |o| if let Shape::Text(t) = &mut o.shape {
                t.padding = [100.; 4];
            })
        )
        .is_err()
    );
    assert_eq!(e.scene(), &before);
}

#[test]
fn ellipsis_never_truncates_intrinsic_measurement_or_changes_measured_size() {
    for sizing in [TextSizing::Natural, TextSizing::AutoHeight] {
        let (mut clipped, l, m) = fixture(sizing);
        let mut ellipsis = clipped.clone();
        ellipsis
            .update_object(&"t".into(), |o| {
                if let Shape::Text(t) = &mut o.shape {
                    t.overflow = gpui_flow_scene::display::TextOverflow::Ellipsis;
                }
            })
            .unwrap();
        let measured = l.with_text_measurer(m.clone());
        measured.apply_to_scene(&mut clipped).unwrap();
        measured.apply_to_scene(&mut ellipsis).unwrap();
        assert_eq!(size(&clipped), size(&ellipsis));
        assert!(m.0.lock().unwrap().iter().all(|r| r.content == "fixture"));
    }
}
