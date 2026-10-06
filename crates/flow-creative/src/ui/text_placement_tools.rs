use super::*;
use gpui_flow_core::{Rect, Viewport};

#[derive(Clone)]
pub(super) enum TextPlacementAction {
    Toggle,
    Cancel,
    At(WorldPoint),
}

pub(super) struct TextDrag {
    start: WorldPoint,
    end: WorldPoint,
    viewport: Viewport,
}

impl CreativeCanvas {
    pub(super) fn clear_text_placement(&mut self) {
        self.text_placing = false;
        self.text_placement_drag = None;
    }

    pub(super) fn handle_text_placement(
        &mut self,
        action: TextPlacementAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, TextPlacementAction::Cancel) {
            self.clear_text_placement();
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            return;
        }
        if let TextPlacementAction::At(at) = action {
            match crate::text_placement::default_text_bounds(at) {
                Ok(bounds) => self.insert_placed_text(bounds, window, cx),
                Err(error) => self.notice = Some(error.to_string()),
            }
            cx.notify();
            return;
        }
        let enabled = !self.text_placing;
        self.clear_text_placement();
        self.apply_form_edit(cx);
        self.clear_text_edit();
        self.clear_image_edit();
        self.clear_mask_edit();
        self.clear_asset_export();
        self.asset_picker = None;
        self.show_layers = false;
        self.show_colors = false;
        self.show_processing_menu = false;
        self.quick_edit = false;
        self.canvas.update(cx, |v, cx| v.set_pan_mode(false, cx));
        self.text_placing = enabled;
        if enabled {
            self.command(CanvasCommand::ClearSelection, cx);
            self.text_placement_focus.focus(window, cx);
        } else {
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        }
        cx.notify();
    }

    fn insert_placed_text(&mut self, bounds: Rect, window: &mut Window, cx: &mut Context<Self>) {
        self.clear_text_placement();
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            return;
        }
        self.apply_form_edit(cx);
        let graph = self.canvas.read(cx).editor.graph();
        let id = graph.fresh_id(&format!("creative-{}", self.next_id));
        self.next_id += 1;
        let result = placed_text_graph(&self.registry, graph, bounds, id.into());
        match result {
            Ok((graph, id)) => {
                if self.command(CanvasCommand::ImportGraph(graph), cx) {
                    self.begin_text_edit(id, window, cx);
                } else {
                    self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                }
            }
            Err(error) => {
                self.notice = Some(error);
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            }
        }
        cx.notify();
    }

    fn text_placement_screen(&self, at: Point<Pixels>) -> WorldPoint {
        WorldPoint::new(f32::from(at.x), f32::from(at.y)) - self.canvas_origin
    }

    pub(super) fn finish_text_placement(
        &mut self,
        at: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.text_placement_drag.take() else {
            return;
        };
        if drag.viewport != self.canvas.read(cx).editor.viewport {
            self.handle_text_placement(TextPlacementAction::Cancel, window, cx);
            return;
        }
        match crate::text_placement::text_bounds_from_screen_points(
            drag.start,
            self.text_placement_screen(at),
            drag.viewport,
        ) {
            Ok(bounds) => self.insert_placed_text(bounds, window, cx),
            Err(error) => {
                self.notice = Some(error.to_string());
                cx.notify();
            }
        }
    }

    pub(super) fn text_placement_overlay(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if !self.text_placing {
            return None;
        }
        let mut overlay = div()
            .id("text-placement-surface")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .track_focus(&self.text_placement_focus)
            .cursor_text();
        if let Some(drag) = &self.text_placement_drag
            && let Ok(bounds) = crate::text_placement::text_bounds_from_screen_points(
                drag.start,
                drag.end,
                drag.viewport,
            )
        {
            let screen = drag.viewport.world_rect_to_screen(bounds);
            overlay = overlay.child(
                div()
                    .absolute()
                    .left(px(screen.origin.x))
                    .top(px(screen.origin.y))
                    .w(px(screen.size.width))
                    .h(px(screen.size.height))
                    .border_1()
                    .border_color(rgb(0x4785e7))
                    .bg(rgba(0x4785e710)),
            );
        }
        Some(
            overlay
                .child(
                    div()
                        .absolute()
                        .left(px(80.))
                        .bottom(px(64.))
                        .max_w(px((self.canvas_size.width - 98.).max(24.)))
                        .px_3()
                        .py_2()
                        .rounded_lg()
                        .bg(rgb(0xffffff))
                        .shadow_sm()
                        .text_size(px(12.))
                        .child("点击放置文字 · 拖动设定文本框 · Esc 取消"),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        this.text_placement_focus.focus(window, cx);
                        if this.text_placement_drag.is_none() {
                            let start = this.text_placement_screen(event.position);
                            this.text_placement_drag = Some(TextDrag {
                                start,
                                end: start,
                                viewport: this.canvas.read(cx).editor.viewport,
                            });
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if this.text_placement_drag.is_some() {
                        cx.stop_propagation();
                        if event.pressed_button != Some(MouseButton::Left) {
                            this.text_placement_drag = None;
                        } else {
                            let end = this.text_placement_screen(event.position);
                            if let Some(drag) = &mut this.text_placement_drag {
                                drag.end = end;
                            }
                        }
                        cx.notify();
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseUpEvent, window, cx| {
                        cx.stop_propagation();
                        this.finish_text_placement(event.position, window, cx);
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _: &MouseUpEvent, window, cx| {
                        cx.stop_propagation();
                        this.handle_text_placement(TextPlacementAction::Cancel, window, cx);
                    }),
                )
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if matches!(event.keystroke.key.as_str(), "escape" | "t" | "v" | "h")
                        && !event.keystroke.modifiers.control
                        && !event.keystroke.modifiers.platform
                        && !event.keystroke.modifiers.alt
                    {
                        cx.stop_propagation();
                        this.handle_text_placement(TextPlacementAction::Cancel, window, cx);
                        if event.keystroke.key == "h" {
                            this.action(Action::PanTool(true), window, cx);
                        }
                    }
                })),
        )
    }
}

fn placed_text_graph(
    registry: &CreativeRegistry,
    graph: &gpui_flow_core::Graph,
    bounds: Rect,
    id: NodeId,
) -> Result<(gpui_flow_core::Graph, NodeId), String> {
    if graph
        .groups
        .values()
        .any(|frame| frame.locked && frame.bounds.contains_rect(bounds))
    {
        return Err("该画框已锁定，请解锁后添加文字".into());
    }
    let frame = crate::text_placement::containing_text_frame(graph, bounds);
    let mut node = registry
        .make_node("text", id, bounds.origin, None)
        .map_err(|error| error.to_string())?;
    node.size = bounds.size;
    node.data["text"] = Value::String(String::new());
    let id = node.id.clone();
    let mut candidate = graph.clone();
    candidate
        .add_node(node)
        .map_err(|error| error.to_string())?;
    if let Some(frame) = frame {
        crate::frames::add_frame_members(
            &mut candidate,
            &frame,
            &[id.clone()].into_iter().collect(),
        )
        .map_err(|error| error.to_string())?;
    }
    Ok((candidate, id))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    use gpui_flow_core::{Editor, Graph};
    use gpui_flow_interaction::PluginHost;

    #[test]
    fn placed_text_and_frame_membership_undo_together_and_locked_frame_rejects() {
        let registry = Arc::new(standard_registry());
        let mut graph = Graph::new();
        crate::frames::create_frame(
            &mut graph,
            "frame",
            "Frame",
            Rect::new(WorldPoint::new(100., 100.), WorldSize::new(800., 600.)),
            crate::frames::FrameContents::Empty,
        )
        .unwrap();
        let bounds = Rect::new(WorldPoint::new(140., 160.), WorldSize::new(360., 120.));
        let (placed, id) = placed_text_graph(&registry, &graph, bounds, "text".into()).unwrap();
        assert!(graph.nodes.is_empty());
        assert_eq!(placed.node(&id).unwrap().bounds(), bounds);
        assert_eq!(placed.node(&id).unwrap().data["text"], "");
        assert!(placed.groups[&"frame".into()].node_ids.contains(&id));
        let mut editor = Editor::new(graph.clone()).unwrap();
        let mut host = PluginHost::with_defaults();
        host.install(CreativeGuard(registry.clone())).unwrap();
        host.execute(&mut editor, CanvasCommand::ImportGraph(placed.clone()))
            .unwrap();
        host.execute(&mut editor, CanvasCommand::Undo).unwrap();
        assert_eq!(editor.graph(), &graph);
        host.execute(&mut editor, CanvasCommand::Redo).unwrap();
        assert_eq!(editor.graph(), &placed);
        graph.groups.get_mut(&"frame".into()).unwrap().locked = true;
        assert!(placed_text_graph(&registry, &graph, bounds, "text".into()).is_err());
        assert!(graph.nodes.is_empty());
        assert!(graph.groups[&"frame".into()].node_ids.is_empty());
    }
}
