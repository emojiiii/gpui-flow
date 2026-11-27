use super::*;

pub(super) fn weight(data: &Value) -> FontWeight {
    FontWeight(f32::from(
        crate::text_weight::read_weight(data).unwrap_or(400),
    ))
}

pub(super) fn alignment(value: TextAlignment) -> gpui::TextAlign {
    match value {
        TextAlignment::Left => gpui::TextAlign::Left,
        TextAlignment::Center => gpui::TextAlign::Center,
        TextAlignment::Right => gpui::TextAlign::Right,
    }
}

impl CreativeCanvas {
    pub(super) fn set_text_weight(
        &mut self,
        weight: u16,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            return;
        }
        let Some(node) = self.selected(cx).filter(|node| {
            node.kind == "text"
                && !crate::layer_guard::node_is_locked(
                    self.canvas.read(cx).editor.graph(),
                    &node.id,
                )
        }) else {
            return;
        };
        match crate::text_weight::write_weight(&node.data, weight) {
            Ok(data) if data != node.data => {
                if self.command(
                    CanvasCommand::SetNodeData {
                        node: node.id,
                        data,
                    },
                    cx,
                ) {
                    self.form_source = self.selected(cx).as_ref().map(form_identity);
                    self.sync_form(window, cx);
                }
            }
            Ok(_) => {}
            Err(error) => self.notice = Some(error),
        }
        self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }

    pub(super) fn is_editing_text(&self, node: &NodeId) -> bool {
        self.editing_text
            .lock()
            .is_ok_and(|id| id.as_ref() == Some(node))
    }
    pub(super) fn clear_text_edit(&self) {
        if let Ok(mut id) = self.editing_text.lock() {
            *id = None;
        }
    }
    pub(super) fn begin_text_edit(
        &mut self,
        node: NodeId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let canvas = self.canvas.read(cx);
        if !canvas.is_editable()
            || canvas.interactions.has_capture()
            || canvas
                .editor
                .graph()
                .node(&node)
                .is_none_or(|n| n.kind != "text" || n.locked)
        {
            return;
        }
        self.apply_form_edit(cx);
        if !self.command(
            CanvasCommand::SelectNode {
                node: node.clone(),
                mode: SelectionMode::Replace,
            },
            cx,
        ) {
            return;
        }
        self.sync_form(window, cx);
        if let Ok(mut id) = self.editing_text.lock() {
            *id = Some(node);
        }
        self.form.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    pub(super) fn edit_text_style(
        &mut self,
        edit: impl FnOnce(&mut TextStyle),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(node) = self.selected(cx).filter(|n| n.kind == "text") else {
            return;
        };
        match crate::edit_text_style(&node.data, edit) {
            Ok(data) => {
                if self.command(
                    CanvasCommand::SetNodeData {
                        node: node.id,
                        data,
                    },
                    cx,
                ) {
                    // Style changes must not replace the live draft or move its caret.
                    self.form_source = self.selected(cx).as_ref().map(form_identity);
                    self.sync_form(window, cx);
                }
            }
            Err(error) => {
                self.notice = Some(error);
                cx.notify();
            }
        }
    }
    pub(super) fn text_editor(
        &self,
        node: &Node,
        bounds: gpui_flow_core::Rect,
        zoom: f32,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let style = TextStyle::from_data(&node.data).unwrap_or_default();
        let appearance =
            crate::text_appearance::TextAppearance::from_data(&node.data).unwrap_or_default();
        // The native textarea uses fixed 10px/8px insets. Compensate here so
        // its glyph origin matches the scaled canvas object at every zoom.
        div()
            .id("canvas-text-editor")
            .occlude()
            .absolute()
            .left(px(bounds.origin.x + 10. * zoom - 10. + 1.))
            .top(px(bounds.origin.y + 8. * zoom - 8. + 1.))
            .w(px((bounds.size.width - 20. * zoom + 20. - 2.).max(24.)))
            .h(px((bounds.size.height - 16. * zoom + 16. - 2.).max(24.)))
            .cursor_text()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if event.keystroke.key == "escape" {
                    this.action(Action::FinishText, window, cx);
                    cx.stop_propagation();
                }
            }))
            .child(
                Textarea::new(&self.form)
                    .h(px((bounds.size.height - 16. * zoom + 16. - 2.).max(24.)))
                    .bordered(false)
                    .appearance(false)
                    .text_size(px(style.font_size * zoom))
                    .font_weight(weight(&node.data))
                    .text_align(alignment(style.alignment))
                    .line_height(relative(1.25))
                    .text_color(rgb(appearance.color))
                    .when_some(
                        appearance.installed_family(&self.font_families),
                        |v, family| v.font_family(SharedString::from(family.to_string())),
                    )
                    .aria_label(tr(
                        "画布文字，按 Escape 完成编辑",
                        "Canvas text. Press Escape to finish editing.",
                    )),
            )
    }
    pub(super) fn text_style_toolbar(
        &self,
        node: &Node,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let style = TextStyle::from_data(&node.data).unwrap_or_default();
        let mut row = div()
            .flex()
            .items_center()
            .gap_1()
            .child(
                self.button(
                    "text-appearance",
                    tr("字体", "Font"),
                    Action::TextAppearance(text_appearance_tools::TypographyAction::Toggle),
                    !editable,
                    cx,
                )
                .when(self.typography.is_some(), |v| v.bg(rgb(0xeff4ff))),
            )
            .child(self.icon_button(
                "text-smaller",
                "minus",
                tr("减小字号", "Decrease font size"),
                Action::TextSize(-2.),
                !editable || style.font_size <= 12.,
                cx,
            ))
            .child(
                div()
                    .min_w(px(26.))
                    .text_size(px(12.))
                    .text_center()
                    .child(format!("{:.0}", style.font_size)),
            )
            .child(self.icon_button(
                "text-larger",
                "plus",
                tr("增大字号", "Increase font size"),
                Action::TextSize(2.),
                !editable || style.font_size >= 144.,
                cx,
            ))
            .child(chrome::separator())
            .child(
                self.icon_button(
                    "text-bold",
                    "bold",
                    tr("粗体", "Bold"),
                    Action::TextBold,
                    !editable,
                    cx,
                )
                .when(
                    crate::text_weight::read_weight(&node.data).unwrap_or(400) >= 600,
                    |button| button.bg(rgb(0xeaf0fd)).text_color(rgb(0x467bc6)),
                ),
            );
        for (id, icon, title, align) in [
            (
                "text-left",
                "text-align-start",
                tr("左对齐", "Align left"),
                TextAlignment::Left,
            ),
            (
                "text-center",
                "text-align-center",
                tr("居中", "Center"),
                TextAlignment::Center,
            ),
            (
                "text-right",
                "text-align-end",
                tr("右对齐", "Align right"),
                TextAlignment::Right,
            ),
        ] {
            row = row.child(
                self.icon_button(id, icon, title, Action::TextAlign(align), !editable, cx)
                    .when(style.alignment == align, |button| {
                        button.bg(rgb(0xeaf0fd)).text_color(rgb(0x467bc6))
                    }),
            );
        }
        row.child(chrome::separator())
    }
}
