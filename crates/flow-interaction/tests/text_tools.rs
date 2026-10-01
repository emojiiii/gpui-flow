use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, PropertyKey};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{DisplayObject, Scene, Shape, Size, display};
use serde_json::json;
struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    tools: SceneToolController,
    a: ElementHandle,
    b: ElementHandle,
}
impl Lab {
    fn new() -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("group", display::Group::new()),
                None,
                None,
            )
            .unwrap();
        for id in ["a", "b"] {
            scene
                .insert(
                    DisplayObject::new(id, display::Text::new(id, Size::new(120., 40.))),
                    Some("group".into()),
                    None,
                )
                .unwrap();
        }
        scene
            .insert(
                DisplayObject::new("rect", display::Rect::new(10., 10.)),
                None,
                None,
            )
            .unwrap();
        let editor = SceneEditor::new(scene).unwrap();
        let a = editor.engine.handle(&"a".into()).unwrap();
        let b = editor.engine.handle(&"b".into()).unwrap();
        Self {
            editor,
            host: ScenePluginHost::new(),
            tools: SceneToolController::with_text(),
            a,
            b,
        }
    }
    fn open(&mut self, handle: ElementHandle) -> TextSessionToken {
        self.tools
            .open_text(&mut self.editor, &mut self.host, handle)
            .unwrap()
    }
    fn set(&mut self, token: TextSessionToken, value: &str) {
        self.tools
            .set_text(&mut self.editor, &mut self.host, token, value)
            .unwrap();
    }
    fn text(&self, handle: ElementHandle) -> &str {
        let Shape::Text(text) = &self.editor.engine.object(handle).unwrap().shape else {
            panic!()
        };
        &text.content
    }
}
#[test]
fn text_tool_is_optional_and_plain_text_only_with_registry_isolation() {
    let mut lab = Lab::new();
    let mut without_text = SceneToolController::new();
    assert!(
        without_text
            .open_text(&mut lab.editor, &mut lab.host, lab.a)
            .is_err()
    );
    let rect = lab.editor.engine.handle(&"rect".into()).unwrap();
    assert!(
        lab.tools
            .open_text(&mut lab.editor, &mut lab.host, rect)
            .is_err()
    );
    assert!(!lab.editor.editing());
    let token = lab.open(lab.a);
    assert_eq!(lab.open(lab.a), token);
    assert_eq!(lab.tools.active_text(), Some("a"));
    assert_eq!(lab.tools.active_target(), Some(lab.a));
    let mut registry = SceneToolRegistry::new();
    registry.register_text("writer").unwrap();
    assert!(registry.register_text("other").is_err());
    assert!(registry.register_outer("writer").is_err());
    assert!(registry.register_outer("").is_err());
    assert!(registry.set_default_outer("writer").is_err());
    assert!(registry.inner_for_shape("custom").is_none());
    assert_eq!(registry.inner_for_shape("text").unwrap().id, "writer");
    assert_eq!(
        lab.tools.registry().inner_for_shape("text").unwrap().id,
        "text"
    );
}
#[test]
fn live_text_updates_commit_as_one_undo_and_explicit_lifecycle_is_ordered() {
    let mut lab = Lab::new();
    assert_eq!(
        lab.tools
            .drain_events()
            .iter()
            .map(|event| (&*event.tool_id, event.kind))
            .collect::<Vec<_>>(),
        [
            ("selection", ToolLifecycleKind::Created),
            ("text", ToolLifecycleKind::Created),
            ("selection", ToolLifecycleKind::Loaded),
        ]
    );
    let token = lab.open(lab.a);
    lab.set(token, "a\n🙂");
    lab.set(token, "new\nvalue\n");
    lab.set(token, "new\nvalue\n");
    assert_eq!(lab.text(lab.a), "new\nvalue\n");
    assert!(
        lab.tools
            .finish_with_reason(&mut lab.editor, &mut lab.host, token, ToolCloseReason::Blur)
            .unwrap()
    );
    assert!(
        !lab.tools
            .finish(&mut lab.editor, &mut lab.host, token)
            .unwrap()
    );
    assert!(
        !lab.tools
            .cancel(&mut lab.editor, &mut lab.host, token)
            .unwrap()
    );
    let events = lab.tools.drain_events();
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            ToolLifecycleKind::Unloaded(ToolCloseReason::InnerOpened),
            ToolLifecycleKind::Loaded,
            ToolLifecycleKind::Updated,
            ToolLifecycleKind::Updated,
            ToolLifecycleKind::Unloaded(ToolCloseReason::Blur),
            ToolLifecycleKind::Loaded,
        ]
    );
    assert_eq!(events[1].target, Some(lab.a));
    assert_eq!(events[1].token, Some(token));
    assert!(lab.editor.undo());
    assert_eq!(lab.text(lab.a), "a");
    assert!(!lab.editor.undo());
    assert!(lab.editor.redo());
    assert_eq!(lab.text(lab.a), "new\nvalue\n");
}
#[test]
fn escape_cancel_and_unchanged_done_leave_redo_available() {
    let mut lab = Lab::new();
    lab.editor
        .set_property(lab.a, PropertyKey::TextContent, json!("redo"))
        .unwrap();
    assert!(lab.editor.undo());
    let token = lab.open(lab.a);
    lab.set(token, "cancel me");
    assert!(
        lab.tools
            .cancel(&mut lab.editor, &mut lab.host, token)
            .unwrap()
    );
    assert_eq!(lab.text(lab.a), "a");
    let token = lab.open(lab.a);
    lab.set(token, "a");
    assert!(
        lab.tools
            .finish(&mut lab.editor, &mut lab.host, token)
            .unwrap()
    );
    assert!(lab.editor.redo());
    assert_eq!(lab.text(lab.a), "redo");
}
#[test]
fn target_switch_commits_old_and_stale_callbacks_cannot_close_newer() {
    let mut lab = Lab::new();
    let old = lab.open(lab.a);
    lab.set(old, "old committed");
    let rect = lab.editor.engine.handle(&"rect".into()).unwrap();
    assert!(
        lab.tools
            .validate_text_target(&lab.editor, &lab.host, rect)
            .is_err()
    );
    assert!(
        lab.tools
            .open_text(&mut lab.editor, &mut lab.host, rect)
            .is_err()
    );
    assert_eq!(lab.tools.active_token(), Some(old));
    let new = lab.open(lab.b);
    assert_ne!(old, new);
    assert!(
        !lab.tools
            .finish(&mut lab.editor, &mut lab.host, old)
            .unwrap()
    );
    assert!(
        !lab.tools
            .cancel(&mut lab.editor, &mut lab.host, old)
            .unwrap()
    );
    assert!(
        lab.tools
            .set_text(&mut lab.editor, &mut lab.host, old, "stale")
            .is_err()
    );
    assert_eq!(lab.tools.active_token(), Some(new));
    lab.set(new, "new cancelled");
    lab.tools
        .cancel(&mut lab.editor, &mut lab.host, new)
        .unwrap();
    assert_eq!(lab.text(lab.a), "old committed");
    assert_eq!(lab.text(lab.b), "b");
    assert!(lab.editor.undo());
    assert_eq!(lab.text(lab.a), "a");
}
#[test]
fn readonly_and_inherited_lock_revoke_without_baseline_restore() {
    let mut lab = Lab::new();
    let token = lab.open(lab.a);
    lab.set(token, "kept");
    lab.host.set_editable(false, &mut lab.editor);
    assert!(lab.tools.reconcile(&mut lab.editor, &mut lab.host));
    assert_eq!(lab.text(lab.a), "kept");
    assert!(
        lab.tools
            .set_text(&mut lab.editor, &mut lab.host, token, "blocked")
            .is_err()
    );
    assert!(
        lab.tools
            .open_text(&mut lab.editor, &mut lab.host, lab.a)
            .is_err()
    );
    lab.host.set_editable(true, &mut lab.editor);
    assert!(lab.editor.undo());
    assert_eq!(lab.text(lab.a), "a");
    let token = lab.open(lab.a);
    lab.set(token, "typed");
    let group = lab.editor.engine.handle(&"group".into()).unwrap();
    lab.editor
        .engine
        .set_property(group, PropertyKey::Locked, json!(true))
        .unwrap();
    assert!(
        !lab.tools
            .cancel(&mut lab.editor, &mut lab.host, token)
            .unwrap()
    );
    assert_eq!(lab.text(lab.a), "typed");
    assert!(lab.editor.undo());
    assert!(
        !lab.editor
            .scene()
            .get(&"group".into())
            .unwrap()
            .object
            .locked
    );
    assert_eq!(lab.text(lab.a), "typed");
    assert!(lab.editor.undo());
    assert_eq!(lab.text(lab.a), "a");
}
#[test]
fn unrelated_engine_revision_seals_text_and_records_external_change_separately() {
    let mut lab = Lab::new();
    let token = lab.open(lab.a);
    lab.set(token, "typed");
    lab.editor
        .engine
        .set_property(lab.b, PropertyKey::Name, json!("external"))
        .unwrap();
    assert!(lab.tools.reconcile(&mut lab.editor, &mut lab.host));
    assert!(
        !lab.tools
            .cancel(&mut lab.editor, &mut lab.host, token)
            .unwrap()
    );
    assert!(lab.editor.undo());
    assert_eq!(lab.text(lab.a), "typed");
    assert_eq!(lab.editor.engine.object(lab.b).unwrap().name, "");
    assert!(lab.editor.undo());
    assert_eq!(lab.text(lab.a), "a");
}
#[test]
fn delete_recreate_same_id_and_detach_reattach_never_resume_old_session() {
    let mut lab = Lab::new();
    let old = lab.open(lab.a);
    lab.set(old, "typed");
    lab.editor.engine.destroy(lab.a).unwrap();
    let (fresh, _) = lab
        .editor
        .engine
        .create("text", "a", json!({"content":"replacement"}), None, None)
        .unwrap();
    assert_ne!(fresh, lab.a);
    assert!(lab.tools.reconcile(&mut lab.editor, &mut lab.host));
    assert!(
        !lab.tools
            .cancel(&mut lab.editor, &mut lab.host, old)
            .unwrap()
    );
    assert_eq!(lab.text(fresh), "replacement");
    let token = lab.open(fresh);
    let (detached, _) = lab.editor.engine.detach(fresh).unwrap();
    lab.editor.engine.attach(&detached, None, None).unwrap();
    assert!(lab.tools.reconcile(&mut lab.editor, &mut lab.host));
    assert!(
        !lab.tools
            .cancel(&mut lab.editor, &mut lab.host, token)
            .unwrap()
    );
    assert_eq!(lab.text(fresh), "replacement");
}
#[test]
fn loading_new_document_revokes_session_without_old_history_or_callbacks() {
    let mut lab = Lab::new();
    let old = lab.open(lab.a);
    lab.set(old, "typed");
    let new_scene = lab.editor.scene().clone();
    lab.host
        .execute(&mut lab.editor, SceneCommand::LoadScene(new_scene))
        .unwrap();
    assert!(lab.tools.reconcile(&mut lab.editor, &mut lab.host));
    assert!(
        !lab.tools
            .cancel(&mut lab.editor, &mut lab.host, old)
            .unwrap()
    );
    assert!(lab.editor.engine.object(lab.a).is_err());
    assert!(!lab.editor.undo());
    let fresh = lab.editor.engine.handle(&"a".into()).unwrap();
    let token = lab.open(fresh);
    assert!(
        lab.tools
            .set_text(&mut lab.editor, &mut lab.host, old, "stale")
            .is_err()
    );
    assert_eq!(lab.tools.active_token(), Some(token));
}
#[test]
fn gesture_and_playback_cannot_steal_text_ownership() {
    let mut lab = Lab::new();
    assert!(lab.editor.begin_edit());
    assert!(
        lab.tools
            .open_text(&mut lab.editor, &mut lab.host, lab.a)
            .is_err()
    );
    lab.editor.cancel_edit();
    let playback = lab.editor.begin_playback().unwrap();
    assert!(
        lab.tools
            .open_text(&mut lab.editor, &mut lab.host, lab.a)
            .is_err()
    );
    lab.editor.cancel_playback(playback);
    let token = lab.open(lab.a);
    lab.set(token, "typed");
    assert!(lab.editor.begin_edit());
    assert!(lab.tools.reconcile(&mut lab.editor, &mut lab.host));
    assert!(
        !lab.tools
            .cancel(&mut lab.editor, &mut lab.host, token)
            .unwrap()
    );
    assert!(lab.editor.gesture_editing());
    lab.editor.cancel_edit();
    assert_eq!(lab.text(lab.a), "typed");
}
struct Normalize;
impl CanvasPlugin<SceneEditor, SceneCommand> for Normalize {
    fn id(&self) -> &str {
        "normalize"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetSessionProperty { value, .. } = command {
            *value = json!(value.as_str().unwrap().to_uppercase());
        }
        Ok(())
    }
}
struct Retarget {
    target: ElementHandle,
}
impl CanvasPlugin<SceneEditor, SceneCommand> for Retarget {
    fn id(&self) -> &str {
        "retarget"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetSessionProperty { handle, .. } = command {
            *handle = self.target;
        }
        Ok(())
    }
}
#[test]
fn middleware_can_normalize_but_cannot_retarget_owned_text() {
    let mut lab = Lab::new();
    lab.host.install(Normalize).unwrap();
    let token = lab.open(lab.a);
    lab.set(token, "lower");
    assert_eq!(lab.tools.active_text(), Some("LOWER"));
    assert_eq!(lab.text(lab.a), "LOWER");
    lab.host.install(Retarget { target: lab.b }).unwrap();
    assert!(
        lab.tools
            .set_text(&mut lab.editor, &mut lab.host, token, "blocked")
            .is_err()
    );
    assert_eq!(lab.text(lab.a), "LOWER");
    assert_eq!(lab.text(lab.b), "b");
    assert_eq!(lab.tools.active_token(), Some(token));
}
#[test]
fn dispose_is_idempotent_seals_history_and_never_reloads_outer() {
    let mut lab = Lab::new();
    let token = lab.open(lab.a);
    lab.set(token, "typed");
    lab.tools.drain_events();
    assert!(lab.tools.dispose(&mut lab.editor, &mut lab.host).unwrap());
    assert!(!lab.tools.dispose(&mut lab.editor, &mut lab.host).unwrap());
    let events = lab.tools.drain_events();
    assert_eq!(
        events.iter().map(|event| event.kind).collect::<Vec<_>>(),
        [
            ToolLifecycleKind::Unloaded(ToolCloseReason::Disposed),
            ToolLifecycleKind::Destroyed,
            ToolLifecycleKind::Destroyed,
        ]
    );
    assert!(lab.tools.active_token().is_none());
    assert!(!lab.editor.editing());
    assert!(
        lab.tools
            .open_text(&mut lab.editor, &mut lab.host, lab.a)
            .is_err()
    );
    assert!(lab.editor.undo());
    assert_eq!(lab.text(lab.a), "a");
}
#[test]
fn outer_switch_disable_and_active_unregister_have_deterministic_cleanup() {
    let mut lab = Lab::new();
    lab.tools.register_outer("pan").unwrap();
    assert!(
        lab.tools
            .select_outer(&mut lab.editor, &mut lab.host, "pan")
            .unwrap()
    );
    let token = lab.open(lab.a);
    lab.set(token, "kept");
    lab.tools.drain_events();
    assert!(
        lab.tools
            .set_enabled(&mut lab.editor, &mut lab.host, false)
            .unwrap()
    );
    assert_eq!(
        lab.tools
            .drain_events()
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        [ToolLifecycleKind::Unloaded(ToolCloseReason::Disabled)]
    );
    assert!(
        lab.tools
            .set_enabled(&mut lab.editor, &mut lab.host, true)
            .unwrap()
    );
    let token = lab.open(lab.a);
    lab.set(token, "last");
    assert!(
        lab.tools
            .unregister(&mut lab.editor, &mut lab.host, "text")
            .unwrap()
    );
    assert_eq!(lab.text(lab.a), "last");
    assert!(!lab.editor.editing());
    assert!(
        lab.tools
            .open_text(&mut lab.editor, &mut lab.host, lab.a)
            .is_err()
    );
    lab.tools.register_text("replacement").unwrap();
    assert!(
        lab.tools
            .open_text(&mut lab.editor, &mut lab.host, lab.a)
            .is_ok()
    );
    lab.tools
        .finish(
            &mut lab.editor,
            &mut lab.host,
            lab.tools.active_token().unwrap(),
        )
        .unwrap();
    assert!(
        lab.tools
            .unregister(&mut lab.editor, &mut lab.host, "pan")
            .unwrap()
    );
    assert_eq!(lab.tools.outer_tool(), DEFAULT_OUTER_TOOL);
    assert!(
        lab.tools
            .unregister(&mut lab.editor, &mut lab.host, DEFAULT_OUTER_TOOL)
            .is_err()
    );
}
struct Append;
impl CanvasPlugin<SceneEditor, SceneCommand> for Append {
    fn id(&self) -> &str {
        "append"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetSessionProperty { value, .. } = command {
            *value = json!(format!("{}!", value.as_str().unwrap()));
        }
        Ok(())
    }
}
#[test]
fn final_flush_does_not_repeat_non_idempotent_normalization() {
    let mut lab = Lab::new();
    lab.host.install(Append).unwrap();
    let token = lab.open(lab.a);
    lab.set(token, "typed");
    assert_eq!(lab.tools.active_text(), Some("typed!"));
    let revision = lab.editor.engine.revision();
    lab.set(token, "typed!");
    assert_eq!(lab.editor.engine.revision(), revision);
    assert_eq!(lab.text(lab.a), "typed!");
    lab.tools
        .finish(&mut lab.editor, &mut lab.host, token)
        .unwrap();
    assert!(lab.editor.undo());
    assert_eq!(lab.text(lab.a), "a");
    assert!(!lab.editor.undo());
}
#[test]
fn invalid_document_load_and_generic_commit_cancel_preserve_owned_session() {
    let mut lab = Lab::new();
    let token = lab.open(lab.a);
    lab.set(token, "typed");
    let mut invalid = lab.editor.scene().clone();
    invalid.version = u32::MAX;
    assert!(
        lab.host
            .execute(&mut lab.editor, SceneCommand::LoadScene(invalid))
            .is_err()
    );
    lab.editor.commit_edit();
    assert!(!lab.editor.cancel_edit());
    assert!(!lab.tools.reconcile(&mut lab.editor, &mut lab.host));
    assert_eq!(lab.tools.active_token(), Some(token));
    lab.tools
        .cancel(&mut lab.editor, &mut lab.host, token)
        .unwrap();
    assert_eq!(lab.text(lab.a), "a");
}
#[test]
fn editor_and_controller_instances_have_isolated_runtime_tokens() {
    let mut a = Lab::new();
    let mut b = Lab::new();
    let ta = a.open(a.a);
    let tb = b.open(b.a);
    assert_ne!(ta, tb);
    assert!(
        a.tools
            .set_text(&mut b.editor, &mut b.host, ta, "wrong editor")
            .is_err()
    );
    assert!(
        b.tools
            .set_text(&mut b.editor, &mut b.host, ta, "wrong token")
            .is_err()
    );
    assert_eq!(a.tools.active_token(), Some(ta));
    assert_eq!(b.tools.active_token(), Some(tb));
    assert_eq!(a.text(a.a), "a");
    assert_eq!(b.text(b.a), "a");
}
#[test]
fn public_close_reason_always_restores_outer_without_a_following_open() {
    for reason in [
        ToolCloseReason::TargetSwitch,
        ToolCloseReason::Disabled,
        ToolCloseReason::Disposed,
        ToolCloseReason::OuterToolSwitch,
    ] {
        let mut lab = Lab::new();
        let token = lab.open(lab.a);
        lab.tools.drain_events();
        assert!(
            lab.tools
                .finish_with_reason(&mut lab.editor, &mut lab.host, token, reason)
                .unwrap()
        );
        let events = lab.tools.drain_events();
        assert_eq!(
            events.iter().map(|event| event.kind).collect::<Vec<_>>(),
            [
                ToolLifecycleKind::Unloaded(reason),
                ToolLifecycleKind::Loaded
            ]
        );
        assert_eq!(events[1].tool_id, DEFAULT_OUTER_TOOL);
        assert!(lab.tools.is_enabled());
    }
}
