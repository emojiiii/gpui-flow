//! Native pointer adapter. Scene behavior lives in the document-specific plugin host.
use super::*;
use gpui_flow_interaction as input;
fn modifiers(value: gpui::Modifiers, space: bool) -> input::Modifiers {
    input::Modifiers {
        shift: value.shift,
        control: value.control,
        alt: value.alt,
        meta: value.platform,
        space,
    }
}
fn button(value: MouseButton) -> Option<input::PointerButton> {
    match value {
        MouseButton::Left => Some(input::PointerButton::Left),
        MouseButton::Middle => Some(input::PointerButton::Middle),
        MouseButton::Right => Some(input::PointerButton::Right),
        _ => None,
    }
}
impl SceneView {
    fn input_hit(&self, local: WorldPoint, _control: bool) -> input::HitTarget {
        if let Some(id) = self.resize_target()
            && self
                .selection_corners(&id)
                .is_some_and(|p| p[2].distance(local) <= 10.)
        {
            return input::HitTarget::ObjectResize(id.to_string());
        }
        let world = self.editor.viewport.screen_to_world(local);
        if let Some(id) = self.hit(world) {
            input::HitTarget::Object(id.to_string())
        } else {
            input::HitTarget::Background
        }
    }
    fn plugin_event(&mut self, event: input::CanvasEvent, cx: &mut Context<Self>) {
        let report = self
            .router
            .dispatch(&mut self.editor, &mut self.interactions, event);
        self.apply_routed_report(report, cx);
    }
    fn apply_routed_report(&mut self, report: input::scene::RoutedReport, cx: &mut Context<Self>) {
        self.pending_native_focus |= report.focus_requested;
        for event in report.events {
            cx.emit(SceneViewEvent::Pointer(Box::new(event)));
        }
        if let Some(error) = report.errors.last() {
            self.interaction_error = Some(error.clone());
        }
        self.apply_input_report(report.default, cx);
    }

    /// Focus an attached live element without selecting it. Returns false if the
    /// native window declines focus (for example when focus is disabled), or if
    /// a queued listener action removes the logical target during notification.
    /// This does not activate an OS window or implement Tab/IME/accessibility.
    pub fn focus_element(
        &mut self,
        handle: gpui_flow_engine::ElementHandle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<bool, gpui_flow_engine::EngineError> {
        self.editor.engine.object(handle)?;
        if self.editor.engine.instance(handle)?.state != gpui_flow_engine::InstanceState::Attached {
            return Err(gpui_flow_engine::EngineError::DetachedHandle);
        }
        window.focus(&self.focus, cx);
        if !self.focus.is_focused(window) {
            return Ok(false);
        }
        let report =
            self.router
                .set_focus(&mut self.editor, &mut self.interactions, Some(handle))?;
        self.apply_routed_report(report, cx);
        self.pending_native_focus = false;
        let accepted = self.router.focused(&self.editor.engine) == Some(handle);
        if self.router.focused(&self.editor.engine).is_none() && self.focus.is_focused(window) {
            window.blur(cx);
        }
        Ok(accepted)
    }
    pub fn focused_element(&self) -> Option<gpui_flow_engine::ElementHandle> {
        self.router.focused(&self.editor.engine)
    }
    /// Clear logical focus and this view's native focus only. Another widget's
    /// native focus is never blurred by this operation.
    pub fn clear_element_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let report = self
            .router
            .clear_focus(&mut self.editor, &mut self.interactions);
        self.apply_routed_report(report, cx);
        self.pending_native_focus = false;
        if self.focus.is_focused(window) {
            window.blur(cx);
        }
    }

    pub(super) fn sync_requested_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.pending_native_focus) {
            return;
        }
        if self.router.focused(&self.editor.engine).is_some() {
            window.focus(&self.focus, cx);
            if !self.focus.is_focused(window) {
                let report = self
                    .router
                    .clear_focus(&mut self.editor, &mut self.interactions);
                self.apply_routed_report(report, cx);
                self.pending_native_focus = false;
                self.interaction_error =
                    Some("native window declined the queued focus request".into());
            }
        } else if self.focus.is_focused(window) {
            window.blur(cx);
        }
    }

    pub(super) fn down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let local = self.local(event.position);
        let hit = self.input_hit(local, event.modifiers.control);
        self.plugin_event(
            input::CanvasEvent::PointerDown(input::PointerInput {
                position: local,
                button: button(event.button),
                modifiers: modifiers(event.modifiers, self.router.space_held()),
                hit,
                pointer_id: 0,
            }),
            cx,
        );
    }
    pub(super) fn moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let local = self.local(event.position);
        if event.pressed_button.is_none()
            && (self.interactions.has_capture() || self.router.has_pressed(&self.editor.engine))
        {
            self.cancel_input(cx);
        }
        let hit = self.input_hit(local, event.modifiers.control);
        self.plugin_event(
            input::CanvasEvent::PointerMove(input::PointerInput {
                position: local,
                button: event.pressed_button.and_then(button),
                modifiers: modifiers(event.modifiers, self.router.space_held()),
                hit,
                pointer_id: 0,
            }),
            cx,
        );
    }
    pub(super) fn up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let local = self.local(event.position);
        self.plugin_event(
            input::CanvasEvent::PointerUp(input::PointerInput {
                position: local,
                button: button(event.button),
                modifiers: modifiers(event.modifiers, self.router.space_held()),
                hit: self.input_hit(local, event.modifiers.control),
                pointer_id: 0,
            }),
            cx,
        );
    }
    pub(super) fn cancel_input(&mut self, cx: &mut Context<Self>) {
        self.plugin_event(input::CanvasEvent::PointerCancel, cx);
    }
    pub(super) fn pointer_leave(&mut self, cx: &mut Context<Self>) {
        self.plugin_event(input::CanvasEvent::PointerLeave, cx);
    }
    pub(super) fn focus_lost(&mut self, cx: &mut Context<Self>) {
        self.plugin_event(input::CanvasEvent::FocusLost, cx);
        self.pending_native_focus = false;
    }
    pub(super) fn scroll(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = match event.delta {
            ScrollDelta::Pixels(p) => WorldPoint::new(p.x.into(), p.y.into()),
            ScrollDelta::Lines(p) => WorldPoint::new(p.x * 24., p.y * 24.),
        };
        self.plugin_event(
            input::CanvasEvent::Wheel {
                position: self.local(event.position),
                delta,
                modifiers: modifiers(event.modifiers, self.router.space_held()),
            },
            cx,
        );
    }
    pub(super) fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        self.plugin_event(
            input::CanvasEvent::KeyDown {
                key: key_code(event.keystroke.key.as_str()),
                modifiers: modifiers(event.keystroke.modifiers, self.router.space_held()),
            },
            cx,
        );
        cx.notify();
    }
    pub(super) fn key_up(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        self.plugin_event(
            input::CanvasEvent::KeyUp {
                key: key_code(event.keystroke.key.as_str()),
                modifiers: modifiers(event.keystroke.modifiers, self.router.space_held()),
            },
            cx,
        );
        cx.notify();
    }
    pub(super) fn input_cursor(&self) -> CursorStyle {
        match self.interactions.cursor() {
            input::CanvasCursor::Default
                if self.router.space_held() && self.interactions.is_enabled("pan") =>
            {
                CursorStyle::OpenHand
            }
            input::CanvasCursor::Default => CursorStyle::Arrow,
            input::CanvasCursor::Grab => CursorStyle::OpenHand,
            input::CanvasCursor::Grabbing => CursorStyle::ClosedHand,
            input::CanvasCursor::Resize => CursorStyle::ResizeUpLeftDownRight,
            input::CanvasCursor::Crosshair => CursorStyle::Crosshair,
            input::CanvasCursor::NotAllowed => CursorStyle::OperationNotAllowed,
        }
    }
}

fn key_code(text: &str) -> input::KeyCode {
    match text {
        "space" | " " => input::KeyCode::Space,
        "escape" => input::KeyCode::Escape,
        "delete" => input::KeyCode::Delete,
        "backspace" => input::KeyCode::Backspace,
        text => {
            let mut chars = text.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => input::KeyCode::Character(c),
                _ => input::KeyCode::Other(text.into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::key_code;
    use core::prelude::v1::test;
    use gpui_flow_interaction::KeyCode;
    #[test]
    fn native_key_names_normalize_space_and_named_keys_for_both_edges() {
        assert_eq!(key_code("space"), KeyCode::Space);
        assert_eq!(key_code(" "), KeyCode::Space);
        assert_eq!(key_code("escape"), KeyCode::Escape);
        assert_eq!(key_code("delete"), KeyCode::Delete);
        assert_eq!(key_code("backspace"), KeyCode::Backspace);
        assert_eq!(key_code("a"), KeyCode::Character('a'));
        assert_eq!(key_code("中"), KeyCode::Character('中'));
        assert_eq!(key_code("f12"), KeyCode::Other("f12".into()));
    }
}
