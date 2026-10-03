use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, PropertyKey};
use gpui_flow_interaction::scene::*;
use gpui_flow_scene::{DisplayObject, Scene, display};
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
        for id in ["a", "b"] {
            scene
                .insert(
                    DisplayObject::new(id, display::Rect::new(20., 15.)),
                    None,
                    None,
                )
                .unwrap();
        }
        scene
            .insert(
                DisplayObject::new("ellipse", display::Ellipse::new(20., 15.)),
                None,
                None,
            )
            .unwrap();
        let editor = SceneEditor::new(scene).unwrap();
        let a = editor.engine.handle(&"a".into()).unwrap();
        let b = editor.engine.handle(&"b".into()).unwrap();
        let mut tools = SceneToolController::with_text();
        tools.register_inner("style", "rect").unwrap();
        tools.register_outer("inspect").unwrap();
        Self {
            editor,
            host: ScenePluginHost::new(),
            tools,
            a,
            b,
        }
    }
    fn open(&mut self, h: ElementHandle) -> ToolSessionToken {
        self.tools
            .open_inner(&mut self.editor, &mut self.host, "style", h)
            .unwrap()
    }
    fn set(&mut self, t: ToolSessionToken, value: f32) {
        self.tools
            .set_inner_property(
                &mut self.editor,
                &mut self.host,
                t,
                PropertyKey::Opacity,
                json!(value),
            )
            .unwrap();
    }
    fn opacity(&self, h: ElementHandle) -> f32 {
        self.editor.engine.object(h).unwrap().style.opacity
    }
}
#[test]
fn custom_registry_dispatch_preserves_current_session_on_denied_switch() {
    let mut l = Lab::new();
    let t = l.open(l.a);
    assert_eq!(l.open(l.a), t);
    assert_eq!(l.tools.active_kind(), Some(SceneToolKind::CustomInner));
    assert_eq!(l.tools.active_text(), None);
    assert!(l.tools.register_inner("other", "rect").is_err());
    assert!(l.tools.register_inner("other", "unknown").is_err());
    assert!(l.tools.register_inner("text-custom", "text").is_err());
    let ellipse = l.editor.engine.handle(&"ellipse".into()).unwrap();
    assert!(
        l.tools
            .open_inner(&mut l.editor, &mut l.host, "style", ellipse)
            .is_err()
    );
    assert_eq!(l.tools.active_token(), Some(t));
    assert!(
        l.tools
            .set_text(&mut l.editor, &mut l.host, t, "not text")
            .is_err()
    );
    assert_eq!(l.tools.active_token(), Some(t));
}
#[test]
fn custom_updates_are_one_undo_and_cancel_keeps_redo() {
    let mut l = Lab::new();
    l.tools.drain_events();
    let t = l.open(l.a);
    l.set(t, 0.5);
    l.set(t, 0.25);
    assert!(l.tools.finish(&mut l.editor, &mut l.host, t).unwrap());
    let events = l.tools.drain_events();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind == ToolLifecycleKind::Updated)
            .count(),
        2
    );
    assert!(l.editor.undo());
    assert_eq!(l.opacity(l.a), 1.);
    assert!(!l.editor.undo());
    assert!(l.editor.redo());
    assert_eq!(l.opacity(l.a), 0.25);
    assert!(l.editor.undo());
    let t = l.open(l.a);
    l.set(t, 0.75);
    assert!(l.tools.cancel(&mut l.editor, &mut l.host, t).unwrap());
    assert_eq!(l.opacity(l.a), 1.);
    assert!(l.editor.redo());
    assert_eq!(l.opacity(l.a), 0.25);
}
#[test]
fn stale_callbacks_external_edits_and_replacements_cannot_restore_old_baselines() {
    let mut l = Lab::new();
    let old = l.open(l.a);
    l.set(old, 0.5);
    let new = l.open(l.b);
    assert!(
        l.tools
            .set_inner_property(
                &mut l.editor,
                &mut l.host,
                old,
                PropertyKey::Opacity,
                json!(0.1)
            )
            .is_err()
    );
    assert_eq!(l.tools.active_token(), Some(new));
    assert_eq!(l.opacity(l.b), 1.);
    l.set(new, 0.75);
    l.editor
        .set_property(l.a, PropertyKey::Opacity, json!(0.2))
        .unwrap();
    assert!(l.tools.reconcile(&mut l.editor, &mut l.host));
    assert!(!l.tools.cancel(&mut l.editor, &mut l.host, new).unwrap());
    assert_eq!(l.opacity(l.a), 0.2);
    assert_eq!(l.opacity(l.b), 0.75);
    let t = l.open(l.b);
    let same = l.editor.scene().clone();
    l.host
        .execute(&mut l.editor, SceneCommand::LoadScene(same))
        .unwrap();
    assert!(l.tools.reconcile(&mut l.editor, &mut l.host));
    assert!(!l.tools.cancel(&mut l.editor, &mut l.host, t).unwrap());
    assert!(l.editor.engine.object(l.b).is_err());
    assert!(!l.editor.undo());
}
#[test]
fn registry_unload_destroy_and_outer_selection_are_exactly_once() {
    let mut l = Lab::new();
    let t = l.open(l.a);
    l.set(t, 0.4);
    l.tools.drain_events();
    assert!(
        l.tools
            .select_outer(&mut l.editor, &mut l.host, "inspect")
            .unwrap()
    );
    assert!(l.tools.outer_tool_active("inspect"));
    assert_eq!(l.tools.active_token(), None);
    assert!(
        l.tools
            .unregister(&mut l.editor, &mut l.host, "style")
            .unwrap()
    );
    assert!(
        !l.tools
            .unregister(&mut l.editor, &mut l.host, "style")
            .unwrap()
    );
    assert!(l.tools.dispose(&mut l.editor, &mut l.host).unwrap());
    assert!(!l.tools.dispose(&mut l.editor, &mut l.host).unwrap());
    let events = l.tools.drain_events();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.tool_id == "style" && e.kind == ToolLifecycleKind::Destroyed)
            .count(),
        1
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.tool_id == "style" && matches!(e.kind, ToolLifecycleKind::Unloaded(_)))
            .count(),
        1
    );
    assert!(l.editor.undo());
    assert_eq!(l.opacity(l.a), 1.);
}
#[test]
fn invalid_property_is_atomic_and_policy_revocation_seals_only_accepted_edits() {
    let mut l = Lab::new();
    let t = l.open(l.a);
    l.set(t, 0.6);
    let before = l.editor.scene().clone();
    let revision = l.editor.engine.revision();
    assert!(
        l.tools
            .set_inner_property(
                &mut l.editor,
                &mut l.host,
                t,
                PropertyKey::Opacity,
                json!(2)
            )
            .is_err()
    );
    assert_eq!(l.editor.scene(), &before);
    assert_eq!(l.editor.engine.revision(), revision);
    l.host.set_editable(false, &mut l.editor);
    assert!(
        l.tools
            .set_inner_property(
                &mut l.editor,
                &mut l.host,
                t,
                PropertyKey::Opacity,
                json!(0.1)
            )
            .is_err()
    );
    assert_eq!(l.opacity(l.a), 0.6);
    assert_eq!(l.tools.active_token(), None);
    assert!(l.editor.undo());
    assert_eq!(l.opacity(l.a), 1.);
}
#[test]
fn accepted_self_lock_closes_tool_without_rolling_back_the_command() {
    let mut l = Lab::new();
    let t = l.open(l.a);
    assert!(
        l.tools
            .set_inner_property(
                &mut l.editor,
                &mut l.host,
                t,
                PropertyKey::Locked,
                json!(true)
            )
            .is_ok()
    );
    assert!(l.editor.engine.object(l.a).unwrap().locked);
    assert_eq!(l.tools.active_token(), None);
    assert!(!l.tools.cancel(&mut l.editor, &mut l.host, t).unwrap());
    assert!(l.editor.undo());
    assert!(!l.editor.engine.object(l.a).unwrap().locked);
}
