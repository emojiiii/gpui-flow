use super::*;
use gpui_flow_interaction::{KeyCode, Modifiers as InputModifiers, PointerButton};
fn modifiers(m: gpui::Modifiers) -> InputModifiers {
    InputModifiers {
        shift: m.shift,
        control: m.control,
        alt: m.alt,
        meta: m.platform,
        space: false,
    }
}
fn button(b: MouseButton) -> Option<PointerButton> {
    match b {
        MouseButton::Left => Some(PointerButton::Left),
        MouseButton::Middle => Some(PointerButton::Middle),
        MouseButton::Right => Some(PointerButton::Right),
        _ => None,
    }
}
fn key(k: &str) -> KeyCode {
    match k {
        "space" | " " => KeyCode::Space,
        "escape" => KeyCode::Escape,
        "delete" => KeyCode::Delete,
        "backspace" => KeyCode::Backspace,
        k => {
            let mut chars = k.chars();
            match (chars.next(), chars.next()) {
                (Some(c), None) => KeyCode::Character(c),
                _ => KeyCode::Other(k.into()),
            }
        }
    }
}
impl LayeredApp {
    fn local(&self, p: gpui::Point<Pixels>) -> ScenePoint {
        ScenePoint::new(p.x.into(), p.y.into()) - self.origin
    }
    fn down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
        if !self.focus.is_focused(window) {
            return;
        }
        let pointer = self.native_pointer.down(
            event,
            self.local(event.position),
            HitTarget::Background,
            false,
        );
        self.dispatch_input(CanvasEvent::PointerDown(pointer), cx);
    }
    fn moved(&mut self, event: &MouseMoveEvent, _: &mut Window, cx: &mut Context<Self>) {
        let pointer = self.native_pointer.moved(
            event,
            self.local(event.position),
            HitTarget::Background,
            false,
        );
        self.dispatch_input(CanvasEvent::PointerMove(pointer), cx);
    }
    fn pressure(&mut self, event: &MousePressureEvent, _: &mut Window, cx: &mut Context<Self>) {
        let pointer = self.native_pointer.pressure(
            event,
            self.local(event.position),
            HitTarget::Background,
            false,
        );
        self.dispatch_input(CanvasEvent::PointerMove(pointer), cx);
    }
    fn up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        let pointer = self.native_pointer.up(
            event,
            self.local(event.position),
            HitTarget::Background,
            false,
        );
        self.dispatch_input(CanvasEvent::PointerUp(pointer), cx);
    }
    fn outside_up(&mut self, event: &MouseUpEvent, _: &mut Window, cx: &mut Context<Self>) {
        self.native_pointer.up(
            event,
            self.local(event.position),
            HitTarget::Background,
            false,
        );
        let target = self.routing.release_outside(
            0,
            button(event.button),
            &mut NativeAccess {
                layers: &self.layers,
                cx,
            },
        );
        if target.is_some() {
            cx.emit(LayeredAppEvent::Input {
                target,
                input: CanvasEvent::PointerCancel,
            });
            cx.notify();
        }
    }
    fn outside_move(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let input = self.native_pointer.moved(
            event,
            self.local(event.position),
            HitTarget::Background,
            false,
        );
        let target = self.routing.move_outside(
            input.clone(),
            &mut NativeAccess {
                layers: &self.layers,
                cx,
            },
        );
        if target.is_some() {
            cx.emit(LayeredAppEvent::Input {
                target,
                input: CanvasEvent::PointerMove(input),
            });
            cx.notify();
        }
    }
    fn subscribe(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let id = window.window_handle().window_id().as_u64();
        if self.input_window == Some(id) {
            return;
        }
        self.input_window = Some(id);
        self.subscriptions.clear();
        self.subscriptions.push(cx.on_focus_out(
            &self.focus.clone(),
            window,
            |this, _, window, cx| {
                if !this.focus.is_focused(window) {
                    this.dispatch_input(CanvasEvent::FocusLost, cx);
                }
            },
        ));
        self.subscriptions
            .push(cx.observe_window_activation(window, |this, window, cx| {
                if !window.is_window_active() {
                    this.dispatch_input(CanvasEvent::FocusLost, cx);
                }
            }));
        self.subscriptions
            .push(cx.observe_window_visibility(window, |this, _, window, cx| {
                if !window.is_visible() {
                    this.dispatch_input(CanvasEvent::FocusLost, cx);
                }
            }));
    }
}
impl Render for LayeredApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.subscribe(window, cx);
        let entity = cx.entity().downgrade();
        let origin = canvas(
            move |bounds, _, cx| {
                let _ = entity.update(cx, |this, _| {
                    this.origin = ScenePoint::new(bounds.origin.x.into(), bounds.origin.y.into())
                });
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let cursor = self
            .routing
            .pressed_layer()
            .or(self.routing.hovered)
            .or(self.routing.focused)
            .and_then(|id| self.layers.get(&id))
            .map_or(CursorStyle::Arrow, |layer| {
                layer.view.read(cx).layer_cursor()
            });
        let mut view = div()
            .id("layered-scene-app")
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(rgb(self.background))
            .track_focus(&self.focus)
            .cursor(cursor)
            .child(origin);
        for (id, options) in &self.routing.entries {
            if options.visible
                && let Some(layer) = self.layers.get(id)
            {
                view = view.child(
                    div()
                        .id(("scene-layer", id.serial))
                        .absolute()
                        .top_0()
                        .left_0()
                        .size_full()
                        .child(layer.view.clone()),
                );
            }
        }
        let outside_entity = cx.entity().downgrade();
        view = view.child(
            canvas(
                |_, _, _| (),
                move |bounds, _, window, _| {
                    let entity = outside_entity.clone();
                    // GPUI div.on_mouse_move filters by hover. This bounded window
                    // listener is registered only during paint and replaced each frame.
                    window.on_mouse_event(move |event: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Bubble && !bounds.contains(&event.position) {
                            let _ = entity.update(cx, |this, cx| this.outside_move(event, cx));
                        }
                    });
                },
            )
            .absolute()
            .size_full(),
        );
        view.on_mouse_down(MouseButton::Left, cx.listener(Self::down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::down))
            .on_mouse_move(cx.listener(Self::moved))
            .on_mouse_pressure(cx.listener(Self::pressure))
            .on_mouse_up(MouseButton::Left, cx.listener(Self::up))
            .on_mouse_up(MouseButton::Middle, cx.listener(Self::up))
            .on_mouse_up(MouseButton::Right, cx.listener(Self::up))
            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::outside_up))
            .on_mouse_up_out(MouseButton::Middle, cx.listener(Self::outside_up))
            .on_mouse_up_out(MouseButton::Right, cx.listener(Self::outside_up))
            .on_hover(cx.listener(|this, hovered, _, cx| {
                if !hovered {
                    this.dispatch_input(CanvasEvent::PointerLeave, cx);
                }
            }))
            .on_scroll(cx.listener(|this, event: &ScrollEvent, _, cx| {
                let delta = match event.delta {
                    ScrollDelta::Pixels(p) => ScenePoint::new(p.x.into(), p.y.into()),
                    ScrollDelta::Lines(p) => ScenePoint::new(p.x * 24., p.y * 24.),
                };
                this.dispatch_input(
                    CanvasEvent::Wheel {
                        position: this.local(event.position),
                        delta,
                        modifiers: modifiers(event.modifiers),
                    },
                    cx,
                );
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if this.focus.is_focused(window) {
                    this.dispatch_input(
                        CanvasEvent::KeyDown {
                            key: key(&event.keystroke.key),
                            modifiers: modifiers(event.keystroke.modifiers),
                        },
                        cx,
                    );
                }
            }))
            .on_key_up(cx.listener(|this, event: &KeyUpEvent, window, cx| {
                if this.focus.is_focused(window) {
                    this.dispatch_input(
                        CanvasEvent::KeyUp {
                            key: key(&event.keystroke.key),
                            modifiers: modifiers(event.keystroke.modifiers),
                        },
                        cx,
                    );
                }
            }))
    }
}
