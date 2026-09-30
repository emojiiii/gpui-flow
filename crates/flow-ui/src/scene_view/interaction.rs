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
        for event in report.events {
            cx.emit(SceneViewEvent::Pointer(Box::new(event)));
        }
        if let Some(error) = report.errors.last() {
            self.interaction_error = Some(error.clone());
        }
        self.apply_input_report(report.default, cx);
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
                modifiers: modifiers(event.modifiers, self.space_held),
                hit,
                pointer_id: 0,
            }),
            cx,
        );
    }
    pub(super) fn moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let local = self.local(event.position);
        if event.pressed_button.is_none() && self.interactions.has_capture() {
            self.cancel_input(cx);
        }
        let hit = self.input_hit(local, event.modifiers.control);
        self.plugin_event(
            input::CanvasEvent::PointerMove(input::PointerInput {
                position: local,
                button: event.pressed_button.and_then(button),
                modifiers: modifiers(event.modifiers, self.space_held),
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
                modifiers: modifiers(event.modifiers, self.space_held),
                hit: self.input_hit(local, event.modifiers.control),
                pointer_id: 0,
            }),
            cx,
        );
    }
    pub(super) fn cancel_input(&mut self, cx: &mut Context<Self>) {
        self.plugin_event(input::CanvasEvent::PointerCancel, cx);
    }
    pub(super) fn focus_lost(&mut self, cx: &mut Context<Self>) {
        self.space_held = false;
        self.cancel_input(cx);
        self.plugin_event(input::CanvasEvent::FocusLost, cx);
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
                modifiers: modifiers(event.modifiers, self.space_held),
            },
            cx,
        );
    }
    pub(super) fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let text = event.keystroke.key.as_str();
        if text == "space" || text == " " {
            self.space_held = true;
            cx.notify();
            return;
        }
        let key = match text {
            "escape" => input::KeyCode::Escape,
            "delete" => input::KeyCode::Delete,
            "backspace" => input::KeyCode::Backspace,
            t => {
                let mut chars = t.chars();
                match (chars.next(), chars.next()) {
                    (Some(c), None) => input::KeyCode::Character(c),
                    _ => input::KeyCode::Other(t.into()),
                }
            }
        };
        self.plugin_event(
            input::CanvasEvent::KeyDown {
                key,
                modifiers: modifiers(event.keystroke.modifiers, self.space_held),
            },
            cx,
        );
    }
    pub(super) fn key_up(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        if matches!(event.keystroke.key.as_str(), "space" | " ") {
            self.space_held = false;
            cx.notify();
        }
    }
    pub(super) fn input_cursor(&self) -> CursorStyle {
        match self.interactions.cursor() {
            input::CanvasCursor::Default
                if self.space_held && self.interactions.is_enabled("pan") =>
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
