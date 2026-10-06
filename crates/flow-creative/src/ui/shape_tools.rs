use super::*;
use crate::shapes::{self, ShapeData, ShapeKind};
use gpui_flow_core::Rect;
use gpui_kit::component::input::{Input, InputState};

#[derive(Clone)]
pub(super) enum ShapeAction {
    Menu,
    Draw(ShapeKind),
    Cancel,
    Apply,
    Fill(Option<u32>),
    Stroke(Option<u32>),
}
pub(super) struct ShapeFields {
    source: Node,
    fill: Entity<InputState>,
    stroke: Entity<InputState>,
    weight: Entity<InputState>,
    radius: Entity<InputState>,
    opacity: Entity<InputState>,
    width: Entity<InputState>,
    height: Entity<InputState>,
}
pub(super) struct ShapeDrag {
    start: WorldPoint,
    end: WorldPoint,
    source: Option<Node>,
}
fn kind_title(kind: ShapeKind) -> &'static str {
    match kind {
        ShapeKind::Rectangle => "矩形",
        ShapeKind::Ellipse => "椭圆",
        ShapeKind::Line => "线条",
    }
}
fn parse_color(value: &str) -> Result<Option<u32>, String> {
    let value = value.trim().trim_start_matches('#');
    if value.is_empty() {
        return Ok(None);
    }
    if value.len() != 6 {
        return Err("颜色应为6位十六进制，留空表示透明".into());
    }
    u32::from_str_radix(value, 16)
        .map(Some)
        .map_err(|_| "请输入有效的十六进制颜色".into())
}
fn dimension_text(value: f32) -> String {
    format!("{value:.2}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}
fn parse_dimension(value: &str, original: f32) -> Result<f32, String> {
    let value = value.trim();
    // A style edit must not round geometry merely because the field is concise.
    if value == dimension_text(original) {
        Ok(original)
    } else {
        value.parse().map_err(|_| "请输入有效数字".to_string())
    }
}
fn field(label_text: &'static str, value: &Entity<InputState>, width: f32, disabled: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap_1()
        .child(label(label_text))
        .child(
            Input::new(value)
                .w(px(width))
                .h(px(28.))
                .disabled(disabled)
                .aria_label(label_text),
        )
}
impl CreativeCanvas {
    pub(super) fn clear_shape_tool(&mut self) {
        self.shape_tool = None;
        self.shape_drag = None;
        self.show_shapes = false;
    }
    pub(super) fn sync_shape_fields(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected(cx);
        if let Some(drag) = &self.shape_drag
            && let Some(source) = &drag.source
            && selected.as_ref() != Some(source)
        {
            self.shape_drag = None;
        }
        let Some(node) = selected.filter(|n| n.kind == shapes::SHAPE_NODE_KIND) else {
            self.shape_fields = None;
            return;
        };
        if self.shape_fields.as_ref().is_some_and(|f| f.source == node) {
            return;
        }
        let Ok(shape) = ShapeData::from_node(&node) else {
            self.shape_fields = None;
            return;
        };
        let input = |value: String, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                let mut s = InputState::new(window, cx).context_menu(false);
                s.set_value(value, window, cx);
                s
            })
        };
        let color = |v: Option<u32>| v.map(|v| format!("#{v:06X}")).unwrap_or_default();
        self.shape_fields = Some(ShapeFields {
            fill: input(color(shape.style.fill), window, cx),
            stroke: input(color(shape.style.stroke), window, cx),
            weight: input(shape.style.stroke_width.to_string(), window, cx),
            radius: input(shape.style.radius.to_string(), window, cx),
            opacity: input((shape.style.opacity * 100.).to_string(), window, cx),
            width: input(dimension_text(node.size.width), window, cx),
            height: input(dimension_text(node.size.height), window, cx),
            source: node,
        });
    }
    pub(super) fn handle_shape_action(
        &mut self,
        action: ShapeAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, ShapeAction::Cancel) {
            self.clear_shape_tool();
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            return;
        }
        if matches!(action, ShapeAction::Menu) {
            self.show_shapes = !self.show_shapes;
            self.shape_tool = None;
            self.shape_drag = None;
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
        if let ShapeAction::Draw(kind) = action {
            self.clear_image_edit();
            self.clear_mask_edit();
            self.clear_asset_export();
            self.clear_frame_tool();
            self.asset_picker = None;
            self.show_processing_menu = false;
            self.show_layers = false;
            self.show_shapes = false;
            self.shape_tool = Some(kind);
            self.shape_drag = None;
            self.canvas.update(cx, |v, cx| v.set_pan_mode(false, cx));
            self.command(CanvasCommand::ClearSelection, cx);
            self.shape_focus.focus(window, cx);
            cx.notify();
            return;
        }
        let Some(node) = self
            .selected(cx)
            .filter(|n| n.kind == shapes::SHAPE_NODE_KIND)
        else {
            return;
        };
        if crate::layer_guard::node_is_locked(self.canvas.read(cx).editor.graph(), &node.id) {
            return;
        }
        let result: Result<Node, String> = (|| {
            let mut candidate = node.clone();
            let mut data = ShapeData::from_node(&node)?;
            match action {
                ShapeAction::Fill(color) => data.style.fill = color,
                ShapeAction::Stroke(color) => data.style.stroke = color,
                ShapeAction::Apply => {
                    let f = self.shape_fields.as_ref().ok_or("请重新选择图形")?;
                    if f.source != node {
                        return Err("图形已改变，请重新输入".into());
                    }
                    let number = |input: &Entity<InputState>| {
                        input
                            .read(cx)
                            .value()
                            .trim()
                            .parse::<f32>()
                            .map_err(|_| "请输入有效数字".to_string())
                    };
                    data.style.fill = parse_color(&f.fill.read(cx).value())?;
                    data.style.stroke = parse_color(&f.stroke.read(cx).value())?;
                    data.style.stroke_width = number(&f.weight)?;
                    data.style.radius = number(&f.radius)?;
                    data.style.opacity = number(&f.opacity)? / 100.;
                    candidate.size = WorldSize::new(
                        parse_dimension(&f.width.read(cx).value(), node.size.width)?,
                        parse_dimension(&f.height.read(cx).value(), node.size.height)?,
                    );
                }
                _ => {}
            }
            data.write_to_node(&mut candidate)?;
            shapes::validate_shape_node(&candidate)?;
            Ok(candidate)
        })();
        match result {
            Ok(candidate) => {
                let mut graph = self.canvas.read(cx).editor.graph().clone();
                if candidate != node {
                    graph.nodes.insert(node.id, candidate);
                    self.command(CanvasCommand::ImportGraph(graph), cx);
                }
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            }
            Err(e) => self.notice = Some(e),
        }
        cx.notify();
    }
    fn shape_world_point(&self, point: Point<Pixels>, cx: &App) -> WorldPoint {
        self.canvas.read(cx).editor.viewport.screen_to_world(
            WorldPoint::new(f32::from(point.x), f32::from(point.y)) - self.canvas_origin,
        )
    }
    pub(super) fn finish_shape_drag(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(mut drag) = self.shape_drag.take() else {
            return;
        };
        drag.end = self.shape_world_point(position, cx);
        let kind = self.shape_tool.take();
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            cx.notify();
            return;
        }
        if let Some(source) = drag.source {
            if self.canvas.read(cx).editor.graph().node(&source.id) != Some(&source) {
                self.notice = Some("图形已改变，旧拖动已取消".into());
                cx.notify();
                return;
            }
            let delta = drag.end - drag.start;
            self.command(
                CanvasCommand::ResizeNode {
                    node: source.id,
                    size: WorldSize::new(
                        (source.size.width + delta.x).clamp(1., 16384.),
                        (source.size.height + delta.y).clamp(1., 16384.),
                    ),
                },
                cx,
            );
        } else if let Some(kind) = kind {
            let id = self.canvas.read(cx).editor.graph().fresh_id("shape");
            let zoom = self.canvas.read(cx).editor.viewport.zoom;
            let result = if (drag.end.x - drag.start.x).hypot(drag.end.y - drag.start.y) * zoom < 4.
            {
                let size = kind.default_size();
                let position = drag.start - WorldPoint::new(size.width / 2., size.height / 2.);
                shapes::create_shape_node(
                    id,
                    kind_title(kind),
                    position,
                    size,
                    ShapeData::new(kind),
                )
            } else {
                shapes::create_shape_node_from_points(
                    id,
                    kind_title(kind),
                    kind,
                    drag.start,
                    drag.end,
                )
            };
            match result {
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
                Err(e) => self.notice = Some(e),
            }
        }
        self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    pub(super) fn shape_draw_overlay(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        if self.shape_tool.is_none() && self.shape_drag.is_none() {
            return None;
        }
        let view = self.canvas.read(cx);
        let mut overlay = div()
            .id("shape-drawing")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .occlude()
            .track_focus(&self.shape_focus)
            .cursor_crosshair();
        if let Some(drag) = &self.shape_drag {
            let bounds = if let Some(node) = &drag.source {
                let delta = drag.end - drag.start;
                Rect::new(
                    node.position,
                    WorldSize::new(
                        (node.size.width + delta.x).clamp(1., 16384.),
                        (node.size.height + delta.y).clamp(1., 16384.),
                    ),
                )
            } else {
                Rect::from_points(drag.start, drag.end)
            };
            let r = view.editor.viewport.world_rect_to_screen(bounds);
            overlay = overlay.child(
                div()
                    .absolute()
                    .left(px(r.origin.x))
                    .top(px(r.origin.y))
                    .w(px(r.size.width.max(1.)))
                    .h(px(r.size.height.max(1.)))
                    .border_1()
                    .border_color(rgb(0x4785e7))
                    .bg(rgba(0x4785e710)),
            );
        }
        overlay = overlay.child(
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
                .child("拖动调整对象 · Esc 取消"),
        );
        Some(
            overlay
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        this.shape_focus.focus(window, cx);
                        if this.shape_drag.is_none() {
                            let start = this.shape_world_point(event.position, cx);
                            this.shape_drag = Some(ShapeDrag {
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
                        let end = this.shape_world_point(event.position, cx);
                        if let Some(drag) = &mut this.shape_drag {
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
                        this.finish_shape_drag(event.position, window, cx);
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseUpEvent, window, cx| {
                        cx.stop_propagation();
                        this.finish_shape_drag(event.position, window, cx);
                    }),
                )
                .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
                .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                    if event.keystroke.key == "escape" {
                        cx.stop_propagation();
                        this.handle_shape_action(ShapeAction::Cancel, window, cx);
                    }
                })),
        )
    }
    pub(super) fn shape_resize_handle(
        &self,
        node: &Node,
        bounds: Rect,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if !editable
            || self.importing
            || self.saving
            || self.shape_drag.is_some()
            || self.shape_tool.is_some()
        {
            return None;
        }
        let id = node.id.clone();
        Some(
            div()
                .id("shape-resize-handle")
                .absolute()
                .left(px(bounds.origin.x + bounds.size.width - 24.))
                .top(px(bounds.origin.y + bounds.size.height - 24.))
                .size(px(28.))
                .occlude()
                .cursor_crosshair()
                .child(
                    div()
                        .absolute()
                        .right(px(3.))
                        .bottom(px(3.))
                        .size(px(7.))
                        .rounded(px(2.))
                        .bg(rgb(0xffffff))
                        .border_1()
                        .border_color(rgb(0x4785e7)),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        let Some(source) = this.canvas.read(cx).editor.graph().node(&id).cloned()
                        else {
                            return;
                        };
                        let start = this.shape_world_point(event.position, cx);
                        this.shape_drag = Some(ShapeDrag {
                            start,
                            end: start,
                            source: Some(source),
                        });
                        this.shape_focus.focus(window, cx);
                        cx.notify();
                    }),
                ),
        )
    }
    pub(super) fn shape_menu(&self, cx: &mut Context<Self>) -> Div {
        let mut panel = div()
            .absolute()
            .left(px(78.))
            .top(px(400.))
            .w(px(176.))
            .p_2()
            .rounded_xl()
            .bg(rgb(0xffffff))
            .shadow_md()
            .occlude()
            .flex()
            .flex_col()
            .gap_1();
        for (kind, id) in [
            (ShapeKind::Rectangle, "draw-rectangle"),
            (ShapeKind::Ellipse, "draw-ellipse"),
            (ShapeKind::Line, "draw-line"),
        ] {
            panel = panel.child(self.button(
                id,
                kind_title(kind),
                Action::Shape(ShapeAction::Draw(kind)),
                false,
                cx,
            ));
        }
        panel
    }
    pub(super) fn shape_style_panel(
        &self,
        node: &Node,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(f) = self
            .shape_fields
            .as_ref()
            .filter(|f| f.source.id == node.id)
        else {
            return div();
        };
        let Ok(shape) = ShapeData::from_node(node) else {
            return div();
        };
        let disabled = !editable || self.saving;
        let line = shape.kind == ShapeKind::Line;
        let mut swatches = div().flex().items_center().gap_1();
        for color in [
            0x3B82F6, 0x35BFA4, 0xF2B24E, 0xA78BFA, 0xED748D, 0x20242C, 0xFFFFFF,
        ] {
            swatches = swatches.child(
                self.button(
                    SharedString::from(format!("shape-color-{color}")),
                    "",
                    Action::Shape(if line {
                        ShapeAction::Stroke(Some(color))
                    } else {
                        ShapeAction::Fill(Some(color))
                    }),
                    disabled,
                    cx,
                )
                .w(px(22.))
                .h(px(22.))
                .bg(rgb(color))
                .border_1()
                .border_color(rgb(0xcfd3dc)),
            );
        }
        swatches = swatches.child(self.button(
            "shape-transparent",
            if line { "无描边" } else { "无填充" },
            Action::Shape(if line {
                ShapeAction::Stroke(None)
            } else {
                ShapeAction::Fill(None)
            }),
            disabled,
            cx,
        ));
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(label(kind_title(shape.kind)))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(field("填充", &f.fill, 118., disabled || line))
                    .child(field("描边", &f.stroke, 118., disabled)),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(field("线宽", &f.weight, 55., disabled))
                    .child(field(
                        "圆角",
                        &f.radius,
                        55.,
                        disabled || shape.kind != ShapeKind::Rectangle,
                    ))
                    .child(field("不透明%", &f.opacity, 55., disabled)),
            )
            .child(swatches)
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(field("W", &f.width, 90., disabled))
                    .child(field("H", &f.height, 90., disabled))
                    .child(self.button(
                        "apply-shape-style",
                        "应用",
                        Action::Shape(ShapeAction::Apply),
                        disabled,
                        cx,
                    )),
            )
            .child(label("颜色填 #RRGGBB，留空为透明"))
    }
}

#[cfg(test)]
mod tests {
    use super::{dimension_text, parse_dimension};

    #[test]
    fn concise_dimensions_preserve_unedited_geometry() {
        let original = 300.00458;
        assert_eq!(dimension_text(original), "300");
        assert_eq!(dimension_text(70.015), "70.01");
        assert_eq!(parse_dimension("300", original).unwrap(), original);
        assert_eq!(parse_dimension("320", original).unwrap(), 320.);
        assert!(parse_dimension("invalid", original).is_err());
    }
}
