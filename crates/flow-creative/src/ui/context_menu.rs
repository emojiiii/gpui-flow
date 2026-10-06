use super::*;
use gpui_flow::CanvasContextMenuRequest;
use gpui_flow_core::{NodeOrderAction, Selection};
use gpui_flow_interaction::HitTarget;

#[derive(Clone)]
pub(super) enum ContextAction {
    Copy,
    Paste(WorldPoint),
    Invoke(Selection, Box<Action>),
}

pub(super) struct ObjectMenu {
    position: WorldPoint,
    world: WorldPoint,
    selection: Selection,
}

fn target_selection_command(
    hit: &HitTarget,
    selection: &Selection,
    graph: &gpui_flow_core::Graph,
) -> Option<CanvasCommand> {
    match hit {
        HitTarget::Node(id) | HitTarget::ResizeHandle(id) => {
            (!selection.expanded_nodes(graph).contains(id)).then(|| CanvasCommand::SelectNode {
                node: id.clone(),
                mode: SelectionMode::Replace,
            })
        }
        HitTarget::Port(port) => {
            (!selection.expanded_nodes(graph).contains(&port.node)).then(|| {
                CanvasCommand::SelectNode {
                    node: port.node.clone(),
                    mode: SelectionMode::Replace,
                }
            })
        }
        HitTarget::Group(id) => {
            (!selection.groups.contains(id)).then(|| CanvasCommand::SelectGroup {
                group: id.clone(),
                mode: SelectionMode::Replace,
            })
        }
        HitTarget::Edge(id) => (!selection.edges.contains(id)).then(|| CanvasCommand::SelectEdge {
            edge: id.clone(),
            mode: SelectionMode::Replace,
        }),
        HitTarget::Background => (!selection.is_empty()).then_some(CanvasCommand::ClearSelection),
        _ => None,
    }
}

impl CreativeCanvas {
    pub(super) fn open_object_menu(
        &mut self,
        request: CanvasContextMenuRequest,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A context click must not discard an unfinished paint/crop/form draft.
        if self.importing
            || self.custom_designer.is_some()
            || self.export_dialog.is_some()
            || self.show_rename
            || self.mask_editing.is_some()
            || self.image_editing.is_some()
            || self.image_rotation.is_some()
            || self.image_adjustment.is_some()
            || self.typography.is_some()
            || self.background_panel.is_some()
            || self.pencil_active
            || self.shape_tool.is_some()
            || self.frame_drawing
            || self.editing_text.lock().is_ok_and(|id| id.is_some())
        {
            return;
        }
        if !matches!(
            request.hit,
            HitTarget::Node(_)
                | HitTarget::ResizeHandle(_)
                | HitTarget::Port(_)
                | HitTarget::Group(_)
                | HitTarget::Edge(_)
                | HitTarget::Background
        ) {
            return;
        }
        self.apply_form_edit(cx);
        let view = self.canvas.read(cx);
        let command =
            target_selection_command(&request.hit, &view.editor.selection, view.editor.graph());
        if let Some(command) = command
            && !self.command(command, cx)
        {
            return;
        }
        let view = self.canvas.read(cx);
        let menu = ObjectMenu {
            position: request.position,
            world: view.editor.viewport.screen_to_world(request.position),
            selection: view.editor.selection.clone(),
        };
        self.asset_picker = None;
        self.show_layers = false;
        self.show_colors = false;
        self.show_arrangement = false;
        self.show_processing_menu = false;
        self.quick_edit = false;
        self.object_menu = Some(menu);
        self.object_menu_focus.focus(window, cx);
        cx.notify();
    }

    pub(super) fn reconcile_object_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .object_menu
            .as_ref()
            .is_some_and(|menu| menu.selection != self.canvas.read(cx).editor.selection)
        {
            self.object_menu = None;
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
    }

    pub(super) fn context_action(
        &mut self,
        action: ContextAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            ContextAction::Invoke(selection, action) => {
                if selection != self.canvas.read(cx).editor.selection {
                    self.notice = Some("所选对象已变化，请重新打开菜单".into());
                } else {
                    self.action(*action, window, cx);
                }
            }
            ContextAction::Copy => {
                if self.command(CanvasCommand::CopySelection, cx) {
                    self.notice = Some("已复制对象，可在本画布粘贴".into());
                }
            }
            ContextAction::Paste(at) => {
                if self.importing || self.saving {
                    return;
                }
                let result = self
                    .canvas
                    .update(cx, |canvas, cx| canvas.paste_copied_selection_at(at, cx));
                self.notice = result.err().map(|error| error.to_string());
            }
        }
        cx.notify();
    }

    pub(super) fn object_context_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(menu) = &self.object_menu else {
            return div().into_any_element();
        };
        let selection = menu.selection.clone();
        let position = menu.position;
        let world = menu.world;
        let view = self.canvas.read(cx);
        let graph = view.editor.graph();
        let expanded = selection.expanded_nodes(graph);
        let editable = view.is_editable() && !self.importing && !self.saving;
        let locked = expanded
            .iter()
            .any(|id| crate::layer_guard::node_is_locked(graph, id))
            || selection
                .groups
                .iter()
                .any(|id| graph.groups.get(id).is_some_and(|group| group.locked))
            || selection.edges.iter().any(|id| {
                graph.edges.get(id).is_some_and(|edge| {
                    crate::layer_guard::node_is_locked(graph, &edge.source.node)
                        || crate::layer_guard::node_is_locked(graph, &edge.target.node)
                })
            });
        let has_objects = !expanded.is_empty() || !selection.groups.is_empty();
        let single = (selection.nodes.len() == 1
            && selection.groups.is_empty()
            && selection.edges.is_empty())
        .then(|| selection.nodes.iter().next().and_then(|id| graph.node(id)))
        .flatten();
        let single_group = (selection.groups.len() == 1
            && selection.nodes.is_empty()
            && selection.edges.is_empty())
        .then(|| {
            selection
                .groups
                .iter()
                .next()
                .and_then(|id| graph.groups.get(id))
        })
        .flatten();
        let title = single
            .map(|node| node.title.clone())
            .or_else(|| single_group.map(|group| group.title.clone()))
            .unwrap_or_else(|| {
                if selection.is_empty() {
                    "画布".into()
                } else {
                    format!("{} 项选择", selection.len())
                }
            });
        let mut rows = vec![
            (
                "复制对象",
                Action::Context(ContextAction::Copy),
                !has_objects,
            ),
            (
                "粘贴对象到此处",
                Action::Context(ContextAction::Paste(world)),
                !editable || !view.can_paste_selection(),
            ),
        ];
        if has_objects {
            rows.push(("创建副本", Action::Duplicate, !editable));
            if selection.groups.is_empty() && !selection.nodes.is_empty() {
                rows.extend([
                    (
                        "置于最前",
                        Action::Layer(layers::LayerAction::Reorder(NodeOrderAction::BringToFront)),
                        !editable || locked,
                    ),
                    (
                        "上移一层",
                        Action::Layer(layers::LayerAction::Reorder(NodeOrderAction::BringForward)),
                        !editable || locked,
                    ),
                    (
                        "下移一层",
                        Action::Layer(layers::LayerAction::Reorder(NodeOrderAction::SendBackward)),
                        !editable || locked,
                    ),
                    (
                        "置于最后",
                        Action::Layer(layers::LayerAction::Reorder(NodeOrderAction::SendToBack)),
                        !editable || locked,
                    ),
                    ("编组", Action::Frame, !editable || locked),
                ]);
                if selection.nodes.len() > 1 {
                    rows.push(("排列所选对象…", Action::Arrange(None), !editable || locked));
                }
            }
            if !selection.groups.is_empty() {
                rows.push(("取消编组", Action::Ungroup, !editable || locked));
            }
        }
        if let Some(node) = single {
            let group_locked = graph
                .groups
                .values()
                .any(|group| group.locked && group.node_ids.contains(&node.id));
            rows.push((
                if node.locked {
                    "解锁对象"
                } else {
                    "锁定对象"
                },
                Action::Layer(layers::LayerAction::ToggleLock(node.id.clone())),
                !editable || group_locked,
            ));
            if node.kind == "text" {
                rows.push(("编辑文字", Action::EditText, !editable || locked));
            }
            if node.kind == "image" {
                rows.push(("用于图生图", Action::UseReference, !editable || locked));
            }
            if crate::asset_export::has_media_output(node) {
                let has_output = self.project.as_ref().is_some_and(|project| {
                    crate::asset_export::export_assets(graph, &node.id, project.assets()).is_ok()
                });
                rows.push((
                    "导出本地素材…",
                    Action::ExportAsset(asset_export_ui::ExportAction::Open),
                    !has_output || self.importing,
                ));
            }
        }
        if let Some(group) = single_group {
            rows.push((
                if group.locked {
                    "解锁画框"
                } else {
                    "锁定画框"
                },
                Action::FrameEdit(frame_tools::FrameAction::ToggleLock),
                !editable,
            ));
        }
        if selection.is_empty() {
            rows.extend([
                ("导入本地素材…", Action::ImportImage, !editable),
                ("添加文字", Action::Add("text".into()), !editable),
                ("适应画布", Action::Fit, false),
            ]);
        } else {
            rows.push(("删除所选项", Action::Delete, !editable || locked));
        }
        let size = bounded_panel_size(
            self.canvas_size,
            WorldSize::new(236., 48. + rows.len() as f32 * 30.),
        );
        let at = WorldPoint::new(
            position
                .x
                .clamp(12., (self.canvas_size.width - size.width - 12.).max(12.)),
            position.y.clamp(
                62_f32.min((self.canvas_size.height - size.height - 12.).max(12.)),
                (self.canvas_size.height - size.height - 12.).max(12.),
            ),
        );
        let mut content = div().min_w(px(220.)).p_2().flex().flex_col().child(
            div()
                .px_2()
                .pb_2()
                .text_size(px(11.))
                .text_color(rgb(0x90939d))
                .overflow_hidden()
                .text_ellipsis()
                .whitespace_nowrap()
                .child(title),
        );
        for (index, (label, action, disabled)) in rows.into_iter().enumerate() {
            content = content.child(
                self.button(
                    SharedString::from(format!("context-row-{index}")),
                    label,
                    Action::Context(ContextAction::Invoke(selection.clone(), Box::new(action))),
                    disabled,
                    cx,
                )
                .w_full()
                .h(px(30.))
                .flex_shrink_0()
                .justify_start(),
            );
        }
        div()
            .id("creative-context-menu")
            .track_focus(&self.object_menu_focus)
            .occlude()
            .absolute()
            .left(px(at.x))
            .top(px(at.y))
            .w(px(size.width))
            .bg(rgb(0xffffff))
            .border_1()
            .border_color(rgb(0xe6e7eb))
            .rounded(px(10.))
            .shadow_md()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                this.object_menu = None;
                this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.object_menu = None;
                    this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                    cx.stop_propagation();
                    cx.notify();
                    return;
                }
                if event.keystroke.modifiers.control || event.keystroke.modifiers.platform {
                    let Some(menu) = &this.object_menu else {
                        return;
                    };
                    let action = match event.keystroke.key.as_str() {
                        "c" => Action::Context(ContextAction::Copy),
                        "v" => Action::Context(ContextAction::Paste(menu.world)),
                        "z" if event.keystroke.modifiers.shift => Action::Redo,
                        "z" => Action::Undo,
                        _ => return,
                    };
                    let action = ContextAction::Invoke(menu.selection.clone(), Box::new(action));
                    cx.stop_propagation();
                    this.action(Action::Context(action), window, cx);
                }
            }))
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("context-menu-scroll")
                    .max_h(px(size.height))
                    .overflow_y_scroll()
                    .overflow_x_scroll()
                    .child(content),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    #[test]
    fn right_click_preserves_selected_batch_and_retargets_unselected_objects() {
        let mut graph = gpui_flow_core::Graph::new();
        for id in ["a", "b", "c"] {
            graph.add_node(Node::new(id, id, WorldPoint::ZERO)).unwrap();
        }
        let selection = Selection {
            nodes: ["a".into(), "b".into()].into_iter().collect(),
            ..Default::default()
        };
        assert!(
            target_selection_command(&HitTarget::Node("b".into()), &selection, &graph).is_none()
        );
        assert!(matches!(
            target_selection_command(&HitTarget::Node("c".into()), &selection, &graph),
            Some(CanvasCommand::SelectNode {
                mode: SelectionMode::Replace,
                ..
            })
        ));
        assert_eq!(
            target_selection_command(&HitTarget::Background, &selection, &graph),
            Some(CanvasCommand::ClearSelection)
        );
    }
}
