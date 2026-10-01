//! Tree listeners run before default behaviors. Gesture cleanup is mandatory even
//! when pointer-up is prevented; callbacks cannot strand an editor transaction.
mod focus;
use super::{ScenePluginHost, action_queue::SceneAction};
use crate::{CanvasEvent, DispatchReport, HitTarget, KeyCode, PointerButton};
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, InstanceState, SceneEngine};
use gpui_flow_scene::{
    DisplayId, EventInput, EventKind, InputModifiers, Point, PointerDevice, SceneEvent,
};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct SceneEventRouter {
    pressed: Option<(ElementHandle, Point, u64, Option<PointerButton>)>,
    hover: Option<ElementHandle>,
    focus: Option<ElementHandle>,
    pressed_epoch: u64,
    hover_epoch: u64,
    focus_epoch: u64,
    position: Point,
    sequence: u64,
    held_keys: BTreeSet<KeyCode>,
    space_pan: bool,
    actions: super::action_queue::SceneActionQueue,
}
#[derive(Default)]
pub struct RoutedReport {
    /// An explicit host/listener focus request was processed. Native adapters
    /// should synchronize once to the final live logical target.
    pub focus_requested: bool,
    pub default: DispatchReport,
    pub events: Vec<SceneEvent>,
    pub errors: Vec<String>,
}
/// Derived pointer/focus state. Hover and press include the current target's
/// ancestors; focus is exact. Detached/replaced/remounted targets are inactive.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SceneInteractionState {
    pub hover: bool,
    pub press: bool,
    pub focus: bool,
}
impl SceneEventRouter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn action_sender(&self) -> super::SceneActionSender {
        self.actions.sender.clone()
    }
    fn current(engine: &SceneEngine, handle: ElementHandle, epoch: u64) -> bool {
        engine
            .instance(handle)
            .is_ok_and(|i| i.state == InstanceState::Attached && i.attachment_epoch == epoch)
    }
    fn epoch(engine: &SceneEngine, handle: Option<ElementHandle>) -> u64 {
        handle
            .and_then(|h| engine.instance(h).ok())
            .map_or(0, |i| i.attachment_epoch)
    }
    pub fn has_pressed(&self, engine: &SceneEngine) -> bool {
        self.pressed
            .is_some_and(|p| Self::current(engine, p.0, self.pressed_epoch))
    }
    pub fn state(&self, engine: &SceneEngine, handle: ElementHandle) -> SceneInteractionState {
        let Ok(instance) = engine.instance(handle) else {
            return SceneInteractionState::default();
        };
        if instance.state != InstanceState::Attached {
            return SceneInteractionState::default();
        }
        let contains = |target: Option<ElementHandle>, epoch| {
            target
                .filter(|h| Self::current(engine, *h, epoch))
                .and_then(|h| engine.instance(h).ok())
                .and_then(|i| engine.scene().ancestor_path(&i.id).ok())
                .is_some_and(|path| path.contains(&instance.id))
        };
        SceneInteractionState {
            hover: contains(self.hover, self.hover_epoch),
            press: contains(self.pressed.map(|p| p.0), self.pressed_epoch),
            focus: self.focus == Some(handle) && Self::current(engine, handle, self.focus_epoch),
        }
    }
    /// Accepted Space navigation modifier, scoped to this router. A prevented
    /// initial KeyDown never arms panning; physical release/cancel always clears it.
    pub fn space_held(&self) -> bool {
        self.space_pan
    }
    fn clear_keys(&mut self) {
        self.held_keys.clear();
        self.space_pan = false;
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
        self.actions.begin_operation();
        self.dispatch_with_budget(editor, host, event, &mut 256)
    }
    fn dispatch_with_budget(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        mut event: CanvasEvent,
        action_budget: &mut usize,
    ) -> RoutedReport {
        let mut out = RoutedReport::default();
        // Instance loss cannot leave keyboard defaults armed for a replacement
        // object with the same public ID. A new click establishes fresh focus.
        if self
            .focus
            .is_some_and(|handle| !Self::current(&editor.engine, handle, self.focus_epoch))
        {
            self.focus = None;
            self.clear_keys();
        }
        if self
            .hover
            .is_some_and(|handle| !Self::current(&editor.engine, handle, self.hover_epoch))
        {
            self.hover = None;
        }
        let repeat = match &event {
            CanvasEvent::KeyDown { key, .. } => !self.held_keys.insert(key.clone()),
            CanvasEvent::KeyUp { key, .. } => {
                self.held_keys.remove(key);
                if *key == KeyCode::Space {
                    self.space_pan = false;
                }
                false
            }
            CanvasEvent::PointerCancel | CanvasEvent::FocusLost => {
                self.clear_keys();
                false
            }
            _ => false,
        };
        // Native adapters need not maintain a competing Space latch. Direct
        // host-provided pointer modifiers remain an explicit supported input.
        if let CanvasEvent::PointerDown(pointer)
        | CanvasEvent::PointerMove(pointer)
        | CanvasEvent::PointerUp(pointer) = &mut event
        {
            pointer.modifiers.space |= self.space_pan;
        }
        if editor.reconcile_external() && host.has_capture() {
            self.pressed = None;
            self.clear_keys();
            out.default = host.dispatch(editor, CanvasEvent::PointerCancel);
            return out;
        }
        if let Some((handle, _, pointer_id, _)) = self.pressed {
            if !Self::current(&editor.engine, handle, self.pressed_epoch) {
                self.pressed = None;
                self.clear_keys();
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
            if let HitTarget::Object(id)
            | HitTarget::ObjectResize(id)
            | HitTarget::ObjectResizeHandle { id, .. } = &pointer.hit
            {
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
        let input = metadata(&event, repeat);
        if matches!(
            event,
            CanvasEvent::PointerMove(_)
                | CanvasEvent::PointerLeave
                | CanvasEvent::PointerCancel
                | CanvasEvent::FocusLost
        ) && self.hover != hit
        {
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
            self.hover_epoch = Self::epoch(&editor.engine, hit);
        }
        if matches!(event, CanvasEvent::PointerDown(_)) && self.pressed.is_some() {
            let cancelled =
                self.dispatch_with_budget(editor, host, CanvasEvent::PointerCancel, action_budget);
            out.focus_requested |= cancelled.focus_requested;
            out.events.extend(cancelled.events);
            out.errors.extend(cancelled.errors);
        }
        let target = self.pressed.map(|p| p.0).or(hit);
        let kind = match &event {
            CanvasEvent::PointerDown(_) => EventKind::PointerDown,
            CanvasEvent::PointerMove(_) => EventKind::PointerMove,
            CanvasEvent::PointerUp(_) => EventKind::PointerUp,
            CanvasEvent::PointerLeave => EventKind::PointerLeave,
            CanvasEvent::PointerCancel | CanvasEvent::FocusLost => EventKind::PointerCancel,
            CanvasEvent::Wheel { .. } => EventKind::Wheel,
            CanvasEvent::KeyDown { .. } => EventKind::KeyDown,
            CanvasEvent::KeyUp { .. } => EventKind::KeyUp,
        };
        let target = if matches!(event, CanvasEvent::PointerLeave) {
            None // The complete leave path was emitted above; do not emit twice.
        } else if matches!(
            event,
            CanvasEvent::KeyDown { .. } | CanvasEvent::KeyUp { .. }
        ) {
            self.focus
        } else {
            target
        };
        let prevented = self.emit(editor, target, kind, input.clone(), &mut out);
        if matches!(
            event,
            CanvasEvent::KeyDown {
                key: KeyCode::Space,
                ..
            }
        ) && !repeat
            && !prevented
        {
            self.space_pan = true;
        }
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
            self.transfer_focus(editor, host, hit, false, &mut out, action_budget);
            // A listener-requested focus transfer may have cancelled the new
            // default capture. Do not re-arm its press/click after that transfer.
            if !out.focus_requested {
                self.pressed = hit.map(|h| (h, p.position, p.pointer_id, p.button));
                self.pressed_epoch = Self::epoch(&editor.engine, hit);
            }
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
            self.clear_keys();
        }
        if matches!(event, CanvasEvent::PointerCancel) {
            self.hover = None;
        }
        if matches!(event, CanvasEvent::FocusLost) {
            self.transfer_focus(editor, host, None, false, &mut out, action_budget);
            self.hover = None;
        }
        self.finish_dispatch(
            editor,
            host,
            &mut out,
            action_budget,
            !matches!(event, CanvasEvent::FocusLost),
        );
        out
    }
    fn finish_dispatch(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        out: &mut RoutedReport,
        budget: &mut usize,
        allow_focus: bool,
    ) {
        if let Some(target) = self.drain_actions(editor, host, out, budget, allow_focus) {
            self.transfer_focus(editor, host, target, true, out, budget);
        }
        self.retire_invalid(editor, host, out);
    }
    fn drain_actions(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        out: &mut RoutedReport,
        budget: &mut usize,
        allow_focus: bool,
    ) -> Option<Option<ElementHandle>> {
        let revision = editor.engine.revision();
        let selection = editor.selection.clone();
        let viewport = editor.viewport;
        let mut focus = None;
        while *budget > 0 {
            let Some((origin, action)) = self.actions.next() else {
                break;
            };
            *budget -= 1;
            if Self::id(editor, origin).is_none() {
                out.errors
                    .push("discarded action from a detached or destroyed instance".into());
                continue;
            }
            match action {
                SceneAction::Command(command) => {
                    if let Err(error) = host.execute(editor, *command) {
                        out.errors.push(error.to_string());
                    }
                }
                SceneAction::Focus(request) => {
                    if !allow_focus {
                        out.errors
                            .push("focus request ignored during explicit focus clearing".into());
                        continue;
                    }
                    match request.validate(&editor.engine) {
                        Ok(()) => focus = Some(request),
                        Err(error) => out.errors.push(error.to_string()),
                    }
                }
            }
        }
        if *budget == 0 {
            let discarded = self.actions.discard_pending_focus();
            if discarded > 0 || out.focus_requested {
                let error = "action budget exhausted; remaining focus redirects discarded";
                if !out.errors.iter().any(|e| e == error) {
                    out.errors.push(error.into());
                }
            }
        }
        self.retire_invalid(editor, host, out);
        out.default.document_changed |= revision != editor.engine.revision();
        out.default.selection_changed |= selection != editor.selection;
        out.default.viewport_changed |= viewport != editor.viewport;
        focus.and_then(|request| match request.validate(&editor.engine) {
            Ok(()) => Some(request.target()),
            Err(error) => {
                out.errors.push(error.to_string());
                None
            }
        })
    }
    fn retire_invalid(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        out: &mut RoutedReport,
    ) {
        // Listener actions and keyboard defaults may remove/reset their own
        // target. Retire focus/Space immediately, not on the next input tick.
        if self
            .focus
            .is_some_and(|handle| !Self::current(&editor.engine, handle, self.focus_epoch))
        {
            self.focus = None;
            self.clear_keys();
        }
        if self
            .hover
            .is_some_and(|handle| !Self::current(&editor.engine, handle, self.hover_epoch))
        {
            self.hover = None;
        }
        if self.pressed.is_some_and(|(handle, _, _, _)| {
            !Self::current(&editor.engine, handle, self.pressed_epoch)
        }) {
            self.pressed = None;
            self.clear_keys();
            let cancelled = host.dispatch(editor, CanvasEvent::PointerCancel);
            out.default.effects.extend(cancelled.effects);
            out.default.errors.extend(cancelled.errors);
            out.default.handled |= cancelled.handled;
            out.default.document_changed |= cancelled.document_changed;
            out.default.selection_changed |= cancelled.selection_changed;
            out.default.viewport_changed |= cancelled.viewport_changed;
            out.default.default_prevented |= cancelled.default_prevented;
            out.default.propagation_stopped |= cancelled.propagation_stopped;
        }
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
fn metadata(event: &CanvasEvent, repeat: bool) -> Option<EventInput> {
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
        CanvasEvent::KeyDown { key, modifiers } | CanvasEvent::KeyUp { key, modifiers } => {
            Some(EventInput::Key {
                key: match key {
                    KeyCode::Character(c) => c.to_string(),
                    KeyCode::Other(s) => s.clone(),
                    other => format!("{other:?}"),
                },
                modifiers: mods(*modifiers),
                repeat,
            })
        }
        _ => None,
    }
}
