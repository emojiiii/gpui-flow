//! GPUI-to-plugin adapter. Gesture state and edit commands belong to PluginHost.
use super::*;
use gpui_flow_interaction as input;

fn modifiers(value: gpui::Modifiers) -> input::Modifiers {
    input::Modifiers {
        shift: value.shift,
        control: value.control,
        alt: value.alt,
        meta: value.platform,
        space: false,
    }
}

impl FlowCanvas {
    pub(super) fn apply_interaction_report(
        &mut self,
        report: input::DispatchReport,
        cx: &mut Context<Self>,
    ) {
        if report.default_prevented || report.propagation_stopped {
            cx.stop_propagation();
        }
        if report.document_changed {
            self.changed(cx);
        }
        if report.viewport_changed {
            cx.emit(FlowEvent::ViewportChanged);
        }
        if report.selection_changed {
            cx.emit(FlowEvent::SelectionChanged);
        }
        if let Some(error) = report.errors.last() {
            self.message = error.to_string();
        }
        for effect in report.effects {
            match effect {
                input::CanvasEffect::Clipboard(clipboard) => self.clipboard = Some(clipboard),
                input::CanvasEffect::ConnectionRejected(message) => {
                    self.message = message.clone();
                    cx.emit(FlowEvent::ConnectionRejected(message));
                }
                input::CanvasEffect::RequestPaste => {
                    if let Some(clipboard) = self.clipboard.clone() {
                        let next = self.interactions.execute(
                            &mut self.editor,
                            input::CanvasCommand::Paste {
                                clipboard,
                                offset: model::Point::new(32., 32.),
                            },
                        );
                        match next {
                            Ok(report) => self.apply_interaction_report(report, cx),
                            Err(error) => self.message = error.to_string(),
                        }
                    }
                }
                input::CanvasEffect::RequestCopyDocument => match self.export_json() {
                    Ok(json) => {
                        cx.write_to_clipboard(ClipboardItem::new_string(json));
                        self.message = "Document JSON copied to system clipboard".into();
                    }
                    Err(error) => self.message = error.to_string(),
                },
                input::CanvasEffect::RequestFit => {
                    let next = self.interactions.execute(
                        &mut self.editor,
                        input::CanvasCommand::Fit {
                            canvas: self.extent,
                            padding: 55.,
                        },
                    );
                    match next {
                        Ok(report) => self.apply_interaction_report(report, cx),
                        Err(error) => self.message = error.to_string(),
                    }
                }
                input::CanvasEffect::ToggleGrid => self.show_grid = !self.show_grid,
                input::CanvasEffect::RequestSave => self.save_file(cx),
                _ => {}
            }
        }
        cx.notify();
    }
    fn dispatch_input(&mut self, event: input::CanvasEvent, cx: &mut Context<Self>) {
        if matches!(
            event,
            input::CanvasEvent::PointerCancel | input::CanvasEvent::FocusLost
        ) {
            self.native_pointer.reset();
        }
        let was_connecting = self.interactions.connection().active();
        let report = self.interactions.dispatch(&mut self.editor, event);
        if self.interactions.connection().active() {
            self.message = if self.interactions.connection().target().is_some() {
                "Release to connect"
            } else if self.interactions.connection().hovered().is_some() {
                "This port cannot accept the connection"
            } else {
                "Drag to a compatible port · release empty to cancel"
            }
            .into();
        } else if was_connecting && report.errors.is_empty() {
            self.message = if report.document_changed {
                "Connected"
            } else {
                "Connection cancelled"
            }
            .into();
        }
        self.apply_interaction_report(report, cx);
    }
    pub(super) fn pointer_down(
        &mut self,
        hit: input::HitTarget,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        window.focus(&self.focus, cx);
        let pointer = self.native_pointer.down(
            event,
            self.local(event.position),
            hit,
            self.space_held || self.pan_mode,
        );
        self.dispatch_input(input::CanvasEvent::PointerDown(pointer), cx);
    }
    pub(super) fn move_pointer(&mut self, event: &MouseMoveEvent, cx: &mut Context<Self>) {
        let position = self.local(event.position);
        let pointer = self.native_pointer.moved(
            event,
            position,
            self.hit_at(position),
            self.space_held || self.pan_mode,
        );
        self.dispatch_input(input::CanvasEvent::PointerMove(pointer), cx);
    }
    pub(super) fn pressure_pointer(&mut self, event: &MousePressureEvent, cx: &mut Context<Self>) {
        let position = self.local(event.position);
        let hit = self.hit_at(position);
        let pointer =
            self.native_pointer
                .pressure(event, position, hit, self.space_held || self.pan_mode);
        self.dispatch_input(input::CanvasEvent::PointerMove(pointer), cx);
    }
    pub(super) fn release_pointer(&mut self, event: &MouseUpEvent, cx: &mut Context<Self>) {
        let position = self.local(event.position);
        let pointer = self.native_pointer.up(
            event,
            position,
            self.hit_at(position),
            self.space_held || self.pan_mode,
        );
        self.dispatch_input(input::CanvasEvent::PointerUp(pointer), cx);
    }
    pub(super) fn cancel_interaction(&mut self, cx: &mut Context<Self>) {
        self.dispatch_input(input::CanvasEvent::PointerCancel, cx);
    }
    pub(super) fn focus_lost(&mut self, cx: &mut Context<Self>) {
        self.space_held = false;
        self.dispatch_input(input::CanvasEvent::FocusLost, cx);
    }
    pub(super) fn scroll(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let delta = match event.delta {
            ScrollDelta::Pixels(p) => model::Point::new(p.x.into(), p.y.into()),
            ScrollDelta::Lines(p) => model::Point::new(p.x * 24., p.y * 24.),
        };
        self.dispatch_input(
            input::CanvasEvent::Wheel {
                position: self.local(event.position),
                delta,
                modifiers: input::Modifiers {
                    space: self.space_held || self.pan_mode,
                    ..modifiers(event.modifiers)
                },
            },
            cx,
        );
    }
    pub(super) fn key(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        if matches!(event.keystroke.key.as_str(), "space" | " ") {
            self.space_held = true;
            cx.notify();
            return;
        }
        let key = match event.keystroke.key.as_str() {
            "escape" => input::KeyCode::Escape,
            "delete" => input::KeyCode::Delete,
            "backspace" => input::KeyCode::Backspace,
            key => {
                let mut chars = key.chars();
                match (chars.next(), chars.next()) {
                    (Some(ch), None) => input::KeyCode::Character(ch),
                    _ => input::KeyCode::Other(key.to_string()),
                }
            }
        };
        self.dispatch_input(
            input::CanvasEvent::KeyDown {
                key,
                modifiers: input::Modifiers {
                    space: self.space_held || self.pan_mode,
                    ..modifiers(event.keystroke.modifiers)
                },
            },
            cx,
        );
    }
    pub(super) fn key_up(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        if matches!(event.keystroke.key.as_str(), "space" | " ") {
            self.space_held = false;
            if self.pan_mode {
                cx.notify();
                return;
            }
            self.dispatch_input(
                input::CanvasEvent::KeyUp {
                    key: input::KeyCode::Space,
                    modifiers: modifiers(event.keystroke.modifiers),
                },
                cx,
            );
        }
    }
    pub(super) fn canvas_cursor(&self) -> CursorStyle {
        match self.interactions.cursor() {
            input::CanvasCursor::Default => {
                if (self.space_held || self.pan_mode)
                    && self.interactions.is_enabled("pan")
                    && self.interactions.policy().navigation
                {
                    CursorStyle::OpenHand
                } else {
                    CursorStyle::Arrow
                }
            }
            input::CanvasCursor::Crosshair => CursorStyle::Crosshair,
            input::CanvasCursor::Grab => CursorStyle::OpenHand,
            input::CanvasCursor::Grabbing => CursorStyle::ClosedHand,
            input::CanvasCursor::Resize => CursorStyle::ResizeUpLeftDownRight,
            input::CanvasCursor::ResizeHorizontal => CursorStyle::ResizeLeftRight,
            input::CanvasCursor::ResizeVertical => CursorStyle::ResizeUpDown,
            input::CanvasCursor::ResizeOtherDiagonal => CursorStyle::ResizeUpRightDownLeft,
            input::CanvasCursor::NotAllowed => CursorStyle::OperationNotAllowed,
        }
    }
    /// Exact visible hit areas only. Proximity snapping is owned by SnappingPlugin.
    pub(super) fn hit_at(&self, screen: model::Point) -> input::HitTarget {
        hit_at_editor_ordered(
            &self.editor,
            screen,
            self.interactions.policy().editable && self.interactions.is_enabled("resize"),
            self.selection_on_top,
        )
    }
    pub(super) fn local(&self, p: gpui::Point<Pixels>) -> model::Point {
        model::Point::new(p.x.into(), p.y.into()) - self.origin
    }
}

#[cfg(test)]
fn hit_at_editor(editor: &Editor, screen: model::Point, resize_enabled: bool) -> input::HitTarget {
    hit_at_editor_ordered(editor, screen, resize_enabled, false)
}
fn hit_at_editor_ordered(
    editor: &Editor,
    screen: model::Point,
    resize_enabled: bool,
    selection_on_top: bool,
) -> input::HitTarget {
    let viewport = editor.viewport;
    let hidden = model::hidden_node_ids(editor.graph());
    let mut nodes: Vec<_> = model::ordered_node_ids(editor.graph())
        .into_iter()
        .filter(|id| !hidden.contains(id))
        .filter_map(|id| editor.graph().node(&id))
        .collect();
    if selection_on_top {
        nodes.sort_by_key(|node| editor.selection.nodes.contains(&node.id));
    }
    for node in nodes.into_iter().rev() {
        let rect = viewport.world_rect_to_screen(node.bounds());
        if editor.selection.nodes.contains(&node.id)
            && !node.locked
            && resize_enabled
            && rect.size.width >= 36.
            && rect.size.height >= 36.
        {
            let resize = model::Rect::new(
                rect.origin + model::Point::new(rect.size.width - 22., rect.size.height - 22.),
                model::Size::new(20., 20.),
            );
            if resize.contains(screen) {
                return input::HitTarget::ResizeHandle(node.id.clone());
            }
        }
        for port in &node.ports {
            let reference = PortRef::new(node.id.clone(), port.id.clone());
            if let Some(position) = editor.graph().port_position(&reference) {
                let center = viewport.world_to_screen(position);
                // PortHandle is a 22px square hit box containing a round socket.
                if (center.x - screen.x).abs() <= 11. && (center.y - screen.y).abs() <= 11. {
                    return input::HitTarget::Port(reference);
                }
            }
        }
        if rect.contains(screen) {
            return input::HitTarget::Node(node.id.clone());
        }
    }
    for group in editor.graph().groups.values().rev() {
        let rect = viewport.world_rect_to_screen(group.bounds);
        if model::Rect::new(rect.origin, model::Size::new(rect.size.width, 28.)).contains(screen) {
            return input::HitTarget::Group(group.id.clone());
        }
    }
    input::HitTarget::Background
}

#[cfg(test)]
mod tests {
    use super::hit_at_editor;
    use gpui_flow_core as model;
    use gpui_flow_core::{Editor, Graph, Node, PortRef};
    use gpui_flow_interaction as input;
    fn editor() -> Editor {
        let mut graph = Graph::new();
        graph
            .add_node(
                Node::new("card", "Card", model::Point::new(100., 100.))
                    .with_port(model::Port::input("in", "Input", model::DataType::Number)),
            )
            .unwrap();
        Editor::new(graph).unwrap()
    }
    #[test]
    fn selected_media_front_order_matches_hit_testing_without_changing_default() {
        let mut graph = Graph::new();
        for id in ["a", "z"] {
            graph
                .add_node(Node::new(id, id, model::Point::new(100., 100.)))
                .unwrap();
        }
        let mut editor = Editor::new(graph).unwrap();
        editor.selection.select_node("a", false);
        let at = model::Point::new(150., 160.);
        assert_eq!(
            super::hit_at_editor_ordered(&editor, at, false, false),
            input::HitTarget::Node("z".into())
        );
        assert_eq!(
            super::hit_at_editor_ordered(&editor, at, false, true),
            input::HitTarget::Node("a".into())
        );
    }
    #[test]
    fn explicit_stack_order_drives_overlap_hits_even_after_selection_and_history() {
        let mut graph = Graph::new();
        for id in ["a", "b", "c"] {
            graph
                .add_node(Node::new(id, id, model::Point::ZERO))
                .unwrap();
        }
        let mut editor = Editor::new(graph).unwrap();
        editor.selection.select_node("c", false);
        let targets = editor.selection.nodes.clone();
        editor
            .transaction("Send back", |graph| {
                model::reorder_nodes(graph, &targets, model::NodeOrderAction::SendToBack)?;
                Ok(())
            })
            .unwrap();
        assert_eq!(
            hit_at_editor(&editor, model::Point::new(30., 40.), false),
            input::HitTarget::Node("b".into())
        );
        assert!(editor.undo());
        assert_eq!(
            hit_at_editor(&editor, model::Point::new(30., 40.), false),
            input::HitTarget::Node("c".into())
        );
        assert!(editor.redo());
        assert_eq!(
            hit_at_editor(&editor, model::Point::new(30., 40.), false),
            input::HitTarget::Node("b".into())
        );
    }

    #[test]
    fn hidden_node_body_ports_and_resize_handle_cannot_be_hit() {
        let mut editor = editor();
        editor.selection.select_node("card", false);
        let rect = editor.graph().node(&"card".into()).unwrap().bounds();
        let port = editor
            .graph()
            .port_position(&PortRef::new("card", "in"))
            .unwrap();
        editor
            .transaction("Hide", |graph| {
                model::set_node_visible(graph, &"card".into(), false)?;
                Ok(())
            })
            .unwrap();
        for point in [
            rect.center(),
            port,
            rect.origin + model::Point::new(rect.size.width - 10., rect.size.height - 10.),
        ] {
            assert_eq!(
                hit_at_editor(&editor, point, true),
                input::HitTarget::Background
            );
        }
        editor.undo();
        assert_eq!(
            hit_at_editor(&editor, port, true),
            input::HitTarget::Port(PortRef::new("card", "in"))
        );
    }

    #[test]
    fn body_hit_never_becomes_background_pan_and_exact_ports_do_not_magnet_snap() {
        let editor = editor();
        let rect = editor.graph().node(&"card".into()).unwrap().bounds();
        assert_eq!(
            hit_at_editor(&editor, rect.center(), true),
            input::HitTarget::Node("card".into())
        );
        let port = PortRef::new("card", "in");
        let center = editor.graph().port_position(&port).unwrap();
        assert_eq!(
            hit_at_editor(&editor, center + model::Point::new(10., 0.), true),
            input::HitTarget::Port(port.clone())
        );
        assert_ne!(
            hit_at_editor(&editor, center + model::Point::new(12., 0.), true),
            input::HitTarget::Port(port)
        );
    }
    #[test]
    fn resize_hit_tracks_selected_editable_visible_handle_and_uses_screen_pixels() {
        let mut editor = editor();
        for zoom in [0.25, 1., 3.] {
            editor.viewport.zoom = zoom;
            let rect = editor
                .viewport
                .world_rect_to_screen(editor.graph().node(&"card".into()).unwrap().bounds());
            let point =
                rect.origin + model::Point::new(rect.size.width - 10., rect.size.height - 10.);
            editor.selection.clear();
            assert_eq!(
                hit_at_editor(&editor, point, true),
                input::HitTarget::Node("card".into())
            );
            editor.selection.nodes.insert("card".into());
            assert_eq!(
                hit_at_editor(&editor, point, true),
                input::HitTarget::ResizeHandle("card".into())
            );
            assert_eq!(
                hit_at_editor(&editor, point, false),
                input::HitTarget::Node("card".into())
            );
        }
        editor.viewport.zoom = 0.1;
        let rect = editor
            .viewport
            .world_rect_to_screen(editor.graph().node(&"card".into()).unwrap().bounds());
        assert_ne!(
            hit_at_editor(&editor, rect.center(), true),
            input::HitTarget::ResizeHandle("card".into())
        );
    }
}
