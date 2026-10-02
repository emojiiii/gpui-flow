use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::{FlowLayout, TextMeasureRequest, TextMeasurer, TextMetrics, TextSizing};
use gpui_flow_scene::{
    DisplayObject, Scene, Shape, Size,
    display::{Text, TextCase},
};
use std::sync::{Arc, Mutex};
#[derive(Default)]
struct Fixture(Mutex<Vec<TextMeasureRequest>>);
impl TextMeasurer for Fixture {
    fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
        self.0.lock().unwrap().push(r.clone());
        let content = r.text_case.presentation(&r.content)?;
        // Fixed contract fixtures, not a replacement font engine.
        let width = if content.starts_with("SS") { 90. } else { 50. };
        let lines = r.paragraph_breaks() + 1 + usize::from(r.wrap_width.is_some());
        Ok(TextMetrics {
            width,
            height: r.block_height(lines),
            line_height: r.line_height,
            lines,
        })
    }
}
fn fixture(
    sizing: TextSizing,
) -> (
    SceneEditor,
    gpui_flow_layout::MeasuredFlowLayout,
    Arc<Fixture>,
) {
    let mut s = Scene::new();
    let mut t = Text::new("ß\nend\n", Size::new(120., 100.));
    t.font_size = 20.;
    t.line_height = 1.;
    t.padding = [3.; 4];
    t.wrap = sizing != TextSizing::Natural;
    s.insert(DisplayObject::new("t", t), None, None).unwrap();
    let mut l = FlowLayout::default();
    l.texts.insert("t".into(), sizing);
    let backend = Arc::new(Fixture::default());
    let measured = l.with_text_measurer(backend.clone());
    let mut e = SceneEditor::new(s).unwrap();
    measured.apply_to_editor(&mut e).unwrap();
    measured.install_reactive(&mut e.engine).unwrap();
    (e, measured, backend)
}
fn text(e: &SceneEditor) -> &Text {
    let Shape::Text(t) = &e.scene().get(&"t".into()).unwrap().object.shape else {
        panic!()
    };
    t
}
#[test]
fn paragraph_styles_remeasure_once_and_undo_redo_keep_canonical_content() {
    for sizing in [TextSizing::Natural, TextSizing::AutoHeight] {
        let (mut e, measured, backend) = fixture(sizing);
        let before = e.scene().clone();
        e.transaction(|s| {
            s.update_object(&"t".into(), |o| {
                if let Shape::Text(t) = &mut o.shape {
                    t.text_case = TextCase::Upper;
                    t.para_spacing = 7.;
                }
            })
        })
        .unwrap();
        assert_eq!(text(&e).content, "ß\nend\n");
        assert_eq!(
            text(&e).size.height,
            if sizing == TextSizing::Natural {
                80.
            } else {
                100.
            }
        );
        if sizing == TextSizing::Natural {
            assert_eq!(text(&e).size.width, 96.);
        }
        assert!(
            backend
                .0
                .lock()
                .unwrap()
                .iter()
                .any(|r| r.content == "ß\nend\n"
                    && r.para_spacing == 7.
                    && r.text_case == gpui_flow_text::TextCase::Upper)
        );
        assert!(measured.arranged(e.scene()).unwrap().1.changed.is_empty());
        let after = e.scene().clone();
        assert!(e.undo());
        assert_eq!(e.scene(), &before);
        assert!(e.redo());
        assert_eq!(e.scene(), &after);
    }
}
#[test]
fn paragraph_spacing_or_conversion_failure_preserves_scene_revision_and_history() {
    let (mut e, _, _) = fixture(TextSizing::Natural);
    let before = e.scene().clone();
    let revision = e.engine.revision();
    for too_large in [false, true] {
        assert!(
            e.transaction(|s| s.update_object(&"t".into(), |o| {
                if let Shape::Text(t) = &mut o.shape {
                    if too_large {
                        t.content = "İ".repeat(32_768);
                        t.text_case = TextCase::Lower;
                    } else {
                        t.para_spacing = -1.;
                    }
                }
            }))
            .is_err()
        );
        assert_eq!(e.scene(), &before);
        assert_eq!(e.engine.revision(), revision);
    }
}
#[test]
fn inconsistent_paragraph_metric_backend_cannot_commit() {
    struct Bad;
    impl TextMeasurer for Bad {
        fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
            Ok(TextMetrics {
                width: 10.,
                height: r.line_height * 3.,
                line_height: r.line_height,
                lines: 3,
            })
        }
    }
    let (mut e, measured, _) = fixture(TextSizing::Natural);
    let mut s = e.scene().clone();
    s.update_object(&"t".into(), |o| {
        if let Shape::Text(t) = &mut o.shape {
            t.para_spacing = 4.;
        }
    })
    .unwrap();
    e.load_scene(s).unwrap();
    let before = e.scene().clone();
    let bad = measured.layout().clone().with_text_measurer(Arc::new(Bad));
    assert!(bad.apply_to_editor(&mut e).is_err());
    assert_eq!(e.scene(), &before);
}
