//! Tree listeners run before default behaviors. Gesture cleanup is mandatory even
//! when pointer-up is prevented; callbacks cannot strand an editor transaction.
use super::ScenePluginHost;
use crate::{CanvasEvent, DispatchReport, HitTarget, KeyCode, PointerButton};
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, InstanceState};
use gpui_flow_scene::{
    DisplayId, EventInput, EventKind, InputModifiers, Point, PointerDevice, SceneEvent,
};

#[derive(Default)]
pub struct SceneEventRouter {
    pressed: Option<(ElementHandle, Point, u64, Option<PointerButton>)>,
    hover: Option<ElementHandle>,
    focus: Option<ElementHandle>,
    position: Point,
    sequence: u64,
    actions: super::action_queue::SceneActionQueue,
}
#[derive(Default)]
pub struct RoutedReport {
    pub default: DispatchReport,
    pub events: Vec<SceneEvent>,
    pub errors: Vec<String>,
}
impl SceneEventRouter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn action_sender(&self) -> super::SceneActionSender {
        self.actions.sender.clone()
    }
    fn id(editor: &SceneEditor, handle: ElementHandle) -> Option<DisplayId> {
        editor
            .engine
            .instance(handle)
            .ok()
            .filter(|i| i.state == InstanceState::Attached)
            .map(|i| i.id)
    }
    fn emit(
        &mut self,
        editor: &mut SceneEditor,
        handle: Option<ElementHandle>,
        kind: EventKind,
        input: Option<EventInput>,
        out: &mut RoutedReport,
    ) -> bool {
        let Some(id) = handle.and_then(|h| Self::id(editor, h)) else {
            return false;
        };
        self.sequence = self.sequence.saturating_add(1);
        let mut event =
            SceneEvent::new(kind, id).at(editor.viewport.screen_to_world(self.position));
        event.input = input;
        event.sequence = self.sequence;
        match editor.engine.dispatch(event) {
            Ok(result) => {
                let prevented = result.event.default_prevented();
                out.events.push(result.event);
                prevented
            }
            Err(error) => {
                out.errors.push(error.to_string());
                false
            }
        }
    }
    pub fn dispatch(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        event: CanvasEvent,
    ) -> RoutedReport {
        let mut out = RoutedReport::default();
        if editor.reconcile_external() && host.has_capture() {
            self.pressed = None;
            out.default = host.dispatch(editor, CanvasEvent::PointerCancel);
            return out;
        }
        if let Some((handle, _, pointer_id, _)) = self.pressed {
            if Self::id(editor, handle).is_none() {
                self.pressed = None;
                out.default = host.dispatch(editor, CanvasEvent::PointerCancel);
                return out;
            }
            if matches!(&event,CanvasEvent::PointerDown(p)|CanvasEvent::PointerMove(p)|CanvasEvent::PointerUp(p) if p.pointer_id!=pointer_id)
            {
                return out;
            }
        }
        let mut hit = None;
        if let Some(pointer) = event.pointer() {
            self.position = pointer.position;
            if let HitTarget::Object(id) | HitTarget::ObjectResize(id) = &pointer.hit {
                hit = editor.engine.handle(&DisplayId::from(id.clone()));
            }
        }
        if let CanvasEvent::Wheel { position, .. } = &event {
            self.position = *position;
            hit = editor
                .engine
                .hit_test(
                    editor.viewport.screen_to_world(*position),
                    gpui_flow_scene::HitTestOptions {
                        include_locked: true,
                        ..Default::default()
                    },
                )
                .ok()
                .flatten()
                .and_then(|h| editor.engine.handle(&h.id));
        }
        let input = metadata(&event);
        if matches!(event, CanvasEvent::PointerMove(_)) && self.hover != hit {
            self.emit(
                editor,
                self.hover,
                EventKind::PointerOut,
                input.clone(),
                &mut out,
            );
            let path = |handle: Option<ElementHandle>| -> Vec<ElementHandle> {
                handle
                    .and_then(|h| Self::id(editor, h))
                    .and_then(|id| editor.scene().ancestor_path(&id).ok())
                    .unwrap_or_default()
                    .iter()
                    .filter_map(|id| editor.engine.handle(id))
                    .collect()
            };
            let old_path = path(self.hover);
            let new_path = path(hit);
            for handle in old_path.iter().rev().filter(|h| !new_path.contains(h)) {
                self.emit(
                    editor,
                    Some(*handle),
                    EventKind::PointerLeave,
                    input.clone(),
                    &mut out,
                );
            }
            self.emit(editor, hit, EventKind::PointerOver, input.clone(), &mut out);
            for handle in new_path.iter().filter(|h| !old_path.contains(h)) {
                self.emit(
                    editor,
                    Some(*handle),
                    EventKind::PointerEnter,
                    input.clone(),
                    &mut out,
                );
            }
            self.hover = hit;
        }
        if matches!(event, CanvasEvent::PointerDown(_)) && self.pressed.is_some() {
            let cancelled = self.dispatch(editor, host, CanvasEvent::PointerCancel);
            out.events.extend(cancelled.events);
            out.errors.extend(cancelled.errors);
        }
        let target = self.pressed.map(|p| p.0).or(hit);
        let kind = match &event {
            CanvasEvent::PointerDown(_) => EventKind::PointerDown,
            CanvasEvent::PointerMove(_) => EventKind::PointerMove,
            CanvasEvent::PointerUp(_) => EventKind::PointerUp,
            CanvasEvent::PointerCancel | CanvasEvent::FocusLost => EventKind::PointerCancel,
            CanvasEvent::Wheel { .. } => EventKind::Wheel,
            CanvasEvent::KeyDown { .. } => EventKind::KeyDown,
        };
        let target = if matches!(event, CanvasEvent::KeyDown { .. }) {
            self.focus
        } else {
            target
        };
        let prevented = self.emit(editor, target, kind, input.clone(), &mut out);
        let escape = matches!(
            event,
            CanvasEvent::KeyDown {
                key: KeyCode::Escape,
                ..
            }
        );
        let terminal =
            matches!(event, CanvasEvent::PointerCancel | CanvasEvent::FocusLost) || escape;
        let matching_up = matches!(&event,CanvasEvent::PointerUp(p) if self.pressed.is_none_or(|old|old.2==p.pointer_id&&old.3==p.button));
        if terminal || (matching_up && prevented) {
            out.default = host.dispatch(editor, CanvasEvent::PointerCancel);
        } else if !prevented {
            out.default = host.dispatch(editor, event.clone());
        }
        out.default.default_prevented |= prevented;
        if let CanvasEvent::PointerDown(p) = &event
            && !prevented
        {
            if self.focus != hit {
                self.emit(editor, self.focus, EventKind::Blur, None, &mut out);
                self.emit(editor, hit, EventKind::Focus, None, &mut out);
                self.focus = hit;
            }
            self.pressed = hit.map(|h| (h, p.position, p.pointer_id, p.button));
        }
        if matching_up
            && let Some((handle, start, _, button)) = self.pressed.take()
            && !prevented
            && button == Some(PointerButton::Left)
            && hit == Some(handle)
            && start.distance(self.position) < 4.
        {
            self.emit(editor, Some(handle), EventKind::Click, input, &mut out);
        }
        if terminal {
            self.pressed = None;
        }
        if matches!(event, CanvasEvent::FocusLost) {
            self.emit(editor, self.focus, EventKind::Blur, None, &mut out);
            self.focus = None;
            self.hover = None;
        }
        let revision = editor.engine.revision();
        let selection = editor.selection.clone();
        let viewport = editor.viewport;
        for _ in 0..256 {
            let Ok((origin, command)) = self.actions.receiver.try_recv() else {
                break;
            };
            if Self::id(editor, origin).is_none() {
                out.errors
                    .push("discarded action from a detached or destroyed instance".into());
                continue;
            }
            if let Err(error) = host.execute(editor, command) {
                out.errors.push(error.to_string());
            }
        }
        out.default.document_changed |= revision != editor.engine.revision();
        out.default.selection_changed |= selection != editor.selection;
        out.default.viewport_changed |= viewport != editor.viewport;
        out
    }
}
fn mods(m: crate::Modifiers) -> InputModifiers {
    InputModifiers {
        shift: m.shift,
        control: m.control,
        alt: m.alt,
        meta: m.meta,
    }
}
fn metadata(event: &CanvasEvent) -> Option<EventInput> {
    if let Some(p) = event.pointer() {
        return Some(EventInput::Pointer {
            pointer_id: p.pointer_id,
            device: PointerDevice::Mouse,
            button: p.button.map(|b| match b {
                PointerButton::Left => 0,
                PointerButton::Middle => 1,
                PointerButton::Right => 2,
            }),
            buttons: if matches!(event, CanvasEvent::PointerUp(_)) {
                0
            } else {
                p.button.map_or(0, |b| match b {
                    PointerButton::Left => 1,
                    PointerButton::Right => 2,
                    PointerButton::Middle => 4,
                })
            },
            pressure: None,
            modifiers: mods(p.modifiers),
            cancelled: false,
        });
    }
    match event {
        CanvasEvent::Wheel {
            delta, modifiers, ..
        } => Some(EventInput::Wheel {
            delta: *delta,
            modifiers: mods(*modifiers),
        }),
        CanvasEvent::KeyDown { key, modifiers } => Some(EventInput::Key {
            key: match key {
                KeyCode::Character(c) => c.to_string(),
                KeyCode::Other(s) => s.clone(),
                other => format!("{other:?}"),
            },
            modifiers: mods(*modifiers),
            repeat: false,
        }),
        _ => None,
    }
}
