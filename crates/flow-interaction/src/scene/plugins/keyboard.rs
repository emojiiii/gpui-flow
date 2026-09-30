use crate::scene::SceneCommand;
use crate::*;
use gpui_flow_editor::SceneEditor;
pub struct SceneKeyboardPlugin;
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneKeyboardPlugin {
    fn id(&self) -> &str {
        "keyboard"
    }
    fn priority(&self) -> i32 {
        100
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn on_event(
        &mut self,
        event: &CanvasEvent,
        cx: &PluginContext<'_, SceneEditor>,
        out: &mut PluginOutput<SceneCommand>,
    ) {
        let CanvasEvent::KeyDown { key, modifiers } = event else {
            return;
        };
        match key {
            KeyCode::Delete | KeyCode::Backspace if cx.allows(Capability::Edit) => {
                out.command(SceneCommand::DeleteSelection)
            }
            KeyCode::Character(c) => {
                let c = c.to_ascii_lowercase();
                if modifiers.command() {
                    match c {
                        'a' if cx.allows(Capability::Selection) => {
                            out.command(SceneCommand::SelectAll)
                        }
                        'z' if cx.allows(Capability::Edit) => out.command(if modifiers.shift {
                            SceneCommand::Redo
                        } else {
                            SceneCommand::Undo
                        }),
                        'y' if cx.allows(Capability::Edit) => out.command(SceneCommand::Redo),
                        'v' if cx.allows(Capability::Edit) => {
                            out.effect(CanvasEffect::RequestPaste)
                        }
                        's' if cx.allows(Capability::ClipboardRead) => {
                            out.effect(if modifiers.shift {
                                CanvasEffect::RequestCopyDocument
                            } else {
                                CanvasEffect::RequestSave
                            })
                        }
                        _ => {}
                    }
                } else {
                    match c {
                        'r' if cx.allows(Capability::Edit) => {
                            out.command(SceneCommand::RotateSelection {
                                degrees: if modifiers.shift { -15.0 } else { 15.0 },
                            })
                        }
                        '+' | '=' if cx.allows(Capability::Edit) => {
                            out.command(SceneCommand::ScaleSelection { factor: 1.1 })
                        }
                        '-' if cx.allows(Capability::Edit) => {
                            out.command(SceneCommand::ScaleSelection { factor: 1.0 / 1.1 })
                        }
                        'f' if cx.allows(Capability::Navigation) => {
                            out.effect(CanvasEffect::RequestFit)
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}
