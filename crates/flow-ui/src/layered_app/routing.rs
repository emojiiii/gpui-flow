//! One pointer stream and one keyboard owner across independent scene routers.
use super::{LayerId, LayerOptions};
use gpui_flow_core::Point;
use gpui_flow_interaction::{CanvasEvent, HitTarget, PointerButton};

pub(super) trait LayerAccess {
    fn hit(&mut self, id: LayerId, position: Point) -> HitTarget;
    fn send(&mut self, id: LayerId, event: CanvasEvent);
}
#[derive(Clone, Copy)]
struct Press {
    layer: LayerId,
    pointer: u64,
    button: Option<PointerButton>,
}
#[derive(Default)]
pub(super) struct LayerRouting {
    pub entries: Vec<(LayerId, LayerOptions)>,
    pub focused: Option<LayerId>,
    pub hovered: Option<LayerId>,
    press: Option<Press>,
    suppressed: Option<(u64, Option<PointerButton>)>,
}
impl LayerRouting {
    pub fn pressed_layer(&self) -> Option<LayerId> {
        self.press.map(|p| p.layer)
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
    /// Lifecycle and stacking changes must not forward a stale release to a
    /// newly exposed layer. Suppression lasts through the matching physical Up.
    pub fn invalidate(&mut self, id: LayerId, access: &mut impl LayerAccess) {
        if self.press.is_some_and(|p| p.layer == id) {
            let press = self.press.take().unwrap();
            self.suppressed = Some((press.pointer, press.button));
            access.send(id, CanvasEvent::PointerCancel);
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
        if let Some(press) = self.press.take() {
            self.suppressed = Some((press.pointer, press.button));
            access.send(press.layer, CanvasEvent::PointerCancel);
        }
        self.hover(None, access);
    }
    /// Outside release has no target and must not cancel an unrelated button's
    /// stream or clear keys in a layer with no pointer ownership.
    pub fn release_outside(
        &mut self,
        pointer: u64,
        button: Option<PointerButton>,
        access: &mut impl LayerAccess,
    ) -> Option<LayerId> {
        if self.suppressed == Some((pointer, button)) {
            self.suppressed = None;
        }
        let press = self
            .press
            .filter(|p| p.pointer == pointer && p.button == button)?;
        self.press = None;
        access.send(press.layer, CanvasEvent::PointerCancel);
        self.hover(None, access);
        Some(press.layer)
    }
    /// Window-level moves outside the painted rectangle retain the accepted
    /// stream but cannot hover or pick content clipped beyond the app surface.
    pub fn move_outside(
        &mut self,
        mut pointer: gpui_flow_interaction::PointerInput,
        access: &mut impl LayerAccess,
    ) -> Option<LayerId> {
        if !pointer.position.is_finite() {
            return None;
        }
        let press = self.press.filter(|p| p.pointer == pointer.pointer_id)?;
        self.hover(None, access);
        if pointer.button.is_none() {
            self.press = None;
            self.suppressed = None;
            access.send(press.layer, CanvasEvent::PointerCancel);
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
            if let Some(press) = self.press {
                if press.pointer != pointer.pointer_id {
                    return None;
                }
                if matches!(event, CanvasEvent::PointerUp(_)) && press.button != pointer.button {
                    return None;
                }
            }
        }
        match &mut event {
            CanvasEvent::PointerDown(pointer) => {
                self.suppressed = None;
                if let Some(old) = self.press.take() {
                    access.send(old.layer, CanvasEvent::PointerCancel);
                }
                let target = self.pick(pointer.position, access);
                self.hover(target.as_ref().map(|(id, _)| *id), access);
                self.activate(target.as_ref().map(|(id, _)| *id), access);
                let (id, hit) = target?;
                pointer.hit = hit;
                self.press = Some(Press {
                    layer: id,
                    pointer: pointer.pointer_id,
                    button: pointer.button,
                });
                access.send(id, event);
                Some(id)
            }
            CanvasEvent::PointerMove(_) | CanvasEvent::PointerUp(_) => {
                // A move with no physical button ends an interrupted stream.
                if matches!(event, CanvasEvent::PointerMove(_))
                    && event.pointer().unwrap().button.is_none()
                {
                    if let Some(old) = self.press.take() {
                        access.send(old.layer, CanvasEvent::PointerCancel);
                    }
                    self.suppressed = None;
                }
                if let Some((pointer_id, button)) = self.suppressed {
                    if matches!(&event, CanvasEvent::PointerUp(p) if p.pointer_id == pointer_id && p.button == button)
                    {
                        self.suppressed = None;
                    }
                    return None;
                }
                let is_up = matches!(event, CanvasEvent::PointerUp(_));
                let pointer = match &mut event {
                    CanvasEvent::PointerMove(p) | CanvasEvent::PointerUp(p) => p,
                    _ => unreachable!(),
                };
                let target = if let Some(press) = self.press {
                    self.eligible(press.layer)
                        .then(|| (press.layer, access.hit(press.layer, pointer.position)))
                } else if is_up {
                    // Unpaired releases never create clicks or finish another layer's gesture.
                    None
                } else {
                    self.pick(pointer.position, access)
                };
                self.hover(target.as_ref().map(|(id, _)| *id), access);
                if is_up {
                    self.press = None;
                }
                let (id, hit) = target?;
                pointer.hit = hit;
                access.send(id, event);
                Some(id)
            }
            CanvasEvent::Wheel {
                position, delta, ..
            } => {
                if !position.is_finite() || !delta.is_finite() {
                    return None;
                }
                let id = self
                    .press
                    .map(|p| p.layer)
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
                // Every router clears held keys even if its target was removed
                // between native focus notifications and this operation.
                for (id, _) in &self.entries {
                    access.send(*id, event.clone());
                }
                if let Some(press) = self.press.take() {
                    self.suppressed = Some((press.pointer, press.button));
                }
                self.hovered = None;
                if focus_lost {
                    self.focused = None;
                }
                None
            }
        }
    }
}
