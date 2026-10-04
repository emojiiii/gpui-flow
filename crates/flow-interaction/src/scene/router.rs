//! Tree listeners run before default behaviors. Gesture cleanup is mandatory even
//! when pointer-up is prevented; callbacks cannot strand an editor transaction.
mod focus;
mod semantic;
pub use semantic::{SemanticDefault, SemanticDispatch};
mod pointer;
use super::{ScenePluginHost, action_queue::SceneAction};
use crate::{CanvasEvent, DispatchReport, HitTarget, KeyCode, PointerIdentity};
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, EngineError, InstanceState, SceneEngine};
use gpui_flow_scene::{
    DisplayId, EventInput, EventKind, InputModifiers, Point, PointerDevice, SceneEvent,
};
pub use pointer::PointerGestureProfile;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Default)]
pub struct SceneEventRouter {
    gesture_profile: PointerGestureProfile,
    contacts: BTreeMap<PointerIdentity, pointer::Contact>,
    default_pointer: Option<PointerIdentity>,
    recent_taps: VecDeque<pointer::Tap>,
    clock_millis: Option<u64>,
    event_time: Option<u64>,
    hover: Option<ElementHandle>,
    focus: Option<ElementHandle>,
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
    /// Choose a profile before accepting input. Existing documents are not changed.
    pub fn with_gesture_profile(profile: PointerGestureProfile) -> Self {
        Self {
            gesture_profile: profile,
            ..Self::default()
        }
    }
    pub fn gesture_profile(&self) -> PointerGestureProfile {
        self.gesture_profile
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
        self.contacts
            .values()
            .any(|p| p.target.is_some_and(|h| Self::current(engine, h, p.epoch)))
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
            press: self.contacts.values().any(|p| contains(p.target, p.epoch)),
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
        event.timestamp_millis = self.event_time;
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
        self.dispatch_with_budget(editor, host, event, None, &mut 256)
    }
    /// Dispatch a wheel event using an adapter's already-resolved presentation
    /// hit. `None` explicitly means background; it never triggers a canonical
    /// repick. An active press still owns the event, as in ordinary dispatch.
    ///
    /// A supplied handle must belong to this engine and be currently attached.
    /// Invalid handles fail before changing input, editor or queued-action state.
    /// This is synchronous trusted-host input, not a deferred mount token.
    pub fn dispatch_picked_wheel(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        position: Point,
        delta: Point,
        modifiers: crate::Modifiers,
        target: Option<ElementHandle>,
    ) -> Result<RoutedReport, EngineError> {
        if let Some(target) = target {
            editor.engine.object(target)?;
            if editor.engine.instance(target)?.state != InstanceState::Attached {
                return Err(EngineError::DetachedHandle);
            }
        }
        self.actions.begin_operation();
        Ok(self.dispatch_with_budget(
            editor,
            host,
            CanvasEvent::Wheel {
                position,
                delta,
                modifiers,
            },
            Some(target),
            &mut 256,
        ))
    }
    fn dispatch_with_budget(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        mut event: CanvasEvent,
        wheel_target: Option<Option<ElementHandle>>,
        action_budget: &mut usize,
    ) -> RoutedReport {
        let mut out = RoutedReport::default();
        self.event_time = event.pointer().and_then(|p| p.timestamp_millis);
        if let Some(pointer) = event.pointer() {
            if !pointer.position.is_finite() {
                let identity = pointer.identity();
                self.cancel_contacts(editor, host, Some(identity), &mut out);
                out.errors
                    .push("pointer position must be finite; matching contact cancelled".into());
                self.finish_dispatch(editor, host, &mut out, action_budget, true);
                return out;
            }
            if pointer
                .pressure
                .is_some_and(|p| !p.is_finite() || !(0.0..=1.0).contains(&p))
            {
                out.errors
                    .push("pointer pressure must be finite and in 0..=1".into());
                return out;
            }
            if let Some(now) = pointer.timestamp_millis
                && !self.accept_time(now, &mut out)
            {
                return out;
            }
        }
        let navigation_stopped = matches!(
            event,
            CanvasEvent::PointerDown(_) | CanvasEvent::Wheel { .. } | CanvasEvent::KeyDown { .. }
        ) && host.stop_navigation_tail();
        if editor.reconcile_external() && host.has_capture() {
            self.cancel_contacts(editor, host, None, &mut out);
            self.clear_keys();
        }
        self.retire_invalid(editor, host, &mut out);
        if let CanvasEvent::PointerDown(pointer)
        | CanvasEvent::PointerMove(pointer)
        | CanvasEvent::PointerUp(pointer) = &mut event
        {
            pointer.modifiers.space |= self.space_pan;
        }
        if event.pointer().is_some() {
            self.dispatch_pointer(editor, host, event.clone(), &mut out, action_budget);
        } else if let CanvasEvent::PointerCancelFor(identity) = event {
            self.cancel_contacts(editor, host, Some(identity), &mut out);
        } else if matches!(event, CanvasEvent::PointerCancel | CanvasEvent::FocusLost) {
            self.cancel_contacts(editor, host, None, &mut out);
            self.update_hover(editor, None, None, &mut out);
            self.clear_keys();
            if matches!(event, CanvasEvent::FocusLost) {
                self.transfer_focus(editor, host, None, false, &mut out, action_budget);
            }
        } else if matches!(event, CanvasEvent::PointerLeave) {
            self.update_hover(editor, None, None, &mut out);
        } else {
            let repeat = match &event {
                CanvasEvent::KeyDown { key, .. } => !self.held_keys.insert(key.clone()),
                CanvasEvent::KeyUp { key, .. } => {
                    self.held_keys.remove(key);
                    if *key == KeyCode::Space {
                        self.space_pan = false;
                    }
                    false
                }
                _ => false,
            };
            let (target, kind) = match &event {
                CanvasEvent::Wheel { position, .. } => {
                    self.position = *position;
                    let hit = wheel_target.unwrap_or_else(|| {
                        editor
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
                            .and_then(|h| editor.engine.handle(&h.id))
                    });
                    (self.primary_target().or(hit), EventKind::Wheel)
                }
                CanvasEvent::KeyDown { .. } => (self.focus, EventKind::KeyDown),
                CanvasEvent::KeyUp { .. } => (self.focus, EventKind::KeyUp),
                _ => unreachable!(),
            };
            let prevented = self.emit(editor, target, kind, metadata(&event, repeat), &mut out);
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
            if matches!(
                event,
                CanvasEvent::KeyDown {
                    key: KeyCode::Escape,
                    ..
                }
            ) {
                self.cancel_contacts(editor, host, None, &mut out);
                self.clear_keys();
            } else if !prevented {
                pointer::merge_report(&mut out.default, host.dispatch(editor, event.clone()));
            }
            out.default.default_prevented |= prevented;
        }
        out.default.handled |= navigation_stopped;
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
        let stale: Vec<_> = self
            .contacts
            .iter()
            .filter(|(id, p)| {
                p.target
                    .is_some_and(|h| !Self::current(&editor.engine, h, p.epoch))
                    || (self.default_pointer == Some(**id)
                        && p.capture_owner
                            .as_deref()
                            .is_some_and(|owner| host.capture_owner() != Some(owner)))
            })
            .map(|(id, _)| *id)
            .collect();
        for identity in stale {
            self.cancel_contacts(editor, host, Some(identity), out);
        }
        self.recent_taps
            .retain(|tap| Self::current(&editor.engine, tap.target, tap.epoch));
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
            device: p.device,
            button: p.button.map(|b| match b {
                crate::PointerButton::Left => 0,
                crate::PointerButton::Middle => 1,
                crate::PointerButton::Right => 2,
            }),
            buttons: p.held_buttons(matches!(event, CanvasEvent::PointerUp(_))),
            pressure: p.pressure,
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
