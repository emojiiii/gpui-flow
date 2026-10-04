use gpui_flow_editor::{ResizeHandle, SceneEditor};
use gpui_flow_interaction::{
    scene::{
        plugins::{SceneResizePlugin, TextResizeMode},
        *,
    },
    *,
};
use gpui_flow_layout::{FlowLayout, TextMeasureRequest, TextMeasurer, TextMetrics, TextSizing};
use gpui_flow_scene::{
    DisplayObject, Point, Scene, Shape, Size,
    display::{Text, TextAlign, TextWritingMode},
};
use gpui_flow_text::{WritingMode, WritingModeError};
use std::sync::Arc;

struct AxisMetrics;
impl TextMeasurer for AxisMetrics {
    fn measure(&self, _: &TextMeasureRequest) -> Result<TextMetrics, String> {
        panic!("vertical text needs axis-aware measurement")
    }
    fn measure_writing_mode(
        &self,
        r: &TextMeasureRequest,
        mode: WritingMode,
    ) -> Result<TextMetrics, WritingModeError> {
        assert!(mode.is_vertical());
        let height = match r.content.as_str() {
            "ab" => r.font_size * 2.,
            "abcdef\n" => r.font_size * 6.,
            "" => r.font_size,
            "unavailable" => return Err("fixture font unavailable".into()),
            _ => panic!("unknown fixture"),
        };
        let extra_columns = r
            .wrap_width
            .map_or(0, |limit| (height / limit).ceil().max(1.) as usize - 1);
        let lines = r.paragraph_breaks() + 1 + extra_columns;
        Ok(TextMetrics {
            width: r.block_height(lines),
            height: r.wrap_width.map_or(height, |limit| height.min(limit)),
            line_height: r.line_height,
            lines,
        })
    }
}
fn fixture(mode: TextWritingMode) -> (SceneEditor, ScenePluginHost) {
    fixture_sized(mode, TextSizing::AutoHeight)
}
fn fixture_sized(mode: TextWritingMode, sizing: TextSizing) -> (SceneEditor, ScenePluginHost) {
    let mut t = Text::new("ab", Size::new(120., 100.));
    t.wrap = false;
    t.writing_mode = mode;
    t.font_size = 20.;
    t.line_height = 1.;
    t.padding = [3., 5., 7., 11.];
    t.auto_size_align = true;
    t.align = TextAlign::Center;
    if sizing == TextSizing::AutoWidth {
        t.wrap = true;
        t.size.height = 50.;
        t.vertical_align = gpui_flow_scene::display::TextVerticalAlign::Middle;
    }
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("t", t), None, None)
        .unwrap();
    let mut layout = FlowLayout::default();
    layout.texts.insert("t".into(), sizing);
    let measured = layout.with_text_measurer(Arc::new(AxisMetrics));
    measured.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    measured.install_reactive(&mut editor.engine).unwrap();
    editor.selection.select("t".into(), false);
    let mut host = ScenePluginHost::with_defaults();
    host.replace(
        SceneResizePlugin::default().with_text_resize_mode(TextResizeMode::Box),
        &mut editor,
    )
    .unwrap();
    (editor, host)
}
fn text(editor: &SceneEditor) -> &Text {
    let Shape::Text(t) = &editor.scene().get(&"t".into()).unwrap().object.shape else {
        panic!()
    };
    t
}
fn dispatch(
    editor: &mut SceneEditor,
    host: &mut ScenePluginHost,
    router: &mut SceneEventRouter,
    event: CanvasEvent,
) -> RoutedReport {
    let report = router.dispatch(editor, host, event);
    assert!(report.errors.is_empty(), "{:?}", report.errors);
    report
}
fn down(
    editor: &mut SceneEditor,
    host: &mut ScenePluginHost,
    router: &mut SceneEventRouter,
    handle: ResizeHandle,
) -> Point {
    let geometry = host
        .resize_geometry(editor, &"t".into(), &editor.engine)
        .unwrap();
    let p = editor.viewport.world_to_screen(
        geometry
            .world
            .transform_point(handle.position(geometry.bounds)),
    );
    let r = dispatch(
        editor,
        host,
        router,
        CanvasEvent::PointerDown(PointerInput::left(
            p,
            HitTarget::ObjectResizeHandle {
                id: "t".into(),
                handle,
            },
        )),
    );
    assert!(r.default.errors.is_empty(), "{:?}", r.default.errors);
    assert!(host.has_capture());
    p
}
#[test]
fn vertical_autoheight_box_gestures_accept_width_but_atomically_reject_height() {
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        let (mut editor, mut host) = fixture(mode);
        let before = editor.scene().clone();
        let mut router = SceneEventRouter::new();
        let p = down(&mut editor, &mut host, &mut router, ResizeHandle::Right);
        let r = dispatch(
            &mut editor,
            &mut host,
            &mut router,
            CanvasEvent::PointerMove(PointerInput::left(
                p + Point::new(40., 0.),
                HitTarget::Background,
            )),
        );
        assert!(r.default.errors.is_empty(), "{:?}", r.default.errors);
        let r = dispatch(
            &mut editor,
            &mut host,
            &mut router,
            CanvasEvent::PointerUp(PointerInput::left(
                p + Point::new(40., 0.),
                HitTarget::Background,
            )),
        );
        assert!(r.default.errors.is_empty());
        assert!(!host.has_capture());
        assert_eq!(text(&editor).size, Size::new(160., 50.));
        assert_eq!(text(&editor).layout_origin, Point::new(0., -23.));
        assert_eq!(text(&editor).content, "ab");
        let after = editor.scene().clone();
        assert!(editor.undo());
        assert_eq!(editor.scene(), &before);
        assert!(!editor.undo());
        assert!(editor.redo());
        assert_eq!(editor.scene(), &after);
        let revision = editor.engine.revision();
        let p = down(&mut editor, &mut host, &mut router, ResizeHandle::Bottom);
        let r = dispatch(
            &mut editor,
            &mut host,
            &mut router,
            CanvasEvent::PointerMove(PointerInput::left(
                p + Point::new(0., 25.),
                HitTarget::Background,
            )),
        );
        assert!(!r.default.errors.is_empty());
        assert!(!host.has_capture());
        assert_eq!(editor.scene(), &after);
        assert_eq!(editor.engine.revision(), revision);
        assert!(editor.undo());
        assert_eq!(editor.scene(), &before);
        assert!(!editor.undo());
    }
}
#[test]
fn vertical_autoheight_text_tool_updates_cancel_and_history_share_derived_geometry() {
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        let (mut editor, mut host) = fixture(mode);
        let mut tools = SceneToolController::with_text();
        let target = editor.engine.handle(&"t".into()).unwrap();
        let before = editor.scene().clone();
        let token = tools.open_text(&mut editor, &mut host, target).unwrap();
        tools
            .set_text(&mut editor, &mut host, token, "abcdef\n")
            .unwrap();
        assert_eq!(text(&editor).size, Size::new(120., 130.));
        assert_eq!(text(&editor).layout_origin, Point::new(0., -63.));
        let revision = editor.engine.revision();
        tools
            .set_text(&mut editor, &mut host, token, "abcdef\n")
            .unwrap();
        assert_eq!(editor.engine.revision(), revision);
        let after = editor.scene().clone();
        assert!(
            tools
                .set_text(&mut editor, &mut host, token, "unavailable")
                .is_err()
        );
        assert_eq!(editor.scene(), &after);
        assert_eq!(editor.engine.revision(), revision);
        tools.finish(&mut editor, &mut host, token).unwrap();
        assert!(editor.undo());
        assert_eq!(editor.scene(), &before);
        assert!(!editor.undo());
        let token = tools.open_text(&mut editor, &mut host, target).unwrap();
        tools.set_text(&mut editor, &mut host, token, "").unwrap();
        assert_eq!(text(&editor).size, Size::new(120., 30.));
        tools.cancel(&mut editor, &mut host, token).unwrap();
        assert_eq!(editor.scene(), &before);
        assert!(editor.redo());
        assert_eq!(editor.scene(), &after);
    }
}

#[test]
fn vertical_autowidth_box_resize_accepts_height_and_rejects_width_atomically() {
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        let (mut editor, mut host) = fixture_sized(mode, TextSizing::AutoWidth);
        let before = editor.scene().clone();
        let mut router = SceneEventRouter::new();
        let p = down(&mut editor, &mut host, &mut router, ResizeHandle::Bottom);
        let r = dispatch(
            &mut editor,
            &mut host,
            &mut router,
            CanvasEvent::PointerMove(PointerInput::left(
                p + Point::new(0., 40.),
                HitTarget::Background,
            )),
        );
        assert!(r.default.errors.is_empty(), "{:?}", r.default.errors);
        let r = dispatch(
            &mut editor,
            &mut host,
            &mut router,
            CanvasEvent::PointerUp(PointerInput::left(
                p + Point::new(0., 40.),
                HitTarget::Background,
            )),
        );
        assert!(r.default.errors.is_empty());
        assert!(!host.has_capture());
        assert_eq!(text(&editor).size, Size::new(36., 90.));
        assert_eq!(text(&editor).layout_origin, Point::new(-21., 0.));
        let after = editor.scene().clone();
        assert!(editor.undo());
        assert_eq!(editor.scene(), &before);
        assert!(!editor.undo());
        assert!(editor.redo());
        assert_eq!(editor.scene(), &after);
        let revision = editor.engine.revision();
        let p = down(&mut editor, &mut host, &mut router, ResizeHandle::Right);
        let r = dispatch(
            &mut editor,
            &mut host,
            &mut router,
            CanvasEvent::PointerMove(PointerInput::left(
                p + Point::new(25., 0.),
                HitTarget::Background,
            )),
        );
        assert!(!r.default.errors.is_empty());
        assert!(!host.has_capture());
        assert_eq!(editor.scene(), &after);
        assert_eq!(editor.engine.revision(), revision);
        assert!(editor.undo());
        assert_eq!(editor.scene(), &before);
        assert!(!editor.undo());
    }
}
#[test]
fn vertical_autowidth_text_tool_reflows_columns_with_cancel_and_history() {
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        let (mut editor, mut host) = fixture_sized(mode, TextSizing::AutoWidth);
        let mut tools = SceneToolController::with_text();
        let target = editor.engine.handle(&"t".into()).unwrap();
        let before = editor.scene().clone();
        let token = tools.open_text(&mut editor, &mut host, target).unwrap();
        tools
            .set_text(&mut editor, &mut host, token, "abcdef\n")
            .unwrap();
        assert_eq!(text(&editor).size, Size::new(96., 50.));
        assert_eq!(text(&editor).layout_origin, Point::new(-51., 0.));
        let after = editor.scene().clone();
        let revision = editor.engine.revision();
        tools
            .set_text(&mut editor, &mut host, token, "abcdef\n")
            .unwrap();
        assert_eq!(editor.engine.revision(), revision);
        assert!(
            tools
                .set_text(&mut editor, &mut host, token, "unavailable")
                .is_err()
        );
        assert_eq!(editor.scene(), &after);
        assert_eq!(editor.engine.revision(), revision);
        tools.finish(&mut editor, &mut host, token).unwrap();
        assert!(editor.undo());
        assert_eq!(editor.scene(), &before);
        assert!(!editor.undo());
        let token = tools.open_text(&mut editor, &mut host, target).unwrap();
        tools.set_text(&mut editor, &mut host, token, "").unwrap();
        assert_eq!(text(&editor).size, Size::new(36., 50.));
        tools.cancel(&mut editor, &mut host, token).unwrap();
        assert_eq!(editor.scene(), &before);
        assert!(editor.redo());
        assert_eq!(editor.scene(), &after);
    }
}

#[test]
fn autowidth_fixed_box_rewrap_conflict_is_rejected_but_matching_font_resize_is_allowed() {
    for mode in [TextWritingMode::Y, TextWritingMode::YReverse] {
        let (mut editor, mut host) = fixture_sized(mode, TextSizing::AutoWidth);
        let mut tools = SceneToolController::with_text();
        let target = editor.engine.handle(&"t".into()).unwrap();
        let token = tools.open_text(&mut editor, &mut host, target).unwrap();
        tools
            .set_text(&mut editor, &mut host, token, "abcdef\n")
            .unwrap();
        tools.finish(&mut editor, &mut host, token).unwrap();
        let before = editor.scene().clone();
        let revision = editor.engine.revision();
        let mut router = SceneEventRouter::new();
        let p = down(&mut editor, &mut host, &mut router, ResizeHandle::Bottom);
        // 40 -> 80 content height reduces the columns from four to three.
        // A fixed-box command cannot also replace its requested width.
        let r = dispatch(
            &mut editor,
            &mut host,
            &mut router,
            CanvasEvent::PointerMove(PointerInput::left(
                p + Point::new(0., 40.),
                HitTarget::Background,
            )),
        );
        assert!(!r.default.errors.is_empty());
        assert!(!host.has_capture());
        assert_eq!(editor.scene(), &before);
        assert_eq!(editor.engine.revision(), revision);
        host.replace(
            SceneResizePlugin::default().with_text_resize_mode(TextResizeMode::FontSize),
            &mut editor,
        )
        .unwrap();
        let p = down(&mut editor, &mut host, &mut router, ResizeHandle::Bottom);
        let r = dispatch(
            &mut editor,
            &mut host,
            &mut router,
            CanvasEvent::PointerMove(PointerInput::left(
                p + Point::new(0., 50.),
                HitTarget::Background,
            )),
        );
        assert!(r.default.errors.is_empty(), "{:?}", r.default.errors);
        let r = dispatch(
            &mut editor,
            &mut host,
            &mut router,
            CanvasEvent::PointerUp(PointerInput::left(
                p + Point::new(0., 50.),
                HitTarget::Background,
            )),
        );
        assert!(r.default.errors.is_empty());
        assert_eq!(text(&editor).size, Size::new(192., 100.));
        assert_eq!(text(&editor).font_size, 40.);
        assert_eq!(text(&editor).padding, [6., 10., 14., 22.]);
        assert!(editor.undo());
        assert_eq!(editor.scene(), &before);
    }
}
