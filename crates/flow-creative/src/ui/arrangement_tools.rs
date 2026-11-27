use super::*;
use crate::arrangement::{Arrangement, arrange_nodes};

fn arrangement_command(
    graph: &gpui_flow_core::Graph,
    selected: &std::collections::BTreeSet<NodeId>,
    action: Arrangement,
) -> Result<Option<CanvasCommand>, gpui_flow_core::GraphError> {
    let positions = arrange_nodes(graph, selected, action)?;
    if positions.is_empty() {
        return Ok(None);
    }
    let mut candidate = graph.clone();
    for (id, position) in positions {
        candidate
            .nodes
            .get_mut(&id)
            .expect("arrangement validated node")
            .position = position;
    }
    // SetPositions is a drag command and passes through proximity snapping.
    // ImportGraph commits exact layout geometry through the same document
    // validator, Creative lock guard, capabilities and one-step history.
    Ok(Some(CanvasCommand::ImportGraph(candidate)))
}

impl CreativeCanvas {
    pub(super) fn handle_arrangement(
        &mut self,
        action: Option<Arrangement>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        let Some(action) = action else {
            self.show_arrangement = !self.show_arrangement;
            cx.notify();
            return;
        };
        let view = self.canvas.read(cx);
        if self.importing || self.saving || !view.is_editable() {
            return;
        }
        if !view.editor.selection.groups.is_empty() {
            self.notice = Some(
                tr(
                    "请单独选择对象；排列不会改变整个 Frame",
                    "Select individual objects to arrange them",
                )
                .into(),
            );
            cx.notify();
            return;
        }
        match arrangement_command(view.editor.graph(), &view.editor.selection.nodes, action) {
            Ok(Some(command)) => {
                self.command(command, cx);
            }
            Ok(None) => {
                self.notice = Some(
                    tr(
                        "所选对象已在目标位置",
                        "Selected objects are already in place",
                    )
                    .into(),
                )
            }
            Err(error) => self.notice = Some(error.to_string()),
        }
        cx.notify();
    }

    pub(super) fn arrangement_panel(&self, size: WorldSize, cx: &mut Context<Self>) -> Div {
        use Arrangement::*;
        let view = self.canvas.read(cx);
        let graph = view.editor.graph();
        let selected = &view.editor.selection.nodes;
        let editable = view.is_editable()
            && !self.importing
            && !self.saving
            && view.editor.selection.groups.is_empty()
            && selected.len() >= 2
            && selected.iter().all(|id| {
                graph.node(id).is_some()
                    && gpui_flow_core::node_is_visible(graph, id)
                    && !crate::layer_guard::node_is_locked(graph, id)
            });
        let distribution_allowed = [DistributeHorizontal, DistributeVertical]
            .map(|action| editable && arrange_nodes(graph, selected, action).is_ok());
        let hint = if selected
            .iter()
            .any(|id| crate::layer_guard::node_is_locked(graph, id))
        {
            tr(
                "含锁定对象，请先解锁后排列",
                "Unlock the selected objects before arranging them",
            )
        } else if !view.editor.selection.groups.is_empty() {
            tr(
                "请选择对象；不排列整个 Frame",
                "Select objects within the frame",
            )
        } else {
            tr(
                "按所选外沿对齐；等间距需 3 个对象及足够空间",
                "Align to selection bounds. Spacing requires three objects and enough room.",
            )
        };
        let mut content = div()
            .min_w(px(276.))
            .p_2()
            .flex()
            .flex_col()
            .gap_1()
            .child(label(tr("排列所选对象", "Arrange selected objects")));
        for (row_index, actions) in [
            [
                (tr("左对齐", "Align left"), AlignLeft),
                (tr("水平居中", "Center horizontally"), AlignCenter),
                (tr("右对齐", "Align right"), AlignRight),
            ],
            [
                (tr("顶对齐", "Align top"), AlignTop),
                (tr("垂直居中", "Center vertically"), AlignMiddle),
                (tr("底对齐", "Align bottom"), AlignBottom),
            ],
        ]
        .into_iter()
        .enumerate()
        {
            let mut row = div().flex().gap_1();
            for (index, (title, action)) in actions.into_iter().enumerate() {
                row = row.child(
                    self.button(
                        SharedString::from(format!("arrange-{row_index}-{index}")),
                        title,
                        Action::Arrange(Some(action)),
                        !editable,
                        cx,
                    )
                    .flex_1(),
                );
            }
            content = content.child(row);
        }
        let mut distribution = div().flex().gap_1();
        for (index, (title, action)) in [
            (
                tr("水平等间距", "Distribute horizontally"),
                DistributeHorizontal,
            ),
            (
                tr("垂直等间距", "Distribute vertically"),
                DistributeVertical,
            ),
        ]
        .into_iter()
        .enumerate()
        {
            let allowed = distribution_allowed[index];
            distribution = distribution.child(
                self.button(
                    SharedString::from(format!("distribute-{index}")),
                    title,
                    Action::Arrange(Some(action)),
                    !allowed,
                    cx,
                )
                .flex_1(),
            );
        }
        content = content.child(distribution).child(
            div()
                .text_size(px(10.))
                .text_color(rgb(0x90939d))
                .child(hint),
        );
        div()
            .occlude()
            .w(px(size.width))
            .rounded(px(10.))
            .bg(rgb(0xffffff))
            .border_1()
            .border_color(rgb(0xe9eaed))
            .shadow_sm()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("arrangement-scroll")
                    .max_h(px(size.height))
                    .overflow_x_scroll()
                    .overflow_y_scroll()
                    .child(content),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_flow_core::{Editor, Graph, Point, Size};
    use gpui_flow_interaction::{CanvasEffect, PluginHost};

    #[test]
    fn exact_arrangement_ignores_proximity_snap_and_keeps_history() {
        let mut graph = Graph::new();
        for (id, x, y) in [("a", 0., 0.), ("b", 100., 3.)] {
            graph
                .add_node(Node::new(id, id, Point::new(x, y)).with_size(Size::new(20., 20.)))
                .unwrap();
        }
        let selected = [NodeId::from("a"), NodeId::from("b")].into_iter().collect();
        let command = arrangement_command(&graph, &selected, Arrangement::AlignLeft)
            .unwrap()
            .unwrap();
        let mut editor = Editor::new(graph.clone()).unwrap();
        let mut host = PluginHost::with_defaults();
        let effects = host.execute(&mut editor, command).unwrap();
        assert_eq!(
            editor.graph().nodes[&NodeId::from("b")].position,
            Point::new(0., 3.)
        );
        assert!(
            !effects
                .effects
                .iter()
                .any(|effect| matches!(effect, CanvasEffect::Guides(guides) if !guides.is_empty()))
        );
        assert!(editor.undo());
        assert_eq!(editor.graph(), &graph);
        assert!(!editor.undo());
        assert!(editor.redo());
        assert_eq!(
            editor.graph().nodes[&NodeId::from("b")].position,
            Point::new(0., 3.)
        );
        assert!(
            arrangement_command(editor.graph(), &selected, Arrangement::AlignLeft)
                .unwrap()
                .is_none()
        );
        graph.nodes.get_mut(&NodeId::from("b")).unwrap().locked = true;
        assert!(arrangement_command(&graph, &selected, Arrangement::AlignLeft).is_err());
    }
}
