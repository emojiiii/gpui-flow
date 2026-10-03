//! Bounded pointer ownership across independent scene routers, one keyboard owner.
use super::{LayerId, LayerOptions};
use gpui_flow_core::Point;
use gpui_flow_interaction::{
    CanvasEvent, HitTarget, PointerButton, PointerDevice, PointerIdentity,
};
use std::collections::BTreeMap;

pub(super) trait LayerAccess {
    fn hit(&mut self, id: LayerId, position: Point) -> HitTarget;
    fn send(&mut self, id: LayerId, event: CanvasEvent);
}
#[derive(Clone, Copy)]
struct Press {
    layer: LayerId,
    button: Option<PointerButton>,
}
#[derive(Default)]
pub(super) struct LayerRouting {
    pub entries: Vec<(LayerId, LayerOptions)>,
    pub focused: Option<LayerId>,
    pub hovered: Option<LayerId>,
    presses: BTreeMap<PointerIdentity, Press>,
    owner: Option<PointerIdentity>,
    suppressed: BTreeMap<PointerIdentity, Option<PointerButton>>,
}
impl LayerRouting {
    pub fn pressed_layer(&self) -> Option<LayerId> {
        self.owner
            .and_then(|id| self.presses.get(&id))
            .map(|p| p.layer)
    }
    fn eligible(&self, id: LayerId) -> bool {
        self.entries
            .iter()
            .any(|(entry, options)| *entry == id && options.visible && options.interactive)
    }
    fn pick(&self, position: Point, access: &mut impl LayerAccess) -> Option<(LayerId, HitTarget)> {
        self.entries.iter().rev().find_map(|(id, options)| {
            if !options.visible || !options.interactive {
                return None;
            }
            let hit = access.hit(*id, position);
            (hit != HitTarget::Background || options.hit_background).then_some((*id, hit))
        })
    }
    fn hover(&mut self, target: Option<LayerId>, access: &mut impl LayerAccess) {
        if self.hovered != target {
            if let Some(old) = self.hovered.take() {
                access.send(old, CanvasEvent::PointerLeave);
            }
            self.hovered = target;
        }
    }
    pub fn activate(&mut self, target: Option<LayerId>, access: &mut impl LayerAccess) {
        if self.focused != target {
            if let Some(old) = self.focused.take() {
                access.send(old, CanvasEvent::FocusLost);
            }
            self.focused = target;
        }
    }
    fn suppress(&mut self, identity: PointerIdentity, button: Option<PointerButton>) {
        if self.suppressed.len() == 16 && !self.suppressed.contains_key(&identity) {
            self.suppressed.pop_first();
        }
        self.suppressed.insert(identity, button);
    }
    fn remove_press(&mut self, identity: PointerIdentity) -> Option<Press> {
        if self.owner == Some(identity) {
            self.owner = None;
        }
        self.presses.remove(&identity)
    }
    fn cancel(
        &mut self,
        identity: PointerIdentity,
        access: &mut impl LayerAccess,
    ) -> Option<LayerId> {
        let press = self.remove_press(identity)?;
        self.suppress(identity, press.button);
        access.send(press.layer, CanvasEvent::PointerCancelFor(identity));
        Some(press.layer)
    }
    /// Lifecycle changes cannot forward a stale release to a newly exposed layer.
    pub fn invalidate(&mut self, id: LayerId, access: &mut impl LayerAccess) {
        let contacts: Vec<_> = self
            .presses
            .iter()
            .filter(|(_, press)| press.layer == id)
            .map(|(key, _)| *key)
            .collect();
        for key in contacts {
            self.cancel(key, access);
        }
        if self.hovered == Some(id) {
            self.hover(None, access);
        }
        if self.focused == Some(id) {
            self.activate(None, access);
        } else {
            access.send(id, CanvasEvent::FocusLost);
        }
    }
    pub fn reorder(&mut self, access: &mut impl LayerAccess) {
        let contacts: Vec<_> = self.presses.keys().copied().collect();
        for key in contacts {
            self.cancel(key, access);
        }
        self.hover(None, access);
    }
    pub fn release_outside(
        &mut self,
        pointer: u64,
        button: Option<PointerButton>,
        access: &mut impl LayerAccess,
    ) -> Option<LayerId> {
        let key = PointerIdentity {
            pointer_id: pointer,
            device: PointerDevice::Mouse,
        };
        if self.suppressed.get(&key) == Some(&button) {
            self.suppressed.remove(&key);
        }
        self.presses.get(&key).filter(|p| p.button == button)?;
        let layer = self.cancel(key, access);
        self.suppressed.remove(&key);
        self.hover(None, access);
        layer
    }
    pub fn move_outside(
        &mut self,
        mut pointer: gpui_flow_interaction::PointerInput,
        access: &mut impl LayerAccess,
    ) -> Option<LayerId> {
        if !pointer.position.is_finite() {
            return None;
        }
        let key = pointer.identity();
        let press = *self.presses.get(&key)?;
        if pointer.device != PointerDevice::Touch {
            self.hover(None, access);
        }
        if pointer.held_buttons(false) == 0 {
            self.cancel(key, access);
            self.suppressed.remove(&key);
        } else {
            pointer.hit = HitTarget::Background;
            access.send(press.layer, CanvasEvent::PointerMove(pointer));
        }
        Some(press.layer)
    }
    pub fn route(
        &mut self,
        mut event: CanvasEvent,
        access: &mut impl LayerAccess,
    ) -> Option<LayerId> {
        if let Some(pointer) = event.pointer() {
            if !pointer.position.is_finite() {
                return None;
            }
            // The legacy mouse transport has one cursor. Distinct touch IDs
            // are admitted independently without impersonating that cursor.
            if pointer.device == PointerDevice::Mouse
                && self
                    .presses
                    .keys()
                    .any(|key| key.device == PointerDevice::Mouse && *key != pointer.identity())
            {
                return None;
            }
            if matches!(event, CanvasEvent::PointerUp(_))
                && self
                    .presses
                    .get(&pointer.identity())
                    .is_some_and(|p| p.button != pointer.button)
            {
                // Chord releases are observations for the same original layer.
                if pointer.buttons.is_some() {
                    let layer = self.presses[&pointer.identity()].layer;
                    access.send(layer, event);
                    return Some(layer);
                }
                return None;
            }
        }
        match &mut event {
            CanvasEvent::PointerDown(pointer) => {
                let key = pointer.identity();
                self.suppressed.remove(&key);
                if let Some(press) = self.presses.get(&key).copied() {
                    pointer.hit = access.hit(press.layer, pointer.position);
                    access.send(press.layer, event);
                    return Some(press.layer);
                }
                if self.presses.len() >= 16 {
                    return None;
                }
                let target = self.pick(pointer.position, access);
                if pointer.device != PointerDevice::Touch {
                    self.hover(target.as_ref().map(|(id, _)| *id), access);
                }
                if self.presses.is_empty() {
                    self.activate(target.as_ref().map(|(id, _)| *id), access);
                }
                let (id, hit) = target?;
                pointer.hit = hit;
                if self.presses.is_empty() {
                    self.owner = Some(key);
                }
                self.presses.insert(
                    key,
                    Press {
                        layer: id,
                        button: pointer.button,
                    },
                );
                access.send(id, event);
                Some(id)
            }
            CanvasEvent::PointerMove(_) | CanvasEvent::PointerUp(_) => {
                let pointer = event.pointer().unwrap();
                let key = pointer.identity();
                if matches!(event, CanvasEvent::PointerMove(_)) && pointer.held_buttons(false) == 0
                {
                    self.cancel(key, access);
                    self.suppressed.remove(&key);
                }
                if let Some(button) = self.suppressed.get(&key) {
                    if matches!(&event, CanvasEvent::PointerUp(p) if p.button == *button) {
                        self.suppressed.remove(&key);
                    }
                    return None;
                }
                let is_up = matches!(event, CanvasEvent::PointerUp(_));
                let pointer = match &mut event {
                    CanvasEvent::PointerMove(p) | CanvasEvent::PointerUp(p) => p,
                    _ => unreachable!(),
                };
                let target = if let Some(press) = self.presses.get(&key) {
                    self.eligible(press.layer)
                        .then(|| (press.layer, access.hit(press.layer, pointer.position)))
                } else if is_up || pointer.device == PointerDevice::Touch {
                    None
                } else {
                    self.pick(pointer.position, access)
                };
                if pointer.device != PointerDevice::Touch {
                    self.hover(target.as_ref().map(|(id, _)| *id), access);
                }
                if is_up {
                    self.remove_press(key);
                }
                let (id, hit) = target?;
                pointer.hit = hit;
                access.send(id, event);
                Some(id)
            }
            CanvasEvent::PointerCancelFor(key) => {
                let key = *key;
                self.cancel(key, access)
            }
            CanvasEvent::Wheel {
                position, delta, ..
            } => {
                if !position.is_finite() || !delta.is_finite() {
                    return None;
                }
                let id = self
                    .pressed_layer()
                    .or_else(|| self.pick(*position, access).map(|t| t.0))?;
                access.send(id, event);
                Some(id)
            }
            CanvasEvent::KeyDown { .. } | CanvasEvent::KeyUp { .. } => {
                let id = self.focused.filter(|id| self.eligible(*id))?;
                access.send(id, event);
                Some(id)
            }
            CanvasEvent::PointerLeave => {
                self.hover(None, access);
                None
            }
            CanvasEvent::PointerCancel | CanvasEvent::FocusLost => {
                let focus_lost = matches!(event, CanvasEvent::FocusLost);
                for (id, _) in &self.entries {
                    access.send(*id, event.clone());
                }
                for (key, press) in std::mem::take(&mut self.presses) {
                    self.suppress(key, press.button);
                }
                self.owner = None;
                self.hovered = None;
                if focus_lost {
                    self.focused = None;
                }
                None
            }
        }
    }
}
