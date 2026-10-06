use super::*;
use crate::frames::{self, FrameContents};
use gpui_flow_core::{Group, GroupId, Rect};
use gpui_kit::component::input::{Input, InputState};

#[derive(Clone)]
pub(super) enum FrameAction {
    Draw,
    Cancel,
    Apply,
    Collect,
    Release,
    Remove,
    ToggleLock,
    Select(GroupId),
}
pub(super) struct FrameFields {
    source: Group,
    title: Entity<InputState>,
    width: Entity<InputState>,
    height: Entity<InputState>,
}
pub(super) struct FrameDrag {
    start: WorldPoint,
    end: WorldPoint,
    source: Option<Group>,
}

impl CreativeCanvas {
    fn selected_frame(&self, cx: &App) -> Option<Group> {
        let v = self.canvas.read(cx);
        if !v.editor.selection.nodes.is_empty() || v.editor.selection.groups.len() != 1 {
            return None;
        }
        v.editor
            .selection
            .groups
            .iter()
            .next()
            .and_then(|id| v.editor.graph().groups.get(id))
            .cloned()
    }
    pub(super) fn sync_frame_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected_frame(cx);
        if let Some(drag) = &self.frame_drag
            && let Some(source) = &drag.source
            && selected.as_ref() != Some(source)
        {
            self.frame_drag = None;
        }
        let Some(group) = selected else {
            self.frame_fields = None;
            return;
        };
        if self
            .frame_fields
            .as_ref()
            .is_some_and(|fields| fields.source == group)
        {
            return;
        }
        let input = |value: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut state = InputState::new(window, cx).context_menu(false);
                state.set_value(value, window, cx);
                state
            })
        };
        self.frame_fields = Some(FrameFields {
            title: input(group.title.clone(), window, cx),
            width: input(format!("{:.0}", group.bounds.size.width), window, cx),
            height: input(format!("{:.0}", group.bounds.size.height), window, cx),
            source: group,
        });
    }
    pub(super) fn clear_frame_tool(&mut self) {
        self.frame_drawing = false;
        self.frame_drag = None;
    }
    pub(super) fn handle_frame_action(
        &mut self,
        action: FrameAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let FrameAction::Select(group) = action {
            self.clear_frame_tool();
            self.command(
                CanvasCommand::SelectGroup {
                    group,
                    mode: SelectionMode::Replace,
                },
                cx,
            );
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            return;
        }
        if matches!(action, FrameAction::Cancel) {
            self.clear_frame_tool();
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            return;
        }
        if matches!(action, FrameAction::Draw) {
            let enabled = !self.frame_drawing;
            self.clear_frame_tool();
            self.frame_drawing = enabled;
            self.clear_image_edit();
            self.clear_mask_edit();
            self.clear_asset_export();
            self.asset_picker = None;
            self.show_layers = false;
            self.show_processing_menu = false;
            self.canvas.update(cx, |v, cx| v.set_pan_mode(false, cx));
            if enabled {
                self.command(CanvasCommand::ClearSelection, cx);
                self.frame_focus.focus(window, cx);
            } else {
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            }
            cx.notify();
            return;
        }
        let Some(group) = self.selected_frame(cx) else {
            return;
        };
        let mut graph = self.canvas.read(cx).editor.graph().clone();
        let result: Result<(), String> = (|| {
            match action {
                FrameAction::Apply => {
                    let fields = self.frame_fields.as_ref().ok_or("请重新选择画框")?;
                    if fields.source != group {
                        return Err("画框已改变，请重新输入".into());
                    }
                    let width = fields
                        .width
                        .read(cx)
                        .value()
                        .trim()
                        .parse::<f32>()
                        .map_err(|_| "请输入有效宽度")?;
                    let height = fields
                        .height
                        .read(cx)
                        .value()
                        .trim()
                        .parse::<f32>()
                        .map_err(|_| "请输入有效高度")?;
                    if !(frames::MIN_FRAME_SIZE..=frames::MAX_FRAME_SIZE).contains(&width)
                        || !(frames::MIN_FRAME_SIZE..=frames::MAX_FRAME_SIZE).contains(&height)
                    {
                        return Err("画框宽高应在 32 到 16384 之间".into());
                    }
                    if fields.title.read(cx).value().trim().is_empty() {
                        return Err("请输入画框名称".into());
                    }
                    frames::set_frame_bounds(
                        &mut graph,
                        &group.id,
                        Rect::new(group.bounds.origin, WorldSize::new(width, height)),
                    )
                    .map_err(|e| e.to_string())?;
                    frames::set_frame_title(
                        &mut graph,
                        &group.id,
                        fields.title.read(cx).value().trim().to_string(),
                    )
                    .map_err(|e| e.to_string())?;
                }
                FrameAction::Collect => {
                    let nodes = frames::enclosed_frame_members(&graph, &group.id)
                        .map_err(|e| e.to_string())?;
                    frames::add_frame_members(&mut graph, &group.id, &nodes)
                        .map_err(|e| e.to_string())?;
                }
                FrameAction::Release => {
                    frames::remove_frame_members(&mut graph, &group.id, &group.node_ids)
                        .map_err(|e| e.to_string())?;
                }
                FrameAction::Remove => {
                    frames::remove_frame(&mut graph, &group.id).map_err(|e| e.to_string())?;
                }
                FrameAction::ToggleLock => {
                    graph
                        .groups
                        .get_mut(&group.id)
                        .expect("selected frame")
                        .locked = !group.locked;
                }
                _ => return Ok(()),
            }
            Ok(())
        })();
        match result {
            Ok(()) => {
                if graph != *self.canvas.read(cx).editor.graph() {
                    self.command(CanvasCommand::ImportGraph(graph), cx);
                }
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            }
            Err(error) => self.notice = Some(error),
        }
        cx.notify();
    }
    fn frame_world_point(&self, point: Point<Pixels>, cx: &App) -> WorldPoint {
        let screen = WorldPoint::new(f32::from(point.x), f32::from(point.y)) - self.canvas_origin;
        self.canvas.read(cx).editor.viewport.screen_to_world(screen)
    }
    fn finish_frame_drag(
        &mut self,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut drag) = self.frame_drag.take() else {
            return;
        };
        drag.end = self.frame_world_point(point, cx);
        self.frame_drawing = false;
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            cx.notify();
            return;
        }
        let mut graph = self.canvas.read(cx).editor.graph().clone();
        let result = if let Some(source) = drag.source {
            if graph.groups.get(&source.id) != Some(&source) {
                Err("画框已改变，未应用旧拖动".to_string())
            } else {
                frames::frame_bounds_from_points(source.bounds.origin, drag.end)
                    .and_then(|bounds| frames::set_frame_bounds(&mut graph, &source.id, bounds))
                    .map(|_| source.id)
                    .map_err(|e| e.to_string())
            }
        } else {
            let zoom = self.canvas.read(cx).editor.viewport.zoom;
            let click = (drag.end.x - drag.start.x).hypot(drag.end.y - drag.start.y) * zoom < 4.;
            let bounds = if click {
                frames::default_frame_bounds(drag.start - WorldPoint::new(512., 512.))
            } else {
                frames::frame_bounds_from_points(drag.start, drag.end)
            };
            let id = GroupId::from(graph.fresh_id("frame"));
            let title = format!("画框 {}", graph.groups.len() + 1);
            bounds
                .and_then(|bounds| {
                    frames::create_frame(
                        &mut graph,
                        id,
                        title,
                        bounds,
                        FrameContents::EncloseVisible,
                    )
                })
                .map_err(|e| e.to_string())
        };
        match result {
            Ok(id) => {
                if self.command(CanvasCommand::ImportGraph(graph), cx) {
                    self.command(
                        CanvasCommand::SelectGroup {
                            group: id,
                            mode: SelectionMode::Replace,
                        },
                        cx,
                    );
                }
            }
            Err(error) => self.notice = Some(error),
        }
        self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    pub(super) fn frame_draw_overlay(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if !self.frame_drawing && self.frame_drag.is_none() {
            return None;
        }
        let viewport = self.canvas.read(cx).editor.viewport;
        let mut surface = div()
            .id("frame-drawing")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .track_focus(&self.frame_focus)
            .cursor_crosshair();
        if let Some(drag) = &self.frame_drag {
            let a = drag.source.as_ref().map_or(drag.start, |g| g.bounds.origin);
            let b = drag.end;
            let r = viewport.world_rect_to_screen(Rect::new(
                WorldPoint::new(a.x.min(b.x), a.y.min(b.y)),
                WorldSize::new((a.x - b.x).abs(), (a.y - b.y).abs()),
            ));
            surface = surface.child(
                div()
                    .absolute()
                    .left(px(r.origin.x))
                    .top(px(r.origin.y))
                    .w(px(r.size.width))
                    .h(px(r.size.height))
                    .border_1()
                    .border_color(rgb(0x4785e7))
                    .bg(rgba(0x4785e714))
                    .child(
                        div()
                            .absolute()
                            .top(px(-25.))
                            .text_size(px(12.))
                            .text_color(rgb(0x4785e7))
                            .child(format!(
                                "{:.0} × {:.0}",
                                r.size.width / viewport.zoom,
                                r.size.height / viewport.zoom
                            )),
                    ),
            );
        }
        surface = surface.child(
            div()
                .absolute()
                .left(px(180.))
                .bottom(px(22.))
                .px_3()
                .py_2()
                .rounded_lg()
                .bg(rgb(0xffffff))
                .shadow_sm()
                .text_size(px(12.))
                .child("拖动画框 · 单击创建 1024 × 1024 · Esc 取消"),
        );
        Some(
            surface
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        this.frame_focus.focus(window, cx);
                        if this.frame_drag.is_none() {
                            let start = this.frame_world_point(event.position, cx);
                            this.frame_drag = Some(FrameDrag {
                                start,
                                end: start,
                                source: None,
                            });
                            cx.notify();
                        }
                    }),
                )
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if event.dragging() {
                        let end = this.frame_world_point(event.position, cx);
                        if let Some(drag) = &mut this.frame_drag {
                            drag.end = end;
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseUpEvent, window, cx| {
                        cx.stop_propagation();
                        this.finish_frame_drag(event.position, window, cx);
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseUpEvent, window, cx| {
                        cx.stop_propagation();
                        this.finish_frame_drag(event.position, window, cx);
                    }),
                )
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.handle_frame_action(FrameAction::Cancel, window, cx);
                    }
                })),
        )
    }
    pub(super) fn frame_controls(&self, cx: &mut Context<Self>) -> Option<Div> {
        if self.frame_drawing || self.frame_drag.is_some() {
            return None;
        }
        let fields = self.frame_fields.as_ref()?;
        let group = self.selected_frame(cx)?;
        if fields.source != group {
            return None;
        }
        let view = self.canvas.read(cx);
        let canvas_editable = view.is_editable();
        let disabled = group.locked || !canvas_editable || self.importing || self.saving;
        let bounds = view.editor.viewport.world_rect_to_screen(group.bounds);
        let at = selection_toolbar::toolbar_position(bounds, self.canvas_size, 545.);
        let panel = anchored_panel(bounds, self.canvas_size, WorldSize::new(340., 70.));
        let bar = div()
            .occlude()
            .absolute()
            .left(px(at.x))
            .top(px(at.y))
            .w(px(545.))
            .h(px(42.))
            .px_2()
            .rounded(px(10.))
            .bg(rgb(0xffffff))
            .shadow_sm()
            .flex()
            .items_center()
            .gap_2()
            .child(chrome::icon("grid-2x2", 16.))
            .child(
                Input::new(&fields.title)
                    .w(px(150.))
                    .h(px(28.))
                    .disabled(disabled)
                    .aria_label("画框名称"),
            )
            .child(label("W"))
            .child(
                Input::new(&fields.width)
                    .w(px(62.))
                    .h(px(28.))
                    .disabled(disabled)
                    .aria_label("画框宽度"),
            )
            .child(label("H"))
            .child(
                Input::new(&fields.height)
                    .w(px(62.))
                    .h(px(28.))
                    .disabled(disabled)
                    .aria_label("画框高度"),
            )
            .child(self.button(
                "apply-frame",
                "应用",
                Action::FrameEdit(FrameAction::Apply),
                disabled,
                cx,
            ))
            .child(self.icon_button(
                "lock-frame",
                if group.locked { "lock" } else { "lock-open" },
                "锁定或解锁画框",
                Action::FrameEdit(FrameAction::ToggleLock),
                !canvas_editable || self.importing || self.saving,
                cx,
            ))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        let mut wrapper = div()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .child(bar)
            .child(
                div()
                    .occlude()
                    .absolute()
                    .left(px(panel.x))
                    .top(px(panel.y))
                    .w(px(340.))
                    .p_2()
                    .rounded(px(10.))
                    .bg(rgb(0xffffff))
                    .shadow_sm()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap_1()
                            .child(self.button(
                                "frame-collect",
                                "收纳框内对象",
                                Action::FrameEdit(FrameAction::Collect),
                                disabled,
                                cx,
                            ))
                            .child(self.button(
                                "frame-release",
                                "移出全部",
                                Action::FrameEdit(FrameAction::Release),
                                disabled || group.node_ids.is_empty(),
                                cx,
                            ))
                            .child(self.button(
                                "frame-remove",
                                "移除画框",
                                Action::FrameEdit(FrameAction::Remove),
                                disabled,
                                cx,
                            )),
                    )
                    .child(label(format!(
                        "{} 个对象 · 调整画框不会裁切或拉伸素材",
                        group.node_ids.len()
                    ))),
            );
        if !disabled {
            let id = group.id;
            wrapper = wrapper.child(
                div()
                    .id("frame-resize-handle")
                    .absolute()
                    .left(px(bounds.origin.x + bounds.size.width - 6.))
                    .top(px(bounds.origin.y + bounds.size.height - 6.))
                    .size(px(12.))
                    .bg(rgb(0xffffff))
                    .border_1()
                    .border_color(rgb(0x4785e7))
                    .cursor_crosshair()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                            cx.stop_propagation();
                            window.prevent_default();
                            let Some(group) =
                                this.canvas.read(cx).editor.graph().groups.get(&id).cloned()
                            else {
                                return;
                            };
                            let point = this.frame_world_point(event.position, cx);
                            this.frame_drag = Some(FrameDrag {
                                start: point,
                                end: group.bounds.origin
                                    + WorldPoint::new(
                                        group.bounds.size.width,
                                        group.bounds.size.height,
                                    ),
                                source: Some(group),
                            });
                            this.frame_focus.focus(window, cx);
                            cx.notify();
                        }),
                    ),
            );
        }
        Some(wrapper)
    }
}
