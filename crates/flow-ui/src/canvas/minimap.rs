use super::*;

impl FlowCanvas {
    pub(super) fn minimap(&self, cx: &mut Context<Self>) -> AnyElement {
        let visible = self.editor.viewport.visible_world_rect(self.extent);
        let nodes = self.editor.graph().nodes.values().map(|node| {
            MiniMapNode::new(node.bounds(), kind_color(&node.kind))
                .selected(self.editor.selection.nodes.contains(&node.id))
        });
        div()
            .absolute()
            .right(px(18.))
            .bottom(px(50.))
            .child(
                MiniMap::new("minimap", visible)
                    .nodes(nodes)
                    .theme(self.theme)
                    .on_navigate(cx.listener(|this, world: &model::Point, window, cx| {
                        window.focus(&this.focus, cx);
                        this.cancel_interaction(cx);
                        let target =
                            model::Point::new(this.extent.width * 0.5, this.extent.height * 0.5)
                                - *world * this.editor.viewport.zoom;
                        let _ = this.execute_command(
                            gpui_flow_interaction::CanvasCommand::Pan {
                                delta: target - this.editor.viewport.pan,
                            },
                            cx,
                        );
                        cx.notify();
                    })),
            )
            .into_any_element()
    }
}
