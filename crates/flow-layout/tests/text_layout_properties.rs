use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::{FlowLayout, TextMeasureRequest, TextMeasurer, TextMetrics, TextSizing};
use gpui_flow_scene::{
    DisplayObject, Point, Scene, Shape, Size,
    display::{Text, TextAlign, TextVerticalAlign},
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct Metrics(Mutex<Vec<TextMeasureRequest>>);
impl TextMeasurer for Metrics {
    fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
        self.0.lock().unwrap().push(r.clone());
        Ok(TextMetrics {
            width: 80. + r.letter_spacing + r.word_spacing + r.para_indent,
            height: r.block_height(2),
            line_height: r.line_height,
            lines: 2,
        })
    }
}
fn fixture(
    sizing: TextSizing,
) -> (
    SceneEditor,
    gpui_flow_layout::MeasuredFlowLayout,
    Arc<Metrics>,
) {
    let mut text = Text::new("one\ntwo", Size::new(150., 100.));
    text.font_size = 20.;
    text.line_height = 1.;
    text.padding = [3., 5., 7., 11.];
    text.wrap = sizing != TextSizing::Natural;
    text.auto_size_align = true;
    text.align = TextAlign::Center;
    text.vertical_align = TextVerticalAlign::Middle;
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("t", text), None, None)
        .unwrap();
    let mut layout = FlowLayout::default();
    layout.texts.insert("t".into(), sizing);
    let metrics = Arc::new(Metrics::default());
    (
        SceneEditor::new(scene).unwrap(),
        layout.with_text_measurer(metrics.clone()),
        metrics,
    )
}
fn text(editor: &SceneEditor) -> &Text {
    let Shape::Text(text) = &editor.scene().get(&"t".into()).unwrap().object.shape else {
        panic!()
    };
    text
}
#[test]
fn automatic_alignment_anchors_content_with_asymmetric_padding_and_is_idempotent() {
    for sizing in [TextSizing::Natural, TextSizing::AutoHeight] {
        let (mut editor, layout, _) = fixture(sizing);
        layout.apply_to_editor(&mut editor).unwrap();
        let t = text(&editor);
        assert_eq!(
            t.layout_origin,
            Point::new(
                if sizing == TextSizing::Natural {
                    -51.
                } else {
                    0.
                },
                -23.
            )
        );
        assert_eq!(t.content_bounds().origin.y, -20.);
        assert_eq!(t.size.height, 50.);
        assert_eq!(
            editor.scene().get(&"t".into()).unwrap().object.transform,
            Default::default()
        );
        assert!(
            layout
                .arranged(editor.scene())
                .unwrap()
                .1
                .changed
                .is_empty()
        );
        let encoded = serde_json::to_string(editor.scene()).unwrap();
        assert_eq!(
            serde_json::from_str::<Scene>(&encoded).unwrap(),
            *editor.scene()
        );
    }
}
#[test]
fn inline_layout_reflows_and_history_restores_all_properties_and_origin() {
    let (mut editor, layout, metrics) = fixture(TextSizing::Natural);
    layout.apply_to_editor(&mut editor).unwrap();
    layout.install_reactive(&mut editor.engine).unwrap();
    let before = editor.scene().clone();
    editor
        .transaction(|scene| {
            scene.update_object(&"t".into(), |object| {
                let Shape::Text(t) = &mut object.shape else {
                    panic!()
                };
                t.letter_spacing = 2.;
                t.word_spacing = 3.;
                t.para_indent = 10.;
                t.para_spacing = 4.;
            })
        })
        .unwrap();
    assert_eq!(text(&editor).size, Size::new(111., 54.));
    assert_eq!(text(&editor).layout_origin, Point::new(-58.5, -25.));
    assert!(
        metrics
            .0
            .lock()
            .unwrap()
            .iter()
            .any(|r| r.letter_spacing == 2. && r.word_spacing == 3. && r.para_indent == 10.)
    );
    let after = editor.scene().clone();
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
    editor
        .transaction(|scene| {
            scene.update_object(&"t".into(), |object| {
                let Shape::Text(t) = &mut object.shape else {
                    panic!()
                };
                t.auto_size_align = false;
            })
        })
        .unwrap();
    assert_eq!(text(&editor).layout_origin, Point::ZERO);
    assert_eq!(text(&editor).content, "one\ntwo");
}
#[test]
fn end_bottom_alignment_uses_outer_size_and_invalid_spacing_is_atomic() {
    let (mut editor, layout, _) = fixture(TextSizing::Natural);
    layout
        .transaction_editor(&mut editor, |scene| {
            scene.update_object(&"t".into(), |object| {
                let Shape::Text(t) = &mut object.shape else {
                    panic!()
                };
                t.align = TextAlign::End;
                t.vertical_align = TextVerticalAlign::Bottom;
            })
        })
        .unwrap();
    assert_eq!(text(&editor).layout_origin, Point::new(-96., -50.));
    let before = editor.scene().clone();
    assert!(
        layout
            .transaction_editor(&mut editor, |scene| scene.update_object(
                &"t".into(),
                |object| {
                    let Shape::Text(t) = &mut object.shape else {
                        panic!()
                    };
                    t.letter_spacing = -1.;
                }
            ))
            .is_err()
    );
    assert_eq!(editor.scene(), &before);
}
