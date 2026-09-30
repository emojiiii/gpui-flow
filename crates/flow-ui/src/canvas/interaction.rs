use super::*;

impl FlowCanvas {
    pub(super) fn finish_gesture(&mut self, cx: &mut Context<Self>) {
        if self.editor.transaction_active()
            && let Err(e) = self.editor.commit_transaction()
        {
            self.message = e.to_string();
        }
        match self.gesture.take() {
            Some(Gesture::Move { .. } | Gesture::Resize { .. }) => {
                self.changed(cx);
            }
            Some(Gesture::Marquee { start, current }) => {
                let rect = model::Rect::from_points(start, current);
                self.editor.selection.clear();
                self.editor.selection.nodes = self
                    .editor
                    .graph()
                    .nodes
                    .values()
                    .filter(|n| rect.intersects(n.bounds()))
                    .map(|n| n.id.clone())
                    .collect();
                cx.emit(FlowEvent::SelectionChanged);
                cx.notify();
            }
            _ => {}
        }
    }
    pub(super) fn move_pointer(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let local = self.local(event.position);
        let world = self.world(event.position);
        self.pointer = world;
        if event.pressed_button.is_none() && self.gesture.is_some() {
            self.finish_gesture(cx);
            return;
        }
        match self.gesture.clone() {
            Some(Gesture::Pan { last }) => {
                self.editor.viewport.pan_by(local - last);
                self.gesture = Some(Gesture::Pan { last: local });
            }
            Some(Gesture::Move { last }) => {
                let world = if self.snap_to_grid {
                    world.snapped(20.)
                } else {
                    world
                };
                let result = self.editor.move_selection(world - last);
                if let Err(e) = result {
                    self.message = e.to_string();
                }
                self.gesture = Some(Gesture::Move { last: world });
            }
            Some(Gesture::Resize { node, start, size }) => {
                let delta = world - start;
                let result = self.editor.resize_node(
                    &node,
                    model::Size::new(
                        (size.width + delta.x).max(160.),
                        (size.height + delta.y).max(100.),
                    ),
                );
                if let Err(e) = result {
                    self.message = e.to_string();
                }
            }
            Some(Gesture::Marquee { start, .. }) => {
                self.gesture = Some(Gesture::Marquee {
                    start,
                    current: world,
                })
            }
            None => {}
        }
        cx.notify();
    }
    pub(super) fn port_click(&mut self, port: PortRef, cx: &mut Context<Self>) {
        if let Some(first) = self.connecting.take() {
            let (source, target) = if self
                .editor
                .graph()
                .port(&first)
                .is_some_and(|p| p.direction == PortDirection::Output)
            {
                (first, port)
            } else {
                (port, first)
            };
            let id = self.editor.graph().fresh_id("edge");
            let result = self
                .editor
                .transaction("Connect", |g| g.connect(id, source, target));
            if let Err(e) = result {
                self.message = e.to_string();
                cx.emit(FlowEvent::ConnectionRejected(self.message.clone()));
            } else {
                self.message = "Connected".into();
                self.changed(cx);
            }
        } else {
            self.connecting = Some(port);
            self.message = "Click a compatible port · Esc cancels".into();
        }
        cx.notify();
    }
    pub(super) fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        let command = event.keystroke.modifiers.control || event.keystroke.modifiers.platform;
        match (command, key) {
            (true, "z") => {
                if event.keystroke.modifiers.shift {
                    self.editor.redo();
                } else {
                    self.editor.undo();
                }
                self.changed(cx);
            }
            (true, "y") => {
                self.editor.redo();
                self.changed(cx);
            }
            (true, "a") => {
                self.editor.selection.nodes = self.editor.graph().nodes.keys().cloned().collect();
                cx.notify();
            }
            (true, "c") => self.clipboard = Some(self.editor.copy_selection()),
            (true, "v") => {
                if let Some(clip) = self.clipboard.clone() {
                    match self.editor.paste(&clip, model::Point::new(32., 32.)) {
                        Ok(selection) => self.editor.selection = selection,
                        Err(e) => self.message = e.to_string(),
                    }
                    self.changed(cx);
                }
            }
            (true, "s") => {
                match self.export_json() {
                    Ok(json) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(json));
                        self.message = "Document JSON copied to system clipboard".into();
                    }
                    Err(e) => self.message = e.to_string(),
                }
                cx.notify();
            }
            (_, "backspace" | "delete") => {
                let result = self.editor.delete_selection();
                self.report(result, cx);
            }
            (_, "escape") => {
                self.connecting = None;
                if self.editor.transaction_active() {
                    let _ = self.editor.cancel_transaction();
                }
                self.gesture = None;
                cx.notify();
            }
            (false, "f") => {
                self.fit();
                cx.notify();
            }
            (false, "g") => {
                self.show_grid = !self.show_grid;
                cx.notify();
            }
            _ => {}
        }
    }
    pub(super) fn local(&self, p: gpui::Point<Pixels>) -> model::Point {
        model::Point::new(p.x.into(), p.y.into()) - self.origin
    }
    pub(super) fn world(&self, p: gpui::Point<Pixels>) -> model::Point {
        self.editor.viewport.screen_to_world(self.local(p))
    }
}
