use super::*;

fn visible_minimap_nodes(
    graph: &model::Graph,
    selected: &std::collections::BTreeSet<model::NodeId>,
) -> Vec<MiniMapNode> {
    let hidden = model::hidden_node_ids(graph);
    model::ordered_node_ids(graph)
        .into_iter()
        .filter(|id| !hidden.contains(id))
        .filter_map(|id| graph.node(&id))
        .map(|node| {
            MiniMapNode::new(node.bounds(), kind_color(&node.kind))
                .selected(selected.contains(&node.id))
        })
        .collect()
}

impl FlowCanvas {
    pub(super) fn minimap(&self, cx: &mut Context<Self>) -> AnyElement {
        let visible = self.editor.viewport.visible_world_rect(self.extent);
        let nodes = visible_minimap_nodes(self.editor.graph(), &self.editor.selection.nodes);
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

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    #[test]
    fn hidden_objects_do_not_appear_or_expand_the_overview() {
        let mut graph = model::Graph::new();
        graph
            .add_node(
                model::Node::new("visible", "Visible", model::Point::new(10., 10.))
                    .with_size(model::Size::new(20., 20.)),
            )
            .unwrap();
        graph
            .add_node(
                model::Node::new("hidden", "Hidden", model::Point::new(10000., 10000.))
                    .with_size(model::Size::new(200., 200.)),
            )
            .unwrap();
        model::set_node_visible(&mut graph, &"hidden".into(), false).unwrap();
        let nodes = visible_minimap_nodes(
            &graph,
            &["visible".into(), "hidden".into()].into_iter().collect(),
        );
        assert_eq!(nodes.len(), 1);
        assert!(nodes[0].selected);
        assert_eq!(
            nodes[0].bounds,
            graph.nodes[&model::NodeId::from("visible")].bounds()
        );
        let viewport = model::Rect::new(model::Point::ZERO, model::Size::new(100., 100.));
        let map = MiniMap::new("visible-only-map", viewport).nodes(nodes);
        let transform = map.transform().unwrap();
        let viewport_corner = transform.world_to_map(model::Point::new(100., 100.));
        assert!(viewport_corner.x > 100. && viewport_corner.y > 100.);
    }
}
