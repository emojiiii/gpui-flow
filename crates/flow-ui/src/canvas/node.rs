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
        let mut body = div().child(builtin_node_body(node, context));
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
        .on_header_mouse_down(
            cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&this.focus, cx);
                if event.modifiers.shift {
                    if !this.editor.selection.nodes.insert(id.clone()) {
                        this.editor.selection.nodes.remove(&id);
                    }
                } else if !this.editor.selection.nodes.contains(&id)
                    || !this.editor.selection.groups.is_empty()
                    || !this.editor.selection.edges.is_empty()
                {
                    this.editor.selection.clear();
                    this.editor.selection.nodes.insert(id.clone());
                }
                if this.editor.graph().node(&id).is_some_and(|n| !n.locked) {
                    this.finish_gesture(cx);
                    if let Err(e) = this.editor.begin_transaction("Move nodes") {
                        this.message = e.to_string();
                        return;
                    }
                    let p = this.world(event.position);
                    this.gesture = Some(Gesture::Move {
                        last: if this.snap_to_grid { p.snapped(20.) } else { p },
                    });
                }
                cx.emit(FlowEvent::SelectionChanged);
                cx.notify();
            }),
        )
        .on_resize_mouse_down(cx.listener(
            move |this, event: &MouseDownEvent, window, cx| {
                cx.stop_propagation();
                window.focus(&this.focus, cx);
                this.finish_gesture(cx);
                if let Err(e) = this.editor.begin_transaction("Resize node") {
                    this.message = e.to_string();
                    return;
                }
                this.gesture = Some(Gesture::Resize {
                    node: resize_id.clone(),
                    start: this.world(event.position),
                    size,
                });
                cx.notify();
            },
        ));
        for port in &node.ports {
            let anchor = node.port_position(&port.id).unwrap() - node.position;
            let port_ref = PortRef::new(node.id.clone(), port.id.clone());
            let active = self.connecting.as_ref() == Some(&port_ref);
            card = card.child(
                PortHandle::from_port(
                    SharedString::from(format!("port-{}-{}", node.id, port.id)),
                    port,
                )
                .anchor(anchor * z)
                .active(active)
                .color(color)
                .theme(theme)
                .on_connect(cx.listener(move |this, _, window, cx| {
                    cx.stop_propagation();
                    window.focus(&this.focus, cx);
                    this.port_click(port_ref.clone(), cx);
                })),
            );
        }
        card.into_any_element()
    }
}
