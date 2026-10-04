//! Scripted adapter coverage using a real Linux Window and Kit Textarea.
//! These handler calls do not exercise an OS input method or candidate popup.
use super::*;
use core::prelude::v1::test;
use gpui_flow_engine::ElementHandle;
use gpui_flow_interaction::{
    CanvasEffect, CanvasPlugin, PluginContext,
    scene::{SceneCommand, ToolLifecycleKind},
};
use gpui_flow_scene::{DisplayObject, Shape, Size, display};
use std::{
    cell::{Cell, RefCell},
    ops::Range,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
    rc::Rc,
};

const BASELINE: &str = "A🙂";

struct RejectPreedit;
impl CanvasPlugin<SceneEditor, SceneCommand> for RejectPreedit {
    fn id(&self) -> &str {
        "reject-native-test-preedit"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetSessionProperty { value, .. } = command
            && value.as_str() == Some("A🙂ni")
        {
            return Err("preedit is not an accepted document value".into());
        }
        Ok(())
    }
}

fn fixture_scene() -> Scene {
    let mut scene = Scene::new();
    for (id, value) in [("a", BASELINE), ("b", "Other text")] {
        scene
            .insert(
                DisplayObject::new(id, display::Text::new(value, Size::new(160., 40.))),
                None,
                None,
            )
            .unwrap();
    }
    scene
        .insert(
            DisplayObject::new(
                "image",
                display::Image::new("unused-ime-adapter-fixture.png", Size::new(100., 80.)),
            ),
            None,
            None,
        )
        .unwrap();
    scene
}

fn text(view: &SceneView) -> &str {
    let Shape::Text(text) = &view.editor.scene().get(&"a".into()).unwrap().object.shape else {
        panic!("fixture lost Text target");
    };
    &text.content
}

fn snapshot(
    input: &Entity<TextareaState>,
    window: &mut Window,
    cx: &mut App,
) -> (SharedString, Option<Range<usize>>, Range<usize>) {
    input.update(cx, |input, cx| {
        (
            input.value(),
            input.marked_text_range(window, cx),
            input.selected_range(),
        )
    })
}

fn begin_preedit(
    view: &mut SceneView,
    preedit: &str,
    window: &mut Window,
    cx: &mut Context<SceneView>,
) -> (TextSessionToken, Entity<TextareaState>) {
    assert!(!view.text_editor_active());
    view.close_image_crop_editor(cx);
    view.set_scene(fixture_scene(), cx).unwrap();
    let a = view.editor.engine.handle(&"a".into()).unwrap();
    let token = view.open_text_editor(a, window, cx).unwrap();
    let input = view.text_input.as_ref().unwrap().state.clone();
    input.update(cx, |input, cx| {
        input.set_selected_range(BASELINE.len()..BASELINE.len(), cx);
        input.replace_and_mark_text_in_range(None, preedit, None, window, cx);
    });
    assert_eq!(text(view), BASELINE, "preedit is still input-local");
    (token, input)
}

fn assert_single_undo(view: &mut SceneView, accepted: &str) {
    assert_eq!(text(view), accepted);
    assert!(view.editor.undo());
    assert_eq!(text(view), BASELINE);
    assert!(
        !view.editor.undo(),
        "session created more than one undo step"
    );
    assert!(view.editor.redo());
    assert_eq!(text(view), accepted);
}

fn target(view: &SceneView, id: &str) -> ElementHandle {
    view.editor.engine.handle(&id.into()).unwrap()
}

fn exercise_adapter(
    view: &mut SceneView,
    window: &mut Window,
    cx: &mut Context<SceneView>,
) -> Vec<(TextSessionToken, ToolCloseReason)> {
    view.install_plugin(RejectPreedit, cx).unwrap();
    let mut expected_closes = Vec::new();

    let (token, input) = begin_preedit(view, "ni", window, cx);
    let before = snapshot(&input, window, cx);
    assert_eq!(before.0, "A🙂ni");
    assert_eq!(before.1, Some(3..5), "marked ranges use UTF-16 offsets");
    let document = view.editor.scene().clone();
    for reason in [ToolCloseReason::Done, ToolCloseReason::Blur] {
        assert!(!view.close_text_editor(false, reason, window, cx));
        assert_eq!(view.text_input.as_ref().unwrap().token, token);
        assert_eq!(view.text_input.as_ref().unwrap().state, input);
        assert_eq!(snapshot(&input, window, cx), before);
        assert_eq!(view.editor.scene(), &document);
        assert!(view.interaction_error.is_some());
    }
    assert!(
        view.open_text_editor(target(view, "b"), window, cx)
            .is_err()
    );
    assert!(view.flush_text_for_tool_switch(window, cx).is_err());
    assert_eq!(snapshot(&input, window, cx), before);
    // This must replace "ni", not insert after the caret left by a failed close.
    input.update(cx, |input, cx| {
        input.replace_text_in_range(None, "你", window, cx);
    });
    assert_eq!(input.read(cx).value(), "A🙂你");
    assert!(view.finish_text_editor(window, cx));
    assert!(!view.finish_text_editor(window, cx));
    assert_eq!(snapshot(&input, window, cx).1, None);
    assert_single_undo(view, "A🙂你");
    expected_closes.push((token, ToolCloseReason::Done));

    let (token, input) = begin_preedit(view, "中文", window, cx);
    assert!(view.finish_text_editor(window, cx));
    assert_eq!(snapshot(&input, window, cx).1, None);
    assert_single_undo(view, "A🙂中文");
    expected_closes.push((token, ToolCloseReason::Done));

    let (token, input) = begin_preedit(view, "取消", window, cx);
    assert!(view.cancel_text_editor(window, cx));
    assert!(!view.cancel_text_editor(window, cx));
    assert_eq!(snapshot(&input, window, cx).1, None);
    assert_eq!(text(view), BASELINE);
    assert!(!view.editor.undo(), "cancelled preedit created history");
    expected_closes.push((token, ToolCloseReason::Cancelled));

    view.set_text_escape_behavior(gpui_flow_interaction::scene::TextEscapeBehavior::Finish, cx);
    let (token, input) = begin_preedit(view, "保留", window, cx);
    assert!(view.escape_text_editor(window, cx));
    assert!(!view.escape_text_editor(window, cx));
    assert_eq!(snapshot(&input, window, cx).1, None);
    assert_single_undo(view, "A🙂保留");
    expected_closes.push((token, ToolCloseReason::Done));

    // Explicit Cancel still rolls back under the finish-on-Escape profile.
    let (token, input) = begin_preedit(view, "取消", window, cx);
    assert!(view.cancel_text_editor(window, cx));
    assert_eq!(snapshot(&input, window, cx).1, None);
    assert_eq!(text(view), BASELINE);
    assert!(!view.editor.undo());
    expected_closes.push((token, ToolCloseReason::Cancelled));
    view.set_text_escape_behavior(gpui_flow_interaction::scene::TextEscapeBehavior::Cancel, cx);
    let (token, _) = begin_preedit(view, "取消", window, cx);
    assert!(view.escape_text_editor(window, cx));
    assert_eq!(text(view), BASELINE);
    expected_closes.push((token, ToolCloseReason::Cancelled));

    let (token, input) = begin_preedit(view, "wen", window, cx);
    let before = snapshot(&input, window, cx);
    let document = view.editor.scene().clone();
    let selection = view.editor.selection.clone();
    // Invalid target validation and valid-target composition rejection both
    // leave the complete old text session untouched.
    for id in ["b", "image"] {
        assert!(
            view.open_image_crop_editor(target(view, id), window, cx)
                .is_err()
        );
        assert!(!view.image_crop_editor_active());
        assert_eq!(view.text_input.as_ref().unwrap().token, token);
        assert_eq!(snapshot(&input, window, cx), before);
        assert_eq!(view.editor.scene(), &document);
        assert_eq!(view.editor.selection, selection);
    }
    input.update(cx, |input, cx| {
        input.replace_text_in_range(None, "文", window, cx);
    });
    let image = target(view, "image");
    view.open_image_crop_editor(image, window, cx).unwrap();
    assert_eq!(view.image_crop_target(), Some(image));
    assert!(!view.text_editor_active());
    assert_eq!(snapshot(&input, window, cx).1, None);
    assert_single_undo(view, "A🙂文");
    view.close_image_crop_editor(cx);
    expected_closes.push((token, ToolCloseReason::TargetSwitch));
    expected_closes
}

#[test]
#[ignore = "opens a Linux native window; scripted adapter calls, not OS IME candidate input"]
fn ime_close_rejection_and_crop_switch() {
    let completed = Rc::new(Cell::new(false));
    let completed_in_app = completed.clone();
    let outcome = Rc::new(RefCell::new(None));
    let outcome_in_app = outcome.clone();
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let (window, view) =
                gpui_kit::open_window(WindowOptions::default(), cx, |window, cx| {
                    window.set_window_title("gpui-flow · scripted IME adapter regression");
                    cx.new(|cx| SceneView::new(fixture_scene(), cx).unwrap())
                })
                .unwrap();
            let closes = Rc::new(RefCell::new(Vec::new()));
            let observed_closes = closes.clone();
            cx.subscribe(&view, move |_, event: &SceneViewEvent, _| {
                if let SceneViewEvent::EditorTool(event) = event
                    && event.tool_id == "text"
                    && let (Some(token), ToolLifecycleKind::Unloaded(reason)) =
                        (event.token, event.kind)
                {
                    observed_closes.borrow_mut().push((token, reason));
                }
            })
            .detach();
            window
                .update(cx, move |_, window, _| {
                    window.on_next_frame(move |window, cx| {
                        let result = catch_unwind(AssertUnwindSafe(|| {
                            view.update(cx, |view, cx| exercise_adapter(view, window, cx))
                        }));
                        let expected = match result {
                            Ok(expected) => expected,
                            Err(error) => {
                                *outcome_in_app.borrow_mut() = Some(Err(error));
                                cx.quit();
                                return;
                            }
                        };
                        // Let the actual queued SceneView events dispatch before
                        // checking their count/order. A swallowed callback panic
                        // or an early quit cannot turn this test into a pass.
                        cx.defer(move |cx| {
                            *outcome_in_app.borrow_mut() =
                                Some(catch_unwind(AssertUnwindSafe(|| {
                                    assert_eq!(*closes.borrow(), expected);
                                    completed_in_app.set(true);
                                })));
                            cx.quit();
                        });
                    });
                    window.refresh();
                })
                .unwrap();
            cx.activate(true);
        });
    let result = outcome
        .borrow_mut()
        .take()
        .expect("native adapter test quit before its final assertions");
    if let Err(error) = result {
        resume_unwind(error);
    }
    assert!(completed.get(), "native adapter test did not complete");
    eprintln!("IME adapter regression passed; OS candidate input remains unverified");
}
