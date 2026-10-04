use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::{FlowLayout, TextMeasureRequest, TextMeasurer, TextMetrics, TextSizing};
use gpui_flow_scene::{
    DisplayObject, Point, Scene, Shape, Size,
    display::{Text, TextAlign, TextVerticalAlign, TextWritingMode},
};
use gpui_flow_text::{WritingMode, WritingModeError};
use std::sync::Arc;
struct AxisMetrics;
impl TextMeasurer for AxisMetrics {
    fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
        Ok(TextMetrics {
            width: 40.,
            height: r.block_height(2),
            line_height: r.line_height,
            lines: 2,
        })
    }
    fn measure_writing_mode(
        &self,
        r: &TextMeasureRequest,
        mode: WritingMode,
    ) -> Result<TextMetrics, WritingModeError> {
        let mut m = self.measure(r)?;
        if mode.is_vertical() {
            std::mem::swap(&mut m.width, &mut m.height);
        }
        Ok(m)
    }
}
fn fixture() -> (SceneEditor, FlowLayout) {
    let mut text = Text::new("A\nB", Size::new(100., 100.));
    text.font_size = 20.;
    text.line_height = 1.;
    text.para_spacing = 7.;
    text.padding = [3., 5., 7., 11.];
    text.wrap = false;
    text.writing_mode = TextWritingMode::Y;
    text.auto_size_align = true;
    text.align = TextAlign::Center;
    text.vertical_align = TextVerticalAlign::Middle;
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("t", text), None, None)
        .unwrap();
    let mut layout = FlowLayout::default();
    layout.texts.insert("t".into(), TextSizing::Natural);
    (SceneEditor::new(scene).unwrap(), layout)
}
#[test]
fn vertical_natural_measurement_uses_physical_dimensions_and_atomic_geometry_history() {
    let (mut editor, layout) = fixture();
    let before = editor.scene().clone();
    let measured = layout.with_text_measurer(Arc::new(AxisMetrics));
    measured.apply_to_editor(&mut editor).unwrap();
    let after = editor.scene().clone();
    let Shape::Text(text) = &editor.scene().get(&"t".into()).unwrap().object.shape else {
        unreachable!()
    };
    assert_eq!(text.size, Size::new(63., 50.));
    assert_eq!(text.layout_origin, Point::new(-34.5, -23.));
    assert_eq!(text.content, "A\nB");
    assert!(
        measured
            .arranged(editor.scene())
            .unwrap()
            .1
            .changed
            .is_empty()
    );
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
}
#[test]
fn vertical_wrapped_autoheight_and_legacy_backend_are_explicit_failures_without_mutation() {
    struct Horizontal;
    impl TextMeasurer for Horizontal {
        fn measure(&self, _: &TextMeasureRequest) -> Result<TextMetrics, String> {
            panic!("must not ignore writing mode")
        }
    }
    let (mut editor, mut layout) = fixture();
    let before = editor.scene().clone();
    assert!(
        layout
            .clone()
            .with_text_measurer(Arc::new(Horizontal))
            .apply_to_editor(&mut editor)
            .is_err()
    );
    assert_eq!(editor.scene(), &before);
    layout.texts.insert("t".into(), TextSizing::AutoHeight);
    editor
        .transaction(|scene| {
            scene.update_object(&"t".into(), |object| {
                let Shape::Text(text) = &mut object.shape else {
                    unreachable!()
                };
                text.wrap = true;
            })
        })
        .unwrap();
    let before = editor.scene().clone();
    assert!(
        layout
            .with_text_measurer(Arc::new(AxisMetrics))
            .apply_to_editor(&mut editor)
            .is_err()
    );
    assert_eq!(editor.scene(), &before);
}

#[test]
fn vertical_reverse_auto_start_anchors_the_trailing_physical_edge() {
    let (mut editor, layout) = fixture();
    editor
        .transaction(|scene| {
            scene.update_object(&"t".into(), |object| {
                let Shape::Text(text) = &mut object.shape else {
                    unreachable!()
                };
                text.writing_mode = TextWritingMode::YReverse;
                text.align = TextAlign::Start;
            })
        })
        .unwrap();
    let measured = layout.with_text_measurer(Arc::new(AxisMetrics));
    measured.apply_to_editor(&mut editor).unwrap();
    let Shape::Text(text) = &editor.scene().get(&"t".into()).unwrap().object.shape else {
        unreachable!()
    };
    assert_eq!(text.layout_origin.y, -text.size.height);
    assert_eq!(text.bounds().max_y(), 0.);
}
