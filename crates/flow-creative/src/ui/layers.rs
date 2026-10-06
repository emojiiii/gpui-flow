use super::*;
use gpui_flow_core::{NodeOrderAction, ordered_node_ids, reorder_nodes};

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
                    self.notice = Some("对象所在 Frame 已锁定".into());
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
        let nodes = ordered_node_ids(&graph)
            .into_iter()
            .filter_map(|id| graph.node(&id).cloned())
            .collect::<Vec<_>>();
        let selected = view.editor.selection.expanded_nodes(&graph);
        let selected_groups = view.editor.selection.groups.clone();
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
                "置于最前",
                NodeOrderAction::BringToFront,
            ),
            (
                "layer-forward",
                "arrow-up",
                "上移一层",
                NodeOrderAction::BringForward,
            ),
            (
                "layer-backward",
                "arrow-down",
                "下移一层",
                NodeOrderAction::SendBackward,
            ),
            (
                "layer-back",
                "arrow-down-to-line",
                "置于最后",
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
        let mut rows = div()
            .id("object-layers")
            .max_h(px(420.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap_1();
        for group in graph.groups.values().rev() {
            rows = rows.child(
                self.button(
                    SharedString::from(format!("frame-layer-{}", group.id)),
                    format!(
                        "▣ {} · {} 对象{}",
                        group.title,
                        group.node_ids.len(),
                        if group.locked { " · 锁定" } else { "" }
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
                "解锁对象"
            } else {
                "锁定对象"
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
            rows = rows.child(
                div()
                    .flex()
                    .items_center()
                    .gap_1()
                    .rounded(px(7.))
                    .when(is_selected, |v| v.bg(rgb(0xeff4ff)))
                    .child(
                        Button::new(SharedString::from(format!("layer-select-{id}")))
                            .flex_1()
                            .min_w_0()
                            .h(px(34.))
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
                                    .overflow_hidden()
                                    .text_ellipsis()
                                    .whitespace_nowrap()
                                    .child(node.title),
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
                                    this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                                }),
                            ),
                    )
                    .child(
                        self.icon_button(
                            SharedString::from(format!("layer-visible-{}", node.id)),
                            if visible { "eye" } else { "eye-off" },
                            if visible {
                                "隐藏对象"
                            } else {
                                "显示对象"
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
                    .child(div().text_sm().child("图层"))
                    .child(self.icon_button(
                        "close-layers",
                        "x",
                        "关闭图层",
                        Action::Layers,
                        false,
                        cx,
                    )),
            )
            .child(actions)
            .child(rows)
            .child(
                div()
                    .pt_2()
                    .text_size(px(10.))
                    .text_color(rgb(0x9297a0))
                    .child("上方在前 · Shift 多选 · 锁定后仍可查看"),
            )
    }
}
