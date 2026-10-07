use super::*;
use crate::pencil::{self, PencilData, PencilStyle};
use gpui_kit::component::input::{Input, InputState};

#[derive(Clone)]
pub(super) enum PencilAction {
    Toggle,
    Done,
    Apply,
    Color(u32),
}
pub(super) struct PencilFields {
    source: Option<Node>,
    color: Entity<InputState>,
    width: Entity<InputState>,
    opacity: Entity<InputState>,
}
pub(super) struct PencilDraft {
    points: Vec<WorldPoint>,
    style: PencilStyle,
    limited: bool,
    preview: shape_preview::ShapeViews,
}
impl PencilDraft {
    fn append(&mut self, point: WorldPoint, zoom: f32, last: bool) {
        if !point.is_finite() || self.limited {
            return;
        }
        let previous = *self.points.last().expect("stroke begins with a point");
        let distance = (point.x - previous.x).hypot(point.y - previous.y) * zoom;
        if distance <= f32::EPSILON || (!last && distance < 1.5) {
            return;
        }
        if self.points.len() >= pencil::MAX_PENCIL_POINTS {
            self.limited = true;
        } else {
            self.points.push(point);
        }
    }
}
impl CreativeCanvas {
    pub(super) fn clear_pencil_tool(&mut self) {
        self.pencil_active = false;
        self.pencil_draft = None;
        self.pencil_fields = None;
    }
    fn pencil_world(&self, position: Point<Pixels>, cx: &App) -> WorldPoint {
        self.canvas.read(cx).editor.viewport.screen_to_world(
            WorldPoint::new(position.x.into(), position.y.into()) - self.canvas_origin,
        )
    }
    fn make_pencil_fields(
        &mut self,
        source: Option<Node>,
        style: PencilStyle,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let input = |value: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut s = InputState::new(window, cx).context_menu(false);
                s.set_value(value, window, cx);
                s
            })
        };
        self.pencil_fields = Some(PencilFields {
            color: input(format!("#{:06X}", style.color), window, cx),
            width: input(style.stroke_width.to_string(), window, cx),
            opacity: input((style.opacity * 100.).to_string(), window, cx),
            source,
        });
    }
    pub(super) fn sync_pencil_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pencil_active {
            return;
        }
        let source = self
            .selected(cx)
            .filter(|n| n.kind == pencil::PENCIL_NODE_KIND);
        let Some(node) = source else {
            self.pencil_fields = None;
            return;
        };
        if self
            .pencil_fields
            .as_ref()
            .is_some_and(|f| f.source.as_ref() == Some(&node))
        {
            return;
        }
        if let Ok(data) = PencilData::from_node(&node) {
            self.make_pencil_fields(Some(node), data.style, window, cx);
        } else {
            self.pencil_fields = None;
        }
    }
    fn pencil_style_from_fields(&self, cx: &App) -> Result<PencilStyle, String> {
        let f = self.pencil_fields.as_ref().ok_or("请重新选择笔迹")?;
        let color = f.color.read(cx).value();
        let hex = color.trim().trim_start_matches('#');
        if hex.len() != 6 {
            return Err("颜色应为6位十六进制".into());
        }
        let style = PencilStyle {
            color: u32::from_str_radix(hex, 16).map_err(|_| "请输入有效颜色")?,
            stroke_width: f
                .width
                .read(cx)
                .value()
                .trim()
                .parse()
                .map_err(|_| "请输入有效线宽")?,
            opacity: f
                .opacity
                .read(cx)
                .value()
                .trim()
                .parse::<f32>()
                .map_err(|_| "请输入有效不透明度")?
                / 100.,
        };
        style.validate()?;
        Ok(style)
    }
    pub(super) fn handle_pencil_action(
        &mut self,
        action: PencilAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, PencilAction::Done)
            || (matches!(action, PencilAction::Toggle) && self.pencil_active)
        {
            self.clear_pencil_tool();
            self.sync_pencil_fields(window, cx);
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
        if self.importing || !self.canvas.read(cx).is_editable() {
            return;
        }
        if matches!(action, PencilAction::Toggle) {
            self.clear_image_edit();
            self.clear_mask_edit();
            self.clear_asset_export();
            self.asset_picker = None;
            self.show_layers = false;
            self.show_processing_menu = false;
            self.pencil_active = true;
            self.pencil_draft = None;
            self.make_pencil_fields(None, self.pencil_style, window, cx);
            self.canvas.update(cx, |v, cx| v.set_pan_mode(false, cx));
            self.command(CanvasCommand::ClearSelection, cx);
            self.pencil_focus.focus(window, cx);
            cx.notify();
            return;
        }
        if let PencilAction::Color(color) = action
            && let Some(f) = &self.pencil_fields
        {
            f.color
                .update(cx, |f, cx| f.set_value(format!("#{color:06X}"), window, cx));
        }
        let result = self.pencil_style_from_fields(cx);
        match result {
            Ok(style) if self.pencil_active => {
                // A settings edit starts a new brush state, never an alpha
                // replacement on a still-running older stroke preview.
                self.pencil_draft = None;
                self.pencil_style = style;
                self.pencil_focus.focus(window, cx);
            }
            Ok(style) => {
                let Some(node) = self
                    .selected(cx)
                    .filter(|n| n.kind == pencil::PENCIL_NODE_KIND)
                else {
                    return;
                };
                if crate::layer_guard::node_is_locked(self.canvas.read(cx).editor.graph(), &node.id)
                {
                    return;
                }
                if self.pencil_fields.as_ref().and_then(|f| f.source.as_ref()) != Some(&node) {
                    self.notice = Some("笔迹已改变，请重新输入".into());
                } else {
                    let result = PencilData::from_node(&node).and_then(|mut data| {
                        data.style = style;
                        data.write_to_data(&node.data)
                    });
                    match result {
                        Ok(data) if data != node.data => {
                            self.command(
                                CanvasCommand::SetNodeData {
                                    node: node.id,
                                    data,
                                },
                                cx,
                            );
                        }
                        Ok(_) => {}
                        Err(error) => self.notice = Some(error),
                    }
                }
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            }
            Err(error) => self.notice = Some(error),
        }
        cx.notify();
    }
    fn finish_pencil(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut draft) = self.pencil_draft.take() else {
            return;
        };
        draft.append(
            self.pencil_world(position, cx),
            self.canvas.read(cx).editor.viewport.zoom,
            true,
        );
        if !self.pencil_active || self.importing || !self.canvas.read(cx).is_editable() {
            cx.notify();
            return;
        }
        if draft.limited {
            self.notice = Some("单条笔迹最多8192点，本次笔迹未添加；请分段绘制".into());
        } else {
            let id = self.canvas.read(cx).editor.graph().fresh_id("pencil");
            match pencil::create_pencil_node_from_points(id, "笔迹", &draft.points, draft.style) {
                Ok(node) => {
                    let id = node.id.clone();
                    if self.add_drawn_object(node, cx) {
                        self.command(
                            CanvasCommand::SelectNode {
                                node: id,
                                mode: SelectionMode::Replace,
                            },
                            cx,
                        );
                    }
                }
                Err(error) => self.notice = Some(error),
            }
        }
        self.pencil_focus.focus(window, cx);
        cx.notify();
    }
    pub(super) fn pencil_overlay(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if !self.pencil_active {
            return None;
        }
        let mut overlay = div()
            .id("pencil-drawing")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .track_focus(&self.pencil_focus)
            .cursor_crosshair();
        if let Some(draft) = &self.pencil_draft {
            overlay = overlay.child(draft.preview.render_draft(
                &draft.points,
                draft.style,
                self.canvas.read(cx).editor.viewport,
                cx,
            ));
        }
        Some(
            overlay
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        if this.importing || !this.canvas.read(cx).is_editable() {
                            return;
                        }
                        match this.pencil_style_from_fields(cx) {
                            Ok(style) => {
                                this.pencil_style = style;
                                this.pencil_focus.focus(window, cx);
                                this.pencil_draft = Some(PencilDraft {
                                    points: vec![this.pencil_world(event.position, cx)],
                                    style,
                                    limited: false,
                                    preview: Default::default(),
                                });
                            }
                            Err(error) => this.notice = Some(error),
                        }
                        cx.notify();
                    }),
                )
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if event.dragging() {
                        let point = this.pencil_world(event.position, cx);
                        let zoom = this.canvas.read(cx).editor.viewport.zoom;
                        if let Some(draft) = &mut this.pencil_draft {
                            draft.append(point, zoom, false);
                            cx.stop_propagation();
                            cx.notify();
                        }
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseUpEvent, window, cx| {
                        if this.pencil_draft.is_some() {
                            cx.stop_propagation();
                            this.finish_pencil(event.position, window, cx);
                        }
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseUpEvent, window, cx| {
                        if this.pencil_draft.is_some() {
                            cx.stop_propagation();
                            this.finish_pencil(event.position, window, cx);
                        }
                    }),
                )
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.handle_pencil_action(PencilAction::Done, window, cx);
                    } else if (event.keystroke.modifiers.control
                        || event.keystroke.modifiers.platform)
                        && matches!(event.keystroke.key.as_str(), "z" | "y")
                    {
                        cx.stop_propagation();
                        if this.pencil_draft.take().is_none() {
                            let redo =
                                event.keystroke.key == "y" || event.keystroke.modifiers.shift;
                            this.command(
                                if redo {
                                    CanvasCommand::Redo
                                } else {
                                    CanvasCommand::Undo
                                },
                                cx,
                            );
                        }
                        cx.notify();
                    }
                })),
        )
    }
    pub(super) fn pencil_panel(&self, editable: bool, cx: &mut Context<Self>) -> Div {
        let Some(f) = &self.pencil_fields else {
            return div();
        };
        let disabled = !editable || self.importing;
        let field = |title: &'static str, input: &Entity<InputState>, width: f32| {
            div()
                .flex()
                .items_center()
                .gap_1()
                .child(label(title))
                .child(
                    Input::new(input)
                        .w(px(width))
                        .h(px(28.))
                        .disabled(disabled)
                        .aria_label(title),
                )
        };
        let mut colors = div().flex().items_center().gap_1();
        for color in [
            0x33343c, 0x3b82f6, 0x2dbba6, 0xf4b43a, 0x9b83ee, 0xe56b88, 0xffffff,
        ] {
            colors = colors.child(
                self.button(
                    SharedString::from(format!("pencil-color-{color}")),
                    " ",
                    Action::Pencil(PencilAction::Color(color)),
                    disabled,
                    cx,
                )
                .w(px(22.))
                .h(px(22.))
                .bg(rgb(color))
                .border_1()
                .border_color(rgb(0xdadce2)),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(label(if self.pencil_active {
                "自由绘制 · 半透明草稿渐进预览 · Esc 完成"
            } else {
                "笔迹"
            }))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(field("颜色", &f.color, 92.))
                    .child(field("线宽", &f.width, 56.))
                    .child(field("不透明%", &f.opacity, 50.)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(colors)
                    .child(self.button(
                        "apply-pencil",
                        "应用",
                        Action::Pencil(PencilAction::Apply),
                        disabled,
                        cx,
                    ))
                    .when(self.pencil_active, |row| {
                        row.child(self.button(
                            "finish-pencil",
                            "完成",
                            Action::Pencil(PencilAction::Done),
                            false,
                            cx,
                        ))
                    }),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn sampling_retains_final_point_and_never_exceeds_budget() {
        let mut draft = PencilDraft {
            points: vec![WorldPoint::ZERO],
            style: PencilStyle::default(),
            limited: false,
            preview: Default::default(),
        };
        draft.append(WorldPoint::new(0.5, 0.), 1., false);
        assert_eq!(draft.points.len(), 1);
        draft.append(WorldPoint::new(0.5, 0.), 1., true);
        assert_eq!(draft.points.len(), 2);
        draft.points = vec![WorldPoint::ZERO; pencil::MAX_PENCIL_POINTS];
        draft.append(WorldPoint::new(10., 0.), 1., true);
        assert!(draft.limited);
        assert_eq!(draft.points.len(), pencil::MAX_PENCIL_POINTS);
    }
}
