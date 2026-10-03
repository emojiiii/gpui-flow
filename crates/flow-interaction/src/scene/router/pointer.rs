//! One bounded contact table owns press routing and gesture recognition.
use super::*;
use crate::{PointerButton, PointerInput};

pub(super) const MAX_CONTACTS: usize = 16;
const SLOP: f32 = 4.0;
const LONG_PRESS_MS: u64 = 500;
const DOUBLE_TAP_MS: u64 = 300;

pub(super) struct Contact {
    pub target: Option<ElementHandle>,
    pub epoch: u64,
    pub capture_owner: Option<String>,
    down: PointerInput,
    down_sequence: u64,
    last: PointerInput,
    moved: bool,
    long_pressed: bool,
}
pub(super) struct Tap {
    pub target: ElementHandle,
    pub epoch: u64,
    device: PointerDevice,
    position: Point,
    up_time: u64,
    up_sequence: u64,
}
impl SceneEventRouter {
    /// Whether this identity has an accepted contact, including background.
    pub fn has_pointer(&self, identity: PointerIdentity) -> bool {
        self.contacts.contains_key(&identity)
    }
    /// Match the initiating button before cancelling an outside native release.
    pub fn matches_pointer_release(&self, pointer: &PointerInput) -> bool {
        self.contacts
            .get(&pointer.identity())
            .is_some_and(|p| p.down.button == pointer.button)
    }

    pub(super) fn primary_target(&self) -> Option<ElementHandle> {
        self.default_pointer
            .and_then(|id| self.contacts.get(&id))
            .and_then(|p| p.target)
    }
    pub(super) fn accept_time(&mut self, now: u64, out: &mut RoutedReport) -> bool {
        if self.clock_millis.is_some_and(|old| now < old) {
            out.errors
                .push("pointer clock must be monotonic within one router".into());
            return false;
        }
        self.clock_millis = Some(now);
        self.recent_taps
            .retain(|tap| now.saturating_sub(tap.up_time) <= DOUBLE_TAP_MS);
        true
    }
    /// True only while a timed, unmoved primary contact can become a long press.
    /// The native adapter schedules bounded frame callbacks while this is true.
    pub fn needs_gesture_frame(&self) -> bool {
        self.contacts.values().any(Self::long_press_candidate)
    }
    fn long_press_candidate(contact: &Contact) -> bool {
        !contact.moved
            && !contact.long_pressed
            && contact.target.is_some()
            && contact.down.button == Some(PointerButton::Left)
            && contact.down.timestamp_millis.is_some()
            && contact.last.timestamp_millis.is_some()
    }
    /// Advance the same monotonic clock used by PointerInput. No wall clock,
    /// background thread or document mutation is hidden inside recognition.
    pub fn advance_time(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        now: u64,
    ) -> RoutedReport {
        let mut out = RoutedReport::default();
        if !self.accept_time(now, &mut out) {
            return out;
        }
        self.actions.begin_operation();
        self.event_time = Some(now);
        if editor.reconcile_external() && host.has_capture() {
            self.cancel_contacts(editor, host, None, &mut out);
            self.clear_keys();
        }
        self.retire_invalid(editor, host, &mut out);
        self.emit_long_presses(editor, now, &mut out);
        self.finish_dispatch(editor, host, &mut out, &mut 256, true);
        out
    }
    fn emit_long_presses(&mut self, editor: &mut SceneEditor, now: u64, out: &mut RoutedReport) {
        let due: Vec<_> = self
            .contacts
            .iter_mut()
            .filter_map(|(_, contact)| {
                if Self::long_press_candidate(contact)
                    && now.saturating_sub(contact.down.timestamp_millis?) >= LONG_PRESS_MS
                {
                    contact.long_pressed = true;
                    Some((contact.target, contact.last.clone()))
                } else {
                    None
                }
            })
            .collect();
        let position = self.position;
        for (target, mut pointer) in due {
            self.position = pointer.position;
            pointer.timestamp_millis = Some(now);
            self.emit(
                editor,
                target,
                EventKind::LongPress,
                metadata(&CanvasEvent::PointerMove(pointer), false),
                out,
            );
        }
        self.position = position;
    }
    pub(super) fn cancel_contacts(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        identity: Option<PointerIdentity>,
        out: &mut RoutedReport,
    ) {
        let ids: Vec<_> = self
            .contacts
            .keys()
            .copied()
            .filter(|id| identity.is_none_or(|wanted| wanted == *id))
            .collect();
        let position = self.position;
        let mut cancelled_targets = Vec::new();
        for id in ids {
            let contact = self.contacts.remove(&id).unwrap();
            cancelled_targets.push((id.device, contact.target, contact.epoch));
            self.position = contact.last.position;
            let mut input = metadata(&CanvasEvent::PointerUp(contact.last), false);
            if let Some(EventInput::Pointer {
                cancelled, buttons, ..
            }) = &mut input
            {
                *cancelled = true;
                *buttons = 0;
            }
            self.emit(
                editor,
                contact
                    .target
                    .filter(|h| Self::current(&editor.engine, *h, contact.epoch)),
                EventKind::PointerCancel,
                input,
                out,
            );
        }
        self.position = position;
        if identity.is_none() || self.default_pointer.is_some_and(|id| Some(id) == identity) {
            self.default_pointer = None;
            let cancelled = host.dispatch(editor, CanvasEvent::PointerCancel);
            merge_report(&mut out.default, cancelled);
        }
        // Cancellation breaks the click sequence. A delayed up can never revive it.
        if identity.is_some() {
            self.recent_taps.retain(|tap| {
                !cancelled_targets.contains(&(tap.device, Some(tap.target), tap.epoch))
            });
        } else {
            self.recent_taps.clear();
        }
    }
    pub(super) fn update_hover(
        &mut self,
        editor: &mut SceneEditor,
        hit: Option<ElementHandle>,
        input: Option<EventInput>,
        out: &mut RoutedReport,
    ) {
        if self.hover == hit {
            return;
        }
        self.emit(
            editor,
            self.hover,
            EventKind::PointerOut,
            input.clone(),
            out,
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
                out,
            );
        }
        self.emit(editor, hit, EventKind::PointerOver, input.clone(), out);
        for handle in new_path.iter().filter(|h| !old_path.contains(h)) {
            self.emit(
                editor,
                Some(*handle),
                EventKind::PointerEnter,
                input.clone(),
                out,
            );
        }
        self.hover = hit;
        self.hover_epoch = Self::epoch(&editor.engine, hit);
    }
    pub(super) fn dispatch_pointer(
        &mut self,
        editor: &mut SceneEditor,
        host: &mut ScenePluginHost,
        event: CanvasEvent,
        out: &mut RoutedReport,
        budget: &mut usize,
    ) {
        let pointer = event.pointer().unwrap().clone();
        let identity = pointer.identity();
        self.position = pointer.position;
        let hit = match &pointer.hit {
            HitTarget::Object(id)
            | HitTarget::ObjectResize(id)
            | HitTarget::ObjectResizeHandle { id, .. }
            | HitTarget::LineEndpoint { id, .. }
            | HitTarget::GeometryControl { id, .. }
            | HitTarget::ImageCrop { id, .. } => editor
                .engine
                .handle(&DisplayId::from(id.clone()))
                .filter(|h| Self::id(editor, *h).is_some()),
            _ => None,
        };
        let input = metadata(&event, false);
        let is_down = matches!(event, CanvasEvent::PointerDown(_));
        let is_up = matches!(event, CanvasEvent::PointerUp(_));
        // Touch is a contact, never a competing hover cursor.
        if matches!(event, CanvasEvent::PointerMove(_)) && pointer.device != PointerDevice::Touch {
            self.update_hover(editor, hit, input.clone(), out);
        }
        let chord = self
            .contacts
            .get(&identity)
            .is_some_and(|p| p.down.button != pointer.button)
            && (is_down || is_up);
        if is_down && !chord && self.contacts.contains_key(&identity) {
            self.cancel_contacts(editor, host, Some(identity), out);
        }
        if is_down && !self.contacts.contains_key(&identity) && self.contacts.len() >= MAX_CONTACTS
        {
            out.errors
                .push("at most 16 simultaneous pointer contacts are supported".into());
            return;
        }
        // Orphan releases cannot target a replacement instance or release a
        // primary edit. An unpressed move still supplies normal hover routing.
        if is_up && !self.contacts.contains_key(&identity) {
            return;
        }
        if let Some(contact) = self.contacts.get_mut(&identity) {
            contact.moved |= contact.down.position.distance(pointer.position) >= SLOP || chord;
            contact.last = pointer.clone();
            if !is_up
                && !is_down
                && contact
                    .down
                    .button
                    .is_some_and(|b| pointer.held_buttons(false) & b.mask() == 0)
            {
                self.cancel_contacts(editor, host, Some(identity), out);
                return;
            }
        }
        if let Some(now) = pointer.timestamp_millis {
            self.emit_long_presses(editor, now, out);
        }
        let target = self.contacts.get(&identity).map_or(hit, |p| p.target);
        let kind = if is_down {
            EventKind::PointerDown
        } else if is_up {
            EventKind::PointerUp
        } else {
            EventKind::PointerMove
        };
        let prevented = self.emit(editor, target, kind, input.clone(), out);
        out.default.default_prevented |= prevented;
        if chord {
            return;
        }
        if is_down {
            if prevented {
                return;
            }
            let primary = self.contacts.is_empty();
            if primary {
                merge_report(&mut out.default, host.dispatch(editor, event));
                self.transfer_focus(editor, host, hit, false, out, budget);
            }
            if out.focus_requested {
                return;
            }
            if primary {
                self.default_pointer = Some(identity);
            }
            self.contacts.insert(
                identity,
                Contact {
                    target: hit,
                    epoch: Self::epoch(&editor.engine, hit),
                    capture_owner: primary
                        .then(|| host.capture_owner().map(str::to_owned))
                        .flatten(),
                    down: pointer.clone(),
                    down_sequence: self.sequence,
                    last: pointer,
                    moved: false,
                    long_pressed: false,
                },
            );
            return;
        }
        let primary =
            self.default_pointer == Some(identity) || (self.contacts.is_empty() && !is_up);
        if primary {
            if is_up && prevented {
                merge_report(
                    &mut out.default,
                    host.dispatch(editor, CanvasEvent::PointerCancel),
                );
            } else if !prevented {
                let mut default_event = event;
                // `button` identifies the changed button on down/up; legacy
                // plugins expect the initiating button on move.
                if let CanvasEvent::PointerMove(p) = &mut default_event
                    && let Some(contact) = self.contacts.get(&identity)
                    && p.buttons.is_some()
                {
                    p.button = contact.down.button;
                }
                merge_report(&mut out.default, host.dispatch(editor, default_event));
            }
        }
        if !is_up {
            return;
        }
        if primary {
            self.default_pointer = None;
        }
        let contact = self.contacts.remove(&identity).unwrap();
        let eligible = !prevented
            && !contact.moved
            && !contact.long_pressed
            && contact.down.button == Some(PointerButton::Left)
            && contact.target == hit
            && hit.is_some()
            && contact
                .down
                .timestamp_millis
                .zip(pointer.timestamp_millis)
                .is_none_or(|(down, up)| up.saturating_sub(down) < LONG_PRESS_MS);
        if !eligible {
            return;
        }
        let target = hit.unwrap();
        // Every tap is immediate. A double is an additional notification after
        // the second tap; no delayed single-tap callbacks or hidden timers.
        self.emit(editor, Some(target), EventKind::Tap, input.clone(), out);
        self.emit(editor, Some(target), EventKind::Click, input.clone(), out);
        if let Some((down_time, up_time)) =
            contact.down.timestamp_millis.zip(pointer.timestamp_millis)
        {
            let prior = self.recent_taps.iter().position(|tap| {
                tap.device == identity.device
                    && tap.target == target
                    && tap.epoch == contact.epoch
                    && contact.down_sequence > tap.up_sequence
                    && down_time >= tap.up_time
                    && up_time.saturating_sub(tap.up_time) <= DOUBLE_TAP_MS
                    && tap.position.distance(pointer.position) < SLOP
            });
            if let Some(index) = prior {
                self.recent_taps.remove(index);
                self.emit(
                    editor,
                    Some(target),
                    EventKind::DoubleTap,
                    input.clone(),
                    out,
                );
                self.emit(editor, Some(target), EventKind::DoubleClick, input, out);
            } else {
                self.recent_taps.retain(|tap| tap.device != identity.device);
                if self.recent_taps.len() == MAX_CONTACTS {
                    self.recent_taps.pop_front();
                }
                self.recent_taps.push_back(Tap {
                    target,
                    epoch: contact.epoch,
                    device: identity.device,
                    position: pointer.position,
                    up_time,
                    up_sequence: self.sequence,
                });
            }
        }
    }
}
pub(super) fn merge_report(target: &mut DispatchReport, report: DispatchReport) {
    target.effects.extend(report.effects);
    target.errors.extend(report.errors);
    target.handled |= report.handled;
    target.document_changed |= report.document_changed;
    target.selection_changed |= report.selection_changed;
    target.viewport_changed |= report.viewport_changed;
    target.default_prevented |= report.default_prevented;
    target.propagation_stopped |= report.propagation_stopped;
}
