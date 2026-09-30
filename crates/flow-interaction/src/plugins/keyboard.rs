use crate::*;
use gpui_flow_core::Point;
pub struct KeyboardPlugin;
impl CanvasPlugin for KeyboardPlugin {
    fn id(&self) -> &str {
        "keyboard"
    }
    fn priority(&self) -> i32 {
        100
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn on_event(&mut self, event: &CanvasEvent, cx: &PluginContext<'_>, out: &mut PluginOutput) {
        let CanvasEvent::KeyDown { key, modifiers } = event else {
            return;
        };
        match key {
            KeyCode::Delete | KeyCode::Backspace if cx.allows(Capability::Edit) => {
                out.command(CanvasCommand::DeleteSelection)
            }
            KeyCode::Character(character) => {
                let character = character.to_ascii_lowercase();
                if modifiers.command() {
                    match character {
                        'a' if cx.allows(Capability::Selection) => {
                            out.command(CanvasCommand::SelectAll)
                        }
                        'c' if cx.allows(Capability::ClipboardRead) => {
                            out.command(CanvasCommand::CopySelection)
                        }
                        'v' if cx.allows(Capability::Edit) => {
                            out.effect(CanvasEffect::RequestPaste)
                        }
                        'd' if cx.allows(Capability::Edit) => {
                            out.command(CanvasCommand::Duplicate {
                                offset: Point::new(24.0, 24.0),
                            })
                        }
                        'z' if cx.allows(Capability::Edit) => out.command(if modifiers.shift {
                            CanvasCommand::Redo
                        } else {
                            CanvasCommand::Undo
                        }),
                        'y' if cx.allows(Capability::Edit) => out.command(CanvasCommand::Redo),
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
                    match character {
                        'f' if cx.allows(Capability::Navigation) => {
                            out.effect(CanvasEffect::RequestFit)
                        }
                        'g' if cx.allows(Capability::Navigation) => {
                            out.effect(CanvasEffect::ToggleGrid)
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }
}
