use super::*;

pub(super) fn alignment(value: TextAlignment) -> gpui::TextAlign {
    match value {
        TextAlignment::Left => gpui::TextAlign::Left,
        TextAlignment::Center => gpui::TextAlign::Center,
        TextAlignment::Right => gpui::TextAlign::Right,
    }
}

impl CreativeCanvas {
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
                    .font_weight(if style.bold {
                        FontWeight::BOLD
                    } else {
                        FontWeight::NORMAL
                    })
                    .text_align(alignment(style.alignment))
                    .line_height(relative(1.25))
                    .text_color(rgb(appearance.color))
                    .when_some(
                        appearance.installed_family(&self.font_families),
                        |v, family| v.font_family(SharedString::from(family.to_string())),
                    )
                    .aria_label("画布文字，按 Escape 完成编辑"),
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
                    "字体",
                    Action::TextAppearance(text_appearance_tools::TypographyAction::Toggle),
                    !editable,
                    cx,
                )
                .when(self.typography.is_some(), |v| v.bg(rgb(0xeff4ff))),
            )
            .child(self.icon_button(
                "text-smaller",
                "minus",
                "减小字号",
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
                "增大字号",
                Action::TextSize(2.),
                !editable || style.font_size >= 144.,
                cx,
            ))
            .child(chrome::separator())
            .child(
                self.icon_button("text-bold", "bold", "粗体", Action::TextBold, !editable, cx)
                    .when(style.bold, |button| {
                        button.bg(rgb(0xeaf0fd)).text_color(rgb(0x467bc6))
                    }),
            );
        for (id, icon, title, align) in [
            (
                "text-left",
                "text-align-start",
                "左对齐",
                TextAlignment::Left,
            ),
            (
                "text-center",
                "text-align-center",
                "居中",
                TextAlignment::Center,
            ),
            (
                "text-right",
                "text-align-end",
                "右对齐",
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
