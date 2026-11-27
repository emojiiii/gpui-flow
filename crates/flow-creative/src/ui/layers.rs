use super::*;
use gpui_flow_core::{NodeOrderAction, ordered_node_ids, reorder_nodes};
use gpui_kit::component::input::Input;

struct LayerMatches {
    nodes: Vec<NodeId>,
    groups: Vec<gpui_flow_core::GroupId>,
}

fn filtered_layers(graph: &gpui_flow_core::Graph, query: &str) -> LayerMatches {
    let query = query.trim().to_lowercase();
    let named_groups: std::collections::BTreeSet<_> = graph
        .groups
        .values()
        .filter(|group| query.is_empty() || group.title.to_lowercase().contains(&query))
        .map(|group| group.id.clone())
        .collect();
    let nodes: Vec<_> = ordered_node_ids(graph)
        .into_iter()
        .filter(|id| {
            let named = graph
                .node(id)
                .is_some_and(|node| query.is_empty() || node.title.to_lowercase().contains(&query));
            named
                || named_groups
                    .iter()
                    .any(|group| graph.groups[group].node_ids.contains(id))
        })
        .collect();
    let ids: std::collections::BTreeSet<_> = nodes.iter().cloned().collect();
    let groups = graph
        .groups
        .values()
        .rev()
        .filter(|group| named_groups.contains(&group.id) || !group.node_ids.is_disjoint(&ids))
        .map(|group| group.id.clone())
        .collect();
    LayerMatches { nodes, groups }
}

#[derive(Clone)]
pub(super) enum LayerAction {
    ToggleLock(NodeId),
    ToggleVisibility(NodeId),
    Reorder(NodeOrderAction),
}

impl CreativeCanvas {
    pub(super) fn layer_action(&mut self, action: LayerAction, cx: &mut Context<Self>) {
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            return;
        }
        self.apply_form_edit(cx);
        let mut graph = self.canvas.read(cx).editor.graph().clone();
        let changed = match action {
            LayerAction::ToggleVisibility(id) => {
                let visible = gpui_flow_core::node_is_visible(&graph, &id);
                gpui_flow_core::set_node_visible(&mut graph, &id, !visible)
            }
            LayerAction::ToggleLock(id) => {
                if graph
                    .groups
                    .values()
                    .any(|g| g.locked && g.node_ids.contains(&id))
                {
                    self.notice =
                        Some(tr("对象所在 Frame 已锁定", "The object's frame is locked").into());
                    cx.notify();
                    return;
                }
                let Some(node) = graph.nodes.get_mut(&id) else {
                    return;
                };
                node.locked = !node.locked;
                Ok(true)
            }
            LayerAction::Reorder(direction) => {
                let selected = self.canvas.read(cx).editor.selection.expanded_nodes(&graph);
                reorder_nodes(&mut graph, &selected, direction)
            }
        };
        match changed {
            Ok(true) => {
                self.command(CanvasCommand::ImportGraph(graph), cx);
            }
            Ok(false) => {}
            Err(error) => {
                self.notice = Some(error.to_string());
            }
        }
        cx.notify();
    }

    pub(super) fn layers_panel(&self, cx: &mut Context<Self>) -> Div {
        let view = self.canvas.read(cx);
        let graph = view.editor.graph().clone();
        let query = self.layer_query.read(cx).value();
        let matches = filtered_layers(&graph, query.as_ref());
        let shown = matches.nodes.len() + matches.groups.len();
        let nodes = matches
            .nodes
            .into_iter()
            .filter_map(|id| graph.node(&id).cloned())
            .collect::<Vec<_>>();
        let selected = view.editor.selection.expanded_nodes(&graph);
        let selected_groups = view.editor.selection.groups.clone();
        let can_focus = view.editor.selection_bounds().is_some();
        let hidden = gpui_flow_core::hidden_node_ids(&graph);
        let editable = view.is_editable() && !self.importing && !self.saving;
        let reorderable = editable
            && !selected.is_empty()
            && selected
                .iter()
                .all(|id| graph.node(id).is_some_and(|n| !n.locked))
            && !graph
                .groups
                .values()
                .any(|g| g.locked && !g.node_ids.is_disjoint(&selected));
        let mut actions = div().flex().items_center().gap_1().pb_2();
        for (id, icon, title, action) in [
            (
                "layer-front",
                "arrow-up-to-line",
                tr("置于最前", "Bring to front"),
                NodeOrderAction::BringToFront,
            ),
            (
                "layer-forward",
                "arrow-up",
                tr("上移一层", "Bring forward"),
                NodeOrderAction::BringForward,
            ),
            (
                "layer-backward",
                "arrow-down",
                tr("下移一层", "Send backward"),
                NodeOrderAction::SendBackward,
            ),
            (
                "layer-back",
                "arrow-down-to-line",
                tr("置于最后", "Send to back"),
                NodeOrderAction::SendToBack,
            ),
        ] {
            actions = actions.child(self.icon_button(
                id,
                icon,
                title,
                Action::Layer(LayerAction::Reorder(action)),
                !reorderable,
                cx,
            ));
        }
        actions = actions.child(self.icon_button(
            "layer-focus-selection",
            "focus",
            tr("定位选中对象", "Locate selected objects"),
            Action::FocusSelection,
            !can_focus,
            cx,
        ));
        let mut rows = div()
            .id("object-layers")
            .max_h(px((self.canvas_size.height - 290.).clamp(72., 420.)))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        for group in matches.groups.iter().filter_map(|id| graph.groups.get(id)) {
            rows = rows.child(
                self.button(
                    SharedString::from(format!("frame-layer-{}", group.id)),
                    tr_format!(
                        "▣ {} · {} 对象{}",
                        "▣ {} · {} objects{}",
                        group.title,
                        group.node_ids.len(),
                        if group.locked {
                            tr(" · 锁定", " · Locked")
                        } else {
                            ""
                        }
                    ),
                    Action::FrameEdit(frame_tools::FrameAction::Select(group.id.clone())),
                    false,
                    cx,
                )
                .w_full()
                .when(selected_groups.contains(&group.id), |b| b.bg(rgb(0xeff4ff))),
            );
        }
        for node in nodes.into_iter().rev() {
            let id = node.id.clone();
            let visible = !hidden.contains(&id);
            let is_selected = visible && selected.contains(&id);
            let grouped_lock = graph
                .groups
                .values()
                .any(|g| g.locked && g.node_ids.contains(&id));
            let icon = if node.locked || grouped_lock {
                "lock"
            } else {
                "lock-open"
            };
            let title = if node.locked {
                tr("解锁对象", "Unlock object")
            } else {
                tr("锁定对象", "Lock object")
            };
            let kind_icon = match node.kind.as_str() {
                "image" => "image",
                "text" => "type",
                "video" => "video",
                "audio" => "music",
                "model" => "box",
                "mask" => "paintbrush",
                "shape" => "circle",
                "pencil" => "pencil",
                _ => "layers",
            };
            let frames = graph
                .groups
                .values()
                .filter(|group| group.node_ids.contains(&id))
                .map(|group| group.title.as_str())
                .collect::<Vec<_>>()
                .join(" / ");
            rows = rows.child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .gap_1()
                    .rounded(px(7.))
                    .when(is_selected, |v| v.bg(rgb(0xeff4ff)))
                    .child(
                        Button::new(SharedString::from(format!("layer-select-{id}")))
                            .flex_1()
                            .min_w_0()
                            .h(px(if frames.is_empty() { 34. } else { 46. }))
                            .px_2()
                            .flex()
                            .items_center()
                            .gap_2()
                            .text_size(px(12.))
                            .text_color(rgb(if visible { 0x545a65 } else { 0xa8acb4 }))
                            .child(chrome::icon(kind_icon, 15.))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .gap_0()
                                    .child(div().truncate().child(node.title))
                                    .when(!frames.is_empty(), |v| {
                                        v.child(
                                            div()
                                                .truncate()
                                                .text_size(px(10.))
                                                .text_color(rgb(0x9297a0))
                                                .child(tr_format!(
                                                    "画框：{frames}",
                                                    "Frames: {frames}"
                                                )),
                                        )
                                    }),
                            )
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    window.prevent_default();
                                    if !visible {
                                        return;
                                    }
                                    this.apply_form_edit(cx);
                                    this.command(
                                        CanvasCommand::SelectNode {
                                            node: id.clone(),
                                            mode: if event.modifiers.shift {
                                                SelectionMode::Toggle
                                            } else {
                                                SelectionMode::Replace
                                            },
                                        },
                                        cx,
                                    );
                                    if event.click_count == 2 && !event.modifiers.shift {
                                        this.action(
                                            Action::NodeName(node_name::NameAction::Begin),
                                            window,
                                            cx,
                                        );
                                        return;
                                    }
                                    this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                                }),
                            ),
                    )
                    .child(
                        self.icon_button(
                            SharedString::from(format!("layer-visible-{}", node.id)),
                            if visible { "eye" } else { "eye-off" },
                            if visible {
                                tr("隐藏对象", "Hide object")
                            } else {
                                tr("显示对象", "Show object")
                            },
                            Action::Layer(LayerAction::ToggleVisibility(node.id.clone())),
                            !editable,
                            cx,
                        )
                        .size(px(28.)),
                    )
                    .child(
                        self.icon_button(
                            SharedString::from(format!("layer-lock-{}", node.id)),
                            icon,
                            title,
                            Action::Layer(LayerAction::ToggleLock(node.id)),
                            !editable || grouped_lock,
                            cx,
                        )
                        .size(px(28.)),
                    ),
            );
        }
        if shown == 0 {
            rows = rows.child(
                div()
                    .p_3()
                    .text_size(px(12.))
                    .text_color(rgb(0x9297a0))
                    .child(tr("没有匹配的对象或画框", "No matching objects or frames")),
            );
        }
        div()
            .occlude()
            .absolute()
            .left(px(78.))
            .top(px(92.))
            .w(px(280.))
            .p_3()
            .rounded_xl()
            .bg(rgb(0xffffff))
            .border_1()
            .border_color(rgb(0xe9eaed))
            .shadow_md()
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pb_2()
                    .child(div().text_sm().child(tr("图层", "Layers")))
                    .child(self.icon_button(
                        "close-layers",
                        "x",
                        tr("关闭图层", "Close layers"),
                        Action::Layers,
                        false,
                        cx,
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .pb_2()
                    .child(
                        Input::new(&self.layer_query)
                            .w(px(218.))
                            .h(px(30.))
                            .aria_label(tr("搜索图层名称", "Search layer names")),
                    )
                    .child(
                        Button::new("clear-layer-query")
                            .size(px(28.))
                            .child(chrome::icon("x", 14.))
                            .disabled(query.is_empty())
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.layer_query
                                    .update(cx, |input, cx| input.set_value("", window, cx));
                                this.layer_query.read(cx).focus_handle(cx).focus(window, cx);
                            })),
                    ),
            )
            .child(actions)
            .child(rows)
            .child(
                div()
                    .pt_2()
                    .text_size(px(10.))
                    .text_color(rgb(0x9297a0))
                    .child(tr_format!(
                        "显示 {shown}/{} · 上方在前 · Shift 多选",
                        "Showing {shown}/{} · Front layers at top · Shift to select multiple",
                        graph.nodes.len() + graph.groups.len()
                    )),
            )
            .when(!query.trim().is_empty(), |v| {
                v.child(label(tr(
                    "仅过滤列表，保留选择；画框仍选择整框",
                    "Filtering keeps the selection. Selecting a frame selects the entire frame.",
                )))
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_flow_core::{Graph, Group, Rect};

    #[test]
    fn filtering_keeps_layer_order_hidden_and_locked_rows_and_frame_context() {
        let registry = standard_registry();
        let mut graph = Graph::new();
        for (id, title) in [("a", "Hero"), ("b", "Background"), ("c", "HERO 中文")] {
            let mut node = registry
                .make_node("text", id, WorldPoint::ZERO, None)
                .unwrap();
            node.title = title.into();
            graph.add_node(node).unwrap();
        }
        gpui_flow_core::set_node_visible(&mut graph, &"c".into(), false).unwrap();
        graph
            .add_group(
                Group::new(
                    "frame",
                    "Banner",
                    Rect::new(WorldPoint::ZERO, WorldSize::new(400., 300.)),
                )
                .with_nodes(["a".into(), "b".into()]),
            )
            .unwrap();
        reorder_nodes(
            &mut graph,
            &["a".into()].into_iter().collect(),
            NodeOrderAction::BringToFront,
        )
        .unwrap();
        graph.nodes.get_mut(&"a".into()).unwrap().locked = true;
        let original = graph.clone();
        let matches = filtered_layers(&graph, "  hero  ");
        let expected: Vec<_> = ordered_node_ids(&graph)
            .into_iter()
            .filter(|id| id.as_str() != "b")
            .collect();
        assert_eq!(matches.nodes, expected);
        assert_eq!(matches.groups, vec!["frame".into()]);
        let frame = filtered_layers(&graph, "banner");
        assert_eq!(frame.nodes.len(), 2);
        assert!(!frame.nodes.contains(&"c".into()));
        assert_eq!(filtered_layers(&graph, "中文").nodes, vec!["c".into()]);
        assert!(filtered_layers(&graph, "missing").nodes.is_empty());
        assert!(filtered_layers(&graph, "missing").groups.is_empty());
        assert_eq!(filtered_layers(&graph, "").nodes, ordered_node_ids(&graph));
        assert_eq!(graph, original);
    }
}
