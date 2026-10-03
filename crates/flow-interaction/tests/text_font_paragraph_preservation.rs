//! Cross-slice guard: intrinsic font tools must not replay unrelated typography.
use gpui_flow_editor::{ResizeOptions, SceneEditor, TextFontResizeGeometry};
use gpui_flow_interaction::scene::{SceneCommand, ScenePluginHost};
use gpui_flow_scene::{
    Affine, DisplayObject, Point, Scene, Shape, Size,
    display::{Text, TextCase},
};

#[test]
fn font_resize_and_history_preserve_case_spacing_line_height_and_canonical_content() {
    let mut text = Text::new("mixed Case\nß stays canonical", Size::new(160., 80.));
    text.text_case = TextCase::Upper;
    text.para_spacing = 9.;
    text.letter_spacing = 2.;
    text.word_spacing = 3.;
    text.para_indent = 7.;
    text.auto_size_align = true;
    text.layout_origin = Point::new(-10., -5.);
    text.line_height = 1.7;
    text.padding = [4.; 4];
    let result = TextFontResizeGeometry::new(&text)
        .unwrap()
        .resize(
            Affine::IDENTITY,
            Point::ZERO,
            Point::new(40., 20.),
            ResizeOptions::default(),
        )
        .unwrap();
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("text", text.clone()), None, None)
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    let handle = editor.engine.handle(&"text".into()).unwrap();
    ScenePluginHost::with_defaults()
        .execute(
            &mut editor,
            SceneCommand::SetTextFontResize {
                target: handle,
                resize: result,
            },
        )
        .unwrap();
    let Shape::Text(after) = &editor.scene().get(&"text".into()).unwrap().object.shape else {
        panic!()
    };
    assert_ne!(after.font_size, text.font_size);
    assert_eq!(after.text_case, text.text_case);
    assert_eq!(after.para_spacing, text.para_spacing);
    assert_eq!(after.letter_spacing, text.letter_spacing);
    assert_eq!(after.word_spacing, text.word_spacing);
    assert_eq!(after.para_indent, text.para_indent);
    assert_eq!(after.auto_size_align, text.auto_size_align);
    assert_eq!(after.layout_origin, text.layout_origin);
    assert_eq!(after.line_height, text.line_height);
    assert_eq!(after.content, text.content);
    assert!(editor.undo());
    assert_eq!(
        &editor.scene().get(&"text".into()).unwrap().object.shape,
        &Shape::Text(text)
    );
}
