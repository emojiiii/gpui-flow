use super::*;

#[derive(Clone, Copy)]
pub(super) enum SuspendedTool {
    Text,
    Frame,
    Shape(crate::shapes::ShapeKind),
    Pencil,
    Model,
}

impl CreativeCanvas {
    fn focused_drawing_tool(&self, window: &Window, cx: &App) -> Option<SuspendedTool> {
        if self.text_placing && self.text_placement_focus.is_focused(window) {
            Some(SuspendedTool::Text)
        } else if self.frame_drawing && self.frame_focus.is_focused(window) {
            Some(SuspendedTool::Frame)
        } else if let Some(kind) = self.shape_tool
            && self.shape_focus.is_focused(window)
        {
            Some(SuspendedTool::Shape(kind))
        } else if self.pencil_active && self.pencil_focus.is_focused(window) {
            Some(SuspendedTool::Pencil)
        } else if self.model_interaction.is_some()
            && self.canvas.read(cx).focus_handle(cx).is_focused(window)
        {
            Some(SuspendedTool::Model)
        } else {
            None
        }
    }

    fn drawing_in_progress(&self) -> bool {
        self.text_placement_drag.is_some()
            || self.frame_drag.is_some()
            || self.shape_drag.is_some()
            || self.pencil_draft.is_some()
            || self.model_drag.is_some()
    }

    pub(super) fn finish_temporary_hand(
        &mut self,
        restore: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(tool) = self.temporary_hand.take() else {
            return;
        };
        // Release Space through the navigation plugin before changing focus.
        // set_pan_mode(false) cancels capture and would roll back the viewport.
        self.canvas
            .update(cx, |canvas, cx| canvas.set_temporary_pan(false, cx));
        if restore && window.is_window_active() {
            let focus = match tool {
                SuspendedTool::Text => {
                    self.text_placing = true;
                    self.text_placement_focus.clone()
                }
                SuspendedTool::Frame => {
                    self.frame_drawing = true;
                    self.frame_focus.clone()
                }
                SuspendedTool::Shape(kind) => {
                    self.shape_tool = Some(kind);
                    self.shape_focus.clone()
                }
                SuspendedTool::Pencil => {
                    self.pencil_active = true;
                    self.pencil_focus.clone()
                }
                SuspendedTool::Model => self.canvas.read(cx).focus_handle(cx),
            };
            focus.focus(window, cx);
        } else if matches!(tool, SuspendedTool::Model) {
            self.clear_model_interaction(cx);
        }
        cx.notify();
    }

    pub(super) fn tool_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let key = event.keystroke.key.as_str();
        let modifiers = event.keystroke.modifiers;
        if matches!(key, "space" | " ")
            && !modifiers.control
            && !modifiers.platform
            && !modifiers.alt
        {
            if self.temporary_hand.is_some() {
                cx.stop_propagation();
                window.prevent_default();
                return;
            }
            let Some(tool) = self.focused_drawing_tool(window, cx) else {
                return;
            };
            // An in-progress stroke/rectangle belongs to its current pointer.
            // Leave the draft intact; release it before starting navigation.
            if self.drawing_in_progress() || self.canvas.read(cx).interactions.has_capture() {
                self.notice = Some("先松开鼠标完成当前操作，再按住 Space 临时平移".into());
                cx.stop_propagation();
                window.prevent_default();
                cx.notify();
                return;
            }
            self.temporary_hand = Some(tool);
            match tool {
                SuspendedTool::Text => self.text_placing = false,
                SuspendedTool::Frame => self.frame_drawing = false,
                SuspendedTool::Shape(_) => self.shape_tool = None,
                SuspendedTool::Pencil => self.pencil_active = false,
                SuspendedTool::Model => {}
            }
            self.canvas
                .update(cx, |canvas, cx| canvas.set_temporary_pan(true, cx));
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.stop_propagation();
            window.prevent_default();
            cx.notify();
            return;
        }
        let focused = self.canvas.read(cx).focus_handle(cx).is_focused(window)
            || self.focused_drawing_tool(window, cx).is_some();
        if !focused || self.drawing_in_progress() {
            return;
        }
        if key == "escape" && self.temporary_hand.is_some() {
            // Let the focused core canvas process Escape first. The bubbling
            // handler then ends temporary mode without committing a cancelled pan.
            return;
        }
        let Some(shortcut) = tool_shortcut(key, modifiers) else {
            return;
        };
        let action = match shortcut {
            ToolShortcut::Select => Action::PanTool(false),
            ToolShortcut::Hand => Action::PanTool(true),
            ToolShortcut::Frame => Action::FrameEdit(frame_tools::FrameAction::Draw),
            ToolShortcut::Text => {
                Action::TextPlacement(text_placement_tools::TextPlacementAction::Toggle)
            }
            ToolShortcut::Pencil => Action::Pencil(pencil_tools::PencilAction::Toggle),
            ToolShortcut::Image => Action::Add("generate_image".into()),
            ToolShortcut::Video => Action::Add("generate_video".into()),
        };
        cx.stop_propagation();
        window.prevent_default();
        self.action(action, window, cx);
    }
}

#[derive(Debug, PartialEq)]
enum ToolShortcut {
    Select,
    Hand,
    Frame,
    Text,
    Pencil,
    Image,
    Video,
}

fn tool_shortcut(key: &str, modifiers: Modifiers) -> Option<ToolShortcut> {
    if modifiers.control || modifiers.platform || modifiers.alt || modifiers.shift {
        return None;
    }
    match key {
        "v" => Some(ToolShortcut::Select),
        "h" => Some(ToolShortcut::Hand),
        "f" => Some(ToolShortcut::Frame),
        "t" => Some(ToolShortcut::Text),
        "p" => Some(ToolShortcut::Pencil),
        "a" => Some(ToolShortcut::Image),
        "s" => Some(ToolShortcut::Video),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[test]
    fn tool_keys_preserve_modified_core_shortcuts_and_do_not_claim_quick_edit() {
        assert_eq!(
            tool_shortcut("f", Modifiers::default()),
            Some(ToolShortcut::Frame)
        );
        assert_eq!(
            tool_shortcut("a", Modifiers::default()),
            Some(ToolShortcut::Image)
        );
        assert_eq!(
            tool_shortcut("s", Modifiers::default()),
            Some(ToolShortcut::Video)
        );
        for modifiers in [
            Modifiers {
                control: true,
                ..Default::default()
            },
            Modifiers {
                platform: true,
                ..Default::default()
            },
            Modifiers {
                alt: true,
                ..Default::default()
            },
            Modifiers {
                shift: true,
                ..Default::default()
            },
        ] {
            for key in ["f", "a", "s", "t", "p", "v", "h"] {
                assert_eq!(tool_shortcut(key, modifiers), None);
            }
        }
        assert_eq!(tool_shortcut("tab", Modifiers::default()), None);
    }
}
