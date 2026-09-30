use super::*;

impl FlowCanvas {
    pub(super) fn node_body(
        &self,
        node: &Node,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let context = NodeRenderContext {
            selected: self.editor.selection.nodes.contains(&node.id),
            zoom: self.editor.viewport.zoom,
            theme: self.theme,
        };
        if let Some(renderer) = self.registry.get(&node.kind) {
            return renderer.render(node, context, window, cx);
        }
        let rendered = match self.media_players.get(&node.id).map(|(_, player)| player) {
            Some(Ok(player)) if node.kind == "audio" => AudioNode::from_node(node)
                .theme(context.theme)
                .zoom(context.zoom)
                .player(player.clone())
                .into_any_element(),
            Some(Ok(player)) if node.kind == "video" => VideoNode::from_node(node)
                .theme(context.theme)
                .zoom(context.zoom)
                .player(player.clone())
                .into_any_element(),
            Some(Err(error)) => div()
                .child(builtin_node_body(node, context))
                .child(format!("Media: {error}"))
                .into_any_element(),
            _ => builtin_node_body(node, context),
        };
        let mut body = div().child(rendered);
        if let Some(run) = self.runtime.records.get(node.id.as_str()) {
            body = body.child(
                div()
                    .px(px(14. * context.zoom))
                    .text_size(px(11. * context.zoom))
                    .text_color(rgb(context.theme.muted))
                    .child(progress_bar(run.progress, context.theme))
                    .child(format!(
                        "{:?} · {}% · attempt {}",
                        run.status,
                        (run.progress * 100.) as u32,
                        run.attempt
                    )),
            );
        }
        if let Some(record) = self
            .workflow
            .as_ref()
            .and_then(|w| w.records().get(&node.id))
        {
            body = body.child(
                div()
                    .px(px(10. * context.zoom))
                    .text_size(px(10. * context.zoom))
                    .text_color(rgb(context.theme.muted))
                    .child(format!(
                        "Flow {:?} · attempt {}",
                        record.status, record.attempt
                    ))
                    .child(record.message.clone()),
            );
        }
        body.into_any_element()
    }
    pub(super) fn render_node(
        &self,
        node: &Node,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let z = self.editor.viewport.zoom;
        let pos = self.editor.viewport.world_to_screen(node.position);
        let selected = self.editor.selection.nodes.contains(&node.id);
        let theme = self.theme;
        let id = node.id.clone();
        let resize_id = id.clone();
        let body_id = id.clone();
        let size = node.size;
        let color = kind_color(&node.kind);
        let mut card = NodeFrame::new(
            SharedString::from(format!("node-{}", node.id)),
            node.title.clone(),
        )
        .position(pos)
        .size(size)
        .zoom(z)
        .selected(selected)
        .theme(theme)
        .color(color)
        .body(self.node_body(node, window, cx))
        .header_cursor(if self.interactions.has_capture() {
            self.canvas_cursor()
        } else if self.interactions.policy().editable
            && self.interactions.is_enabled("drag")
            && !node.locked
            && (self.interactions.is_enabled("selection")
                || self
                    .editor
                    .selection
                    .expanded_nodes(self.editor.graph())
                    .contains(&node.id))
        {
            CursorStyle::OpenHand
        } else {
            CursorStyle::Arrow
        })
        .dragging(self.interactions.capture_owner() == Some("drag") && selected)
        .on_header_mouse_down(
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.pointer_down(
                    gpui_flow_interaction::HitTarget::Node(id.clone()),
                    event,
                    window,
                    cx,
                );
            }),
        )
        .on_mouse_down(
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                this.pointer_down(
                    gpui_flow_interaction::HitTarget::Node(body_id.clone()),
                    event,
                    window,
                    cx,
                );
            }),
        );
        if !node.locked
            && self.interactions.policy().editable
            && self.interactions.is_enabled("resize")
        {
            card = card.on_resize_mouse_down(cx.listener(
                move |this, event: &MouseDownEvent, window, cx| {
                    this.pointer_down(
                        gpui_flow_interaction::HitTarget::ResizeHandle(resize_id.clone()),
                        event,
                        window,
                        cx,
                    );
                },
            ));
        }
        for port in &node.ports {
            let anchor = node.port_position(&port.id).unwrap() - node.position;
            let port_ref = PortRef::new(node.id.clone(), port.id.clone());
            let connection = self.interactions.connection();
            let state = if !self.interactions.policy().editable
                || !self.interactions.is_enabled("connection")
            {
                PortState::Disabled
            } else if connection.source() == Some(&port_ref) {
                PortState::Source
            } else if connection.target() == Some(&port_ref) {
                PortState::Target
            } else if connection.active() && connection.hovered() == Some(&port_ref) {
                PortState::Rejected
            } else if connection.active()
                && connection
                    .validate_target(self.editor.graph(), &port_ref)
                    .is_ok()
            {
                PortState::Compatible
            } else if connection.active()
                || model::ConnectionDrag::can_start(self.editor.graph(), &port_ref).is_err()
            {
                PortState::Disabled
            } else {
                PortState::Idle
            };
            card = card.child(
                PortHandle::from_port(
                    SharedString::from(format!("port-{}-{}", node.id, port.id)),
                    port,
                )
                .anchor(anchor * z)
                .state(state)
                .color(color)
                .theme(theme)
                .on_connect(cx.listener(
                    move |this, event: &MouseDownEvent, window, cx| {
                        this.pointer_down(
                            gpui_flow_interaction::HitTarget::Port(port_ref.clone()),
                            event,
                            window,
                            cx,
                        );
                    },
                )),
            );
        }
        card.into_any_element()
    }
}
