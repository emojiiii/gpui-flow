use super::*;
use crate::mask::{MaskMode, MaskPoint, MaskSource, MaskStroke};
use gpui_flow_core::Rect;

#[derive(Clone, Copy)]
pub(super) enum MaskAction {
    Begin,
    Finish,
    Mode(MaskMode),
    Radius(f32),
    Reset,
    Apply,
    FromImage,
    EditImage,
    CancelApply,
}
pub(super) struct MaskEditing {
    pub node: NodeId,
    mode: MaskMode,
    radius: f32,
    stroke: Option<MaskStroke>,
    source: MaskSource,
}
fn content_rect(bounds: Rect, source: &MaskSource) -> Rect {
    let scale =
        (bounds.size.width / source.width as f32).min(bounds.size.height / source.height as f32);
    let size = WorldSize::new(source.width as f32 * scale, source.height as f32 * scale);
    Rect::new(
        WorldPoint::new(
            bounds.center().x - size.width / 2.,
            bounds.center().y - size.height / 2.,
        ),
        size,
    )
}
fn brush_point(position: WorldPoint, bounds: Rect) -> Option<MaskPoint> {
    if bounds.size.width <= 0. || bounds.size.height <= 0. {
        return None;
    }
    MaskPoint::from_normalized(
        (position.x - bounds.origin.x) / bounds.size.width,
        (position.y - bounds.origin.y) / bounds.size.height,
    )
    .ok()
}

impl CreativeCanvas {
    pub(super) fn clear_mask_edit(&mut self) {
        self.mask_editing = None;
        self.mask_views.set_draft(None);
    }
    pub(super) fn reconcile_mask_edit(&mut self, cx: &App) {
        let valid = self.mask_editing.as_ref().is_none_or(|edit| {
            self.canvas
                .read(cx)
                .editor
                .graph()
                .node(&edit.node)
                .and_then(|node| MaskNodeData::from_value(&node.data).ok())
                .is_some_and(|data| data.state.source_matches(&edit.source))
                && self
                    .mask_views
                    .context(&edit.node)
                    .is_ok_and(|c| c.source == edit.source)
        });
        if !valid {
            self.clear_mask_edit();
        }
    }
    pub(super) fn handle_mask_action(
        &mut self,
        action: MaskAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match action {
            MaskAction::FromImage => {
                self.create_mask_from_image(cx);
                return;
            }
            MaskAction::EditImage => {
                self.create_masked_image_generator(cx);
                return;
            }
            MaskAction::Apply => {
                self.finish_mask_stroke(cx);
                self.apply_mask(cx);
                return;
            }
            MaskAction::CancelApply => {
                self.cancel_mask_apply(cx);
                return;
            }
            MaskAction::Finish => {
                self.finish_mask_stroke(cx);
                self.clear_mask_edit();
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                cx.notify();
                return;
            }
            MaskAction::Mode(mode) => {
                if let Some(edit) = &mut self.mask_editing {
                    edit.mode = mode;
                }
                cx.notify();
                return;
            }
            MaskAction::Radius(factor) => {
                if let Some(edit) = &mut self.mask_editing {
                    edit.radius = (edit.radius * factor).clamp(0.002, 0.25);
                }
                cx.notify();
                return;
            }
            MaskAction::Begin | MaskAction::Reset => {}
        }
        if self.saving || self.importing || !self.canvas.read(cx).is_editable() {
            return;
        }
        let Some(node) = self
            .selected(cx)
            .filter(|node| node.kind == "mask" && !node.locked)
        else {
            return;
        };
        let result = (|| {
            let context = self.mask_views.context(&node.id).map_err(CreativeError)?;
            let mut data = MaskNodeData::from_value(&node.data)?;
            if matches!(action, MaskAction::Reset) || data.state.source.is_none() {
                data.reset_source(context.source.clone())?;
                if !self.command(
                    CanvasCommand::SetNodeData {
                        node: node.id.clone(),
                        data: serde_json::to_value(&data)?,
                    },
                    cx,
                ) {
                    return Ok(());
                }
            } else if !data.state.source_matches(&context.source) {
                return Err(CreativeError(
                    "输入原图已变更；请明确重置遮罩后绘制，旧笔画仍保留".into(),
                ));
            }
            self.mask_editing = Some(MaskEditing {
                node: node.id,
                mode: MaskMode::Paint,
                radius: 0.025,
                stroke: None,
                source: context.source,
            });
            self.mask_focus.focus(window, cx);
            Ok::<_, CreativeError>(())
        })();
        if let Err(error) = result {
            self.notice = Some(error.to_string());
        }
        cx.notify();
    }
    fn mask_pointer(
        &mut self,
        bounds: Rect,
        position: Point<Pixels>,
        start: bool,
        cx: &mut Context<Self>,
    ) {
        if !self.canvas.read(cx).is_editable() {
            return;
        }
        let position = WorldPoint::new(
            f32::from(position.x) - self.canvas_origin.x,
            f32::from(position.y) - self.canvas_origin.y,
        );
        let Some(point) = brush_point(position, bounds) else {
            return;
        };
        let Some(edit) = &mut self.mask_editing else {
            return;
        };
        let Some(node) = self
            .canvas
            .read(cx)
            .editor
            .graph()
            .node(&edit.node)
            .cloned()
        else {
            return;
        };
        let Ok(mut data) = MaskNodeData::from_value(&node.data) else {
            return;
        };
        if !data.state.source_matches(&edit.source)
            || !self
                .mask_views
                .context(&edit.node)
                .is_ok_and(|context| context.source == edit.source)
        {
            return;
        }
        if start {
            edit.stroke = Some(MaskStroke {
                mode: edit.mode,
                radius: edit.radius,
                points: vec![point],
            });
        } else if let Some(stroke) = &mut edit.stroke {
            if let Some(previous) = stroke.points.last() {
                let (x, y) = point.normalized();
                let (px, py) = previous.normalized();
                if ((x - px) * bounds.size.width).hypot((y - py) * bounds.size.height) < 2. {
                    return;
                }
            }
            let used = data
                .state
                .strokes
                .iter()
                .map(|s| s.points.len())
                .sum::<usize>();
            if used + stroke.points.len() >= crate::mask::MAX_MASK_POINTS {
                self.notice = Some("遮罩笔画点数已达上限，请撤销部分笔画或重置".into());
                return;
            }
            stroke.points.push(point);
        }
        if let Some(stroke) = &edit.stroke {
            match data.state.push_stroke(stroke.clone()) {
                Ok(()) => self
                    .mask_views
                    .set_draft(Some((edit.node.clone(), data.state))),
                Err(error) => self.notice = Some(error.to_string()),
            }
        }
        cx.notify();
    }
    pub(super) fn finish_mask_stroke(&mut self, cx: &mut Context<Self>) {
        let Some(edit) = &mut self.mask_editing else {
            return;
        };
        let Some(stroke) = edit.stroke.take() else {
            return;
        };
        let id = edit.node.clone();
        let source = edit.source.clone();
        self.mask_views.set_draft(None);
        let result = (|| {
            let node = self
                .canvas
                .read(cx)
                .editor
                .graph()
                .node(&id)
                .cloned()
                .ok_or_else(|| CreativeError("遮罩节点已删除".into()))?;
            let current = self.mask_views.context(&id).map_err(CreativeError)?;
            let mut data = MaskNodeData::from_value(&node.data)?;
            if current.source != source || !data.state.source_matches(&source) {
                return Err(CreativeError("原图已改变，未完成笔画已取消".into()));
            }
            data.push_stroke(stroke)?;
            self.command(
                CanvasCommand::SetNodeData {
                    node: id,
                    data: serde_json::to_value(data)?,
                },
                cx,
            );
            Ok::<_, CreativeError>(())
        })();
        if let Err(error) = result {
            self.notice = Some(error.to_string());
        }
        cx.notify();
    }
    pub(super) fn mask_editor_overlay(
        &self,
        node: &Node,
        bounds: Rect,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        let edit = self
            .mask_editing
            .as_ref()
            .filter(|edit| edit.node == node.id)?;
        let context = self.mask_views.context(&node.id).ok()?;
        if context.source != edit.source {
            return None;
        }
        let content = content_rect(bounds, &context.source);
        Some(
            div()
                .id("mask-paint-surface")
                .occlude()
                .track_focus(&self.mask_focus)
                .absolute()
                .left(px(content.origin.x))
                .top(px(content.origin.y))
                .w(px(content.size.width))
                .h(px(content.size.height))
                .cursor_crosshair()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        window.prevent_default();
                        cx.stop_propagation();
                        this.mask_focus.focus(window, cx);
                        this.mask_pointer(content, event.position, true, cx);
                    }),
                )
                .on_mouse_move(cx.listener(move |this, event: &MouseMoveEvent, _, cx| {
                    if event.dragging() {
                        cx.stop_propagation();
                        this.mask_pointer(content, event.position, false, cx);
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.finish_mask_stroke(cx);
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, _, _, cx| {
                        this.finish_mask_stroke(cx);
                    }),
                )
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    cx.stop_propagation();
                    if event.keystroke.key == "escape" {
                        this.clear_mask_edit();
                        this.canvas.read(cx).focus_handle(cx).focus(window, cx);
                        cx.notify();
                    } else if event.keystroke.key == "z"
                        && (event.keystroke.modifiers.control || event.keystroke.modifiers.platform)
                    {
                        if let Some(edit) = &mut this.mask_editing {
                            edit.stroke = None;
                        }
                        this.mask_views.set_draft(None);
                        this.action(
                            if event.keystroke.modifiers.shift {
                                Action::Redo
                            } else {
                                Action::Undo
                            },
                            window,
                            cx,
                        );
                    }
                })),
        )
    }
    pub(super) fn mask_controls(&self, node: &Node, editable: bool, cx: &mut Context<Self>) -> Div {
        let context = self.mask_views.context(&node.id);
        let data = MaskNodeData::from_value(&node.data).ok();
        let stale = context
            .as_ref()
            .ok()
            .zip(data.as_ref())
            .is_some_and(|(c, d)| d.state.source.is_some() && !d.state.source_matches(&c.source));
        let active = self
            .mask_editing
            .as_ref()
            .filter(|edit| edit.node == node.id);
        let ready = context
            .as_ref()
            .ok()
            .zip(data.as_ref())
            .is_some_and(|(c, d)| d.output_is_current(&c.source));
        let disabled = !editable || self.saving || self.importing || context.is_err();
        let status = match &context {
            Err(e) => e.clone(),
            Ok(_) => if stale {
                "原图已变更，旧笔画保留；请重置"
            } else if ready {
                "Mask 已应用 · 0 保留 / 255 编辑"
            } else {
                "画笔标记编辑区域 · 原图保持不变"
            }
            .into(),
        };
        let mut body = div().flex().flex_col().gap_2().child(
            div()
                .text_size(px(11.))
                .text_color(rgb(0x8f7b91))
                .child(status),
        );
        let mut tools = div().flex().items_center().gap_1();
        if let Some(edit) = active {
            for (id, label, mode) in [
                ("mask-paint", "画笔", MaskMode::Paint),
                ("mask-erase", "橡皮", MaskMode::Erase),
            ] {
                tools = tools.child(
                    self.button(
                        id,
                        label,
                        Action::Mask(MaskAction::Mode(mode)),
                        disabled,
                        cx,
                    )
                    .when(edit.mode == mode, |b| b.bg(rgb(0xf1e5ef))),
                );
            }
            tools = tools
                .child(self.button(
                    "mask-smaller",
                    "−",
                    Action::Mask(MaskAction::Radius(0.75)),
                    disabled,
                    cx,
                ))
                .child(label(format!("{:.1}%", edit.radius * 200.)))
                .child(self.button(
                    "mask-larger",
                    "+",
                    Action::Mask(MaskAction::Radius(1.333333)),
                    disabled,
                    cx,
                ))
                .child(self.button(
                    "mask-done",
                    "完成",
                    Action::Mask(MaskAction::Finish),
                    self.importing,
                    cx,
                ));
        } else {
            tools = tools.child(self.button(
                "mask-begin",
                "绘制遮罩",
                Action::Mask(MaskAction::Begin),
                disabled || stale,
                cx,
            ));
        }
        tools = tools.child(self.button(
            "mask-reset",
            if stale {
                "用新原图重置"
            } else {
                "重置"
            },
            Action::Mask(MaskAction::Reset),
            disabled,
            cx,
        ));
        body = body.child(tools);
        body.child(
            div()
                .flex()
                .items_center()
                .gap_2()
                .child(self.button(
                    "mask-apply",
                    if self.importing && self.mask_cancel.is_some() {
                        "正在应用…"
                    } else {
                        "应用遮罩"
                    },
                    Action::Mask(MaskAction::Apply),
                    disabled || stale || data.as_ref().is_none_or(|d| d.state.source.is_none()),
                    cx,
                ))
                .when(self.mask_cancel.is_some(), |row| {
                    row.child(self.button(
                        "mask-cancel",
                        "取消",
                        Action::Mask(MaskAction::CancelApply),
                        false,
                        cx,
                    ))
                })
                .child(self.button(
                    "mask-edit-image",
                    "用于图片编辑",
                    Action::Mask(MaskAction::EditImage),
                    disabled || !ready,
                    cx,
                )),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;
    #[test]
    fn letterboxed_image_pointer_mapping_does_not_paint_the_padding() {
        let source = MaskSource::new("assets/image.png", 100, 200).unwrap();
        let bounds = content_rect(
            Rect::new(WorldPoint::new(200., 100.), WorldSize::new(400., 200.)),
            &source,
        );
        assert_eq!(bounds.origin, WorldPoint::new(350., 100.));
        assert!(brush_point(WorldPoint::new(250., 150.), bounds).is_none());
        let (x, y) = brush_point(bounds.center(), bounds).unwrap().normalized();
        assert!((x - 0.5).abs() < 0.0001 && (y - 0.5).abs() < 0.0001);
    }
}
