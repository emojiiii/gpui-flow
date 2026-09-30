use super::*;
impl SceneView {
    pub(super) fn down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        self.finish(cx);
        let world = self.world(event.position);
        let local = self.local(event.position);
        if event.button == MouseButton::Middle {
            self.gesture = Some(Gesture::Pan { last: local });
            return;
        }
        if self.editable && self.editor.selection.ids.len() == 1 {
            let id = self.editor.selection.ids.iter().next().unwrap().clone();
            if self
                .selection_corners(&id)
                .is_some_and(|corners| corners[2].distance(local) < 10.)
                && let Some(node) = self.editor.scene.get(&id)
                && let Some(bounds) = node.object.shape.local_bounds()
                && let Some(inverse) = self
                    .editor
                    .scene
                    .world_transform(&id)
                    .ok()
                    .and_then(Affine::inverse)
            {
                let original = node.object.transform;
                self.editor.begin_edit();
                self.gesture = Some(Gesture::Resize {
                    id,
                    inverse,
                    original,
                    extent: bounds.size,
                });
                return;
            }
        }
        if let Some(mut id) = self.hit(world) {
            if event.modifiers.control
                && let Some(parent) = self.editor.scene.get(&id).and_then(|n| n.parent.clone())
            {
                id = parent;
            }
            if self.dispatch(EventKind::PointerDown, id.clone(), world, cx) {
                return;
            }
            self.pressed = Some((id.clone(), world));
            if event.modifiers.shift {
                self.editor.selection.toggle(id.clone());
            } else if !self.editor.selection.ids.contains(&id) {
                self.editor.selection.select(id, false);
            }
            if self.editable {
                self.editor.begin_edit();
                self.gesture = Some(Gesture::Move { last: world });
            }
        } else if event.modifiers.shift {
            self.gesture = Some(Gesture::Marquee {
                start: world,
                current: world,
            });
        } else {
            self.editor.selection.clear();
            self.gesture = Some(Gesture::Pan { last: local });
        }
        cx.emit(SceneViewEvent::SelectionChanged);
        cx.notify();
    }
    pub(super) fn moved(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let world = self.world(event.position);
        let local = self.local(event.position);
        if event.pressed_button.is_none() {
            if self.gesture.is_some() {
                self.finish(cx);
            }
            if let Some(id) = self.hit(world) {
                self.dispatch(EventKind::PointerMove, id, world, cx);
            }
            return;
        }
        match self.gesture.clone() {
            Some(Gesture::Pan { last }) => {
                self.editor.viewport.pan_by(local - last);
                self.gesture = Some(Gesture::Pan { last: local });
            }
            Some(Gesture::Move { last }) => {
                let _ = self.editor.translate_selection(world - last);
                self.gesture = Some(Gesture::Move { last: world });
            }
            Some(Gesture::Resize {
                id,
                inverse,
                original,
                extent,
            }) => {
                let p = inverse.transform_point(world);
                let sx = (p.x / extent.width).clamp(0.05, 100.);
                let sy = (p.y / extent.height).clamp(0.05, 100.);
                let _ = self.editor.transaction(|scene| {
                    scene.update_object(&id, |o| o.transform = original * Affine::scale(sx, sy))
                });
            }
            Some(Gesture::Marquee { start, .. }) => {
                self.gesture = Some(Gesture::Marquee {
                    start,
                    current: world,
                })
            }
            None => {}
        }
        if let Some((id, _)) = self.pressed.clone() {
            self.dispatch(EventKind::PointerMove, id, world, cx);
        }
        cx.notify();
    }
    pub(super) fn up(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let world = self.world(event.position);
        if let Some((id, start)) = self.pressed.take() {
            self.dispatch(EventKind::PointerUp, id.clone(), world, cx);
            if start.distance(world) * self.editor.viewport.zoom < 4. {
                self.dispatch(EventKind::Click, id, world, cx);
            }
        }
        self.finish(cx);
    }
    pub(super) fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let command = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
        match (command, event.keystroke.key.as_str()) {
            (true, "z") => {
                if event.keystroke.modifiers.shift {
                    self.editor.redo();
                } else {
                    self.editor.undo();
                }
            }
            (true, "y") => {
                self.editor.redo();
            }
            (_, "escape") => {
                self.editor.cancel_edit();
                self.gesture = None;
                self.pressed = None;
            }
            (_, "delete" | "backspace") if self.editable => {
                let _ = self.editor.delete_selection();
            }
            (false, "r") if self.editable => {
                let _ = self
                    .editor
                    .rotate_selection(if event.keystroke.modifiers.shift {
                        -15.
                    } else {
                        15.
                    });
            }
            (false, "+" | "=") if self.editable => {
                let _ = self.editor.scale_selection(1.1);
            }
            (false, "-") if self.editable => {
                let _ = self.editor.scale_selection(1. / 1.1);
            }
            (false, "f") => self.fit(),
            _ => {}
        }
        cx.emit(SceneViewEvent::Changed);
        cx.notify();
    }
}
