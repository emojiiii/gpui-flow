use super::*;
use gpui_kit::component::input::{Input, InputState};

#[derive(Clone)]
pub(super) enum NameAction {
    Begin,
    Apply,
    Cancel,
}

pub(super) struct NamePanel {
    source: Node,
    input: Entity<InputState>,
    error: Option<String>,
    _subscription: Subscription,
}

/// Titles are display metadata. Every other node field and the graph stay exact.
fn renamed_graph(
    graph: &gpui_flow_core::Graph,
    source: &Node,
    input: &str,
) -> Result<Option<gpui_flow_core::Graph>, String> {
    let current = graph.node(&source.id).ok_or("对象已不存在")?;
    if current != source {
        return Err("对象已变化，请重新打开重命名".into());
    }
    if crate::layer_guard::node_is_locked(graph, &source.id) {
        return Err("对象或所属画框已锁定".into());
    }
    let title = input.trim();
    if title.is_empty() {
        return Err("请输入对象名称".into());
    }
    if title.len() > 512 {
        return Err("名称过长，请缩短后再保存".into());
    }
    if title
        .chars()
        .any(|c| c.is_control() || matches!(c, '\u{2028}' | '\u{2029}'))
    {
        return Err("名称请使用单行文字".into());
    }
    if title == current.title {
        return Ok(None);
    }
    let mut next = graph.clone();
    next.nodes.get_mut(&source.id).expect("existing node").title = title.to_string();
    Ok(Some(next))
}

impl CreativeCanvas {
    pub(super) fn is_node_name_open(&self, id: &NodeId) -> bool {
        self.node_name
            .as_ref()
            .is_some_and(|panel| &panel.source.id == id)
    }
    pub(super) fn close_node_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.node_name.take().is_some() {
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
    }
    pub(super) fn reconcile_node_name(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .node_name
            .as_ref()
            .is_some_and(|panel| self.selected(cx).as_ref() != Some(&panel.source))
        {
            self.close_node_name(window, cx);
        }
    }
    pub(super) fn handle_node_name(
        &mut self,
        action: NameAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, NameAction::Cancel) {
            self.close_node_name(window, cx);
            cx.notify();
            return;
        }
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            return;
        }
        let Some(node) = self.selected(cx).filter(|node| {
            !crate::layer_guard::node_is_locked(self.canvas.read(cx).editor.graph(), &node.id)
        }) else {
            return;
        };
        if matches!(action, NameAction::Begin) {
            self.clear_text_edit();
            self.clear_image_edit();
            self.asset_picker = None;
            self.show_layers = false;
            self.show_colors = false;
            let input = cx.new(|cx| {
                let mut state = InputState::new(window, cx).context_menu(false);
                state.set_value(node.title.clone(), window, cx);
                state.select_all(window, cx);
                state
            });
            let subscription =
                cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
                    InputEvent::PressEnter {
                        secondary: false,
                        shift: false,
                    } => this.action(Action::NodeName(NameAction::Apply), window, cx),
                    InputEvent::Change => {
                        if let Some(panel) = &mut this.node_name {
                            panel.error = None;
                        }
                        cx.notify();
                    }
                    _ => {}
                });
            input.read(cx).focus_handle(cx).focus(window, cx);
            self.node_name = Some(NamePanel {
                source: node,
                input,
                error: None,
                _subscription: subscription,
            });
        } else if let Some(panel) = &self.node_name {
            let result = renamed_graph(
                self.canvas.read(cx).editor.graph(),
                &panel.source,
                panel.input.read(cx).value().as_ref(),
            );
            match result {
                Ok(Some(graph)) => {
                    if self.command(CanvasCommand::ImportGraph(graph), cx) {
                        self.close_node_name(window, cx);
                    }
                }
                Ok(None) => self.close_node_name(window, cx),
                Err(error) => {
                    if let Some(panel) = &mut self.node_name {
                        panel.error = Some(error);
                    }
                }
            }
        }
        cx.notify();
    }
    pub(super) fn node_name_composer(
        &self,
        node: &Node,
        bounds: gpui_flow_core::Rect,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(panel) = &self.node_name else {
            return div();
        };
        let size = self.composer_size(node, cx);
        let at = anchored_panel(bounds, self.canvas_size, size);
        let content = div()
            .p_2()
            .flex()
            .flex_col()
            .gap_2()
            .child(label("画布对象名称"))
            .child(
                Input::new(&panel.input)
                    .h(px(32.))
                    .disabled(!editable || self.saving)
                    .aria_label("对象名称"),
            )
            .when_some(panel.error.clone(), |v, error| {
                v.child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(0xb46649))
                        .child(error),
                )
            })
            .child(label("Enter 应用 · Esc 取消"))
            .child(
                div()
                    .flex()
                    .justify_end()
                    .gap_2()
                    .child(self.button(
                        "cancel-node-name",
                        "取消",
                        Action::NodeName(NameAction::Cancel),
                        false,
                        cx,
                    ))
                    .child(self.button(
                        "apply-node-name",
                        "应用名称",
                        Action::NodeName(NameAction::Apply),
                        !editable || self.saving,
                        cx,
                    )),
            );
        div()
            .occlude()
            .absolute()
            .left(px(at.x))
            .top(px(at.y))
            .w(px(size.width))
            .rounded_xl()
            .bg(rgb(0xffffff))
            .shadow_sm()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("node-name-scroll")
                    .max_h(px(size.height))
                    .overflow_y_scroll()
                    .child(content),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_flow_core::{Editor, Graph, Group, Rect};
    use gpui_flow_interaction::PluginHost;
    #[test]
    fn rename_preserves_identity_content_assets_links_and_uses_history() {
        let registry = Arc::new(standard_registry());
        let node = registry.make_node("image","image",WorldPoint::new(100.,100.),Some(serde_json::json!({"asset":"assets/source.png","caption":"keep caption","host":{"keep":true}}))).unwrap();
        let mut graph = Graph::new();
        graph.add_node(node.clone()).unwrap();
        graph
            .add_node(
                registry
                    .make_node("generate_image", "gen", WorldPoint::new(500., 100.), None)
                    .unwrap(),
            )
            .unwrap();
        graph
            .connect(
                "input",
                gpui_flow_core::PortRef::new("image", "image"),
                gpui_flow_core::PortRef::new("gen", "reference"),
            )
            .unwrap();
        let renamed = renamed_graph(&graph, &node, "  主视觉  ").unwrap().unwrap();
        let mut expected = graph.clone();
        expected.nodes.get_mut(&node.id).unwrap().title = "主视觉".into();
        assert_eq!(renamed, expected);
        let mut editor = Editor::new(graph.clone()).unwrap();
        let mut host = PluginHost::with_defaults();
        host.install(CreativeGuard(registry)).unwrap();
        host.execute(&mut editor, CanvasCommand::ImportGraph(renamed))
            .unwrap();
        host.execute(&mut editor, CanvasCommand::Undo).unwrap();
        assert_eq!(editor.graph(), &graph);
        host.execute(&mut editor, CanvasCommand::Redo).unwrap();
        assert_eq!(editor.graph(), &expected);
    }
    #[test]
    fn rename_rejects_invalid_names_stale_sources_and_parent_locks_without_changes() {
        let node = standard_registry()
            .make_node("text", "text", WorldPoint::ZERO, None)
            .unwrap();
        let mut graph = Graph::new();
        graph.add_node(node.clone()).unwrap();
        let before = graph.clone();
        for name in [
            "".to_string(),
            "  ".into(),
            "a\nb".into(),
            "a\u{2028}b".into(),
            "字".repeat(171),
        ] {
            assert!(renamed_graph(&graph, &node, &name).is_err());
            assert_eq!(graph, before);
        }
        assert!(renamed_graph(&graph, &node, &"字".repeat(170)).is_ok());
        assert!(renamed_graph(&graph, &node, &node.title).unwrap().is_none());
        graph.nodes.get_mut(&node.id).unwrap().data["text"] = serde_json::json!("new content");
        assert!(renamed_graph(&graph, &node, "Name").is_err());
        graph = before;
        let mut group = Group::new(
            "frame",
            "Frame",
            Rect::new(WorldPoint::ZERO, WorldSize::new(500., 500.)),
        )
        .with_nodes([node.id.clone()]);
        group.locked = true;
        graph.add_group(group).unwrap();
        assert!(renamed_graph(&graph, &node, "Name").is_err());
    }
}
