use super::*;
use crate::text_appearance::{TextAppearance, parse_text_color};

#[derive(Clone)]
pub(super) enum TypographyAction {
    Toggle,
    Font(Option<String>),
    Weight(u16),
    Color(u32),
    ApplyColor,
}

pub(super) struct TypographyPanel {
    node: NodeId,
    filter: Entity<TextareaState>,
    color: Entity<TextareaState>,
    source_color: u32,
    _subscriptions: Vec<Subscription>,
}

impl CreativeCanvas {
    pub(super) fn is_typography_open(&self, node: &NodeId) -> bool {
        self.typography
            .as_ref()
            .is_some_and(|panel| &panel.node == node)
    }

    pub(super) fn reconcile_typography(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.selected(cx).filter(|node| node.kind == "text");
        let Some(panel) = &mut self.typography else {
            return;
        };
        let Some(node) = selected.filter(|node| node.id == panel.node) else {
            self.typography = None;
            return;
        };
        let appearance = TextAppearance::from_data(&node.data).unwrap_or_default();
        if panel.source_color != appearance.color {
            panel.source_color = appearance.color;
            panel.color.update(cx, |input, cx| {
                input.set_value(format!("#{:06X}", appearance.color), window, cx)
            });
        }
    }

    pub(super) fn handle_typography(
        &mut self,
        action: TypographyAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let TypographyAction::Weight(weight) = action {
            self.set_text_weight(weight, window, cx);
            return;
        }
        if matches!(action, TypographyAction::Toggle) && self.typography.is_some() {
            self.typography = None;
            self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            cx.notify();
            return;
        }
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
        let mut appearance = TextAppearance::from_data(&node.data).unwrap_or_default();
        let original = appearance.clone();
        if matches!(action, TypographyAction::Toggle) {
            self.clear_text_edit();
            let filter = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .placeholder(tr("搜索本机字体…", "Search installed fonts…"))
                    .context_menu(false)
            });
            let color = cx.new(|cx| {
                TextareaState::new(window, cx)
                    .default_value(format!("#{:06X}", appearance.color))
                    .context_menu(false)
            });
            let subscriptions = [&filter, &color]
                .into_iter()
                .map(|input| {
                    cx.subscribe(input, |_, _, event, cx| {
                        if matches!(event, InputEvent::Change) {
                            cx.notify();
                        }
                    })
                })
                .collect();
            filter.read(cx).focus_handle(cx).focus(window, cx);
            self.typography = Some(TypographyPanel {
                node: node.id,
                filter,
                color,
                source_color: appearance.color,
                _subscriptions: subscriptions,
            });
            cx.notify();
            return;
        }
        match action {
            TypographyAction::Font(family) => {
                if family
                    .as_ref()
                    .is_some_and(|name| !self.font_families.contains(name))
                {
                    self.notice = Some(
                        tr(
                            "本机没有此字体，请重新选择",
                            "Choose a font installed on this device",
                        )
                        .into(),
                    );
                    cx.notify();
                    return;
                }
                appearance.font_family = family;
            }
            TypographyAction::Color(color) => appearance.color = color,
            TypographyAction::ApplyColor => {
                let Some(panel) = &self.typography else {
                    return;
                };
                match parse_text_color(panel.color.read(cx).value().as_ref()) {
                    Ok(color) => appearance.color = color,
                    Err(error) => {
                        self.notice = Some(error);
                        cx.notify();
                        return;
                    }
                }
            }
            TypographyAction::Toggle | TypographyAction::Weight(_) => unreachable!(),
        }
        if appearance == original {
            return;
        }
        match appearance.write_to_data(&node.data) {
            Ok(data) if data != node.data => {
                self.clear_text_edit();
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
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
        cx.notify();
    }

    pub(super) fn typography_panel(
        &self,
        node: &Node,
        editable: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let Some(panel) = &self.typography else {
            return div();
        };
        let filter = panel.filter.clone();
        let color = panel.color.clone();
        let query = filter.read(cx).value().trim().to_lowercase();
        let appearance = TextAppearance::from_data(&node.data).unwrap_or_default();
        let weight = crate::text_weight::read_weight(&node.data).unwrap_or(400);
        let fonts = self
            .font_families
            .iter()
            .filter(|font| font.to_lowercase().contains(&query))
            .take(60)
            .cloned()
            .collect::<Vec<_>>();
        let missing = appearance.font_family.is_some()
            && appearance.installed_family(&self.font_families).is_none();
        let mut list = div()
            .id("local-font-list")
            .max_h(px(136.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(
                self.button(
                    "default-text-family",
                    tr("默认字体", "Default font"),
                    Action::TextAppearance(TypographyAction::Font(None)),
                    !editable,
                    cx,
                )
                .w_full()
                .flex_shrink_0()
                .when(appearance.font_family.is_none(), |v| v.bg(rgb(0xeff4ff))),
            );
        for (index, name) in fonts.into_iter().enumerate() {
            list = list.child(
                self.button(
                    SharedString::from(format!("local-font-{index}")),
                    name.clone(),
                    Action::TextAppearance(TypographyAction::Font(Some(name.clone()))),
                    !editable,
                    cx,
                )
                .w_full()
                .flex_shrink_0()
                .overflow_hidden()
                .when(appearance.font_family.as_ref() == Some(&name), |v| {
                    v.bg(rgb(0xeff4ff))
                }),
            );
        }
        let mut swatches = div().flex().gap_1();
        for (index, rgb_value) in [
            0x32343a, 0xffffff, 0xef4444, 0xf59e0b, 0x22a06b, 0x3b82f6, 0x8b5cf6, 0xec4899,
        ]
        .into_iter()
        .enumerate()
        {
            swatches = swatches.child(
                self.button(
                    SharedString::from(format!("text-color-{index}")),
                    "",
                    Action::TextAppearance(TypographyAction::Color(rgb_value)),
                    !editable,
                    cx,
                )
                .accessibility_label(tr_format!(
                    "文字颜色 #{rgb_value:06X}",
                    "Text color #{rgb_value:06X}"
                ))
                .w(px(28.))
                .px_0()
                .border_1()
                .border_color(rgb(if appearance.color == rgb_value {
                    0x4d8bea
                } else {
                    0xdfe2e7
                }))
                .child(div().size(px(16.)).rounded_sm().bg(rgb(rgb_value))),
            );
        }
        let mut weights = div().flex().flex_wrap().gap_1();
        for value in (100..=900).step_by(100) {
            weights = weights.child(
                self.button(
                    SharedString::from(format!("text-weight-{value}")),
                    value.to_string(),
                    Action::TextAppearance(TypographyAction::Weight(value)),
                    !editable,
                    cx,
                )
                .w(px(48.))
                .flex_shrink_0()
                .accessibility_label(tr_format!("字重 {value}", "Font weight {value}"))
                .when(value == weight, |v| {
                    v.bg(rgb(0xeff4ff)).text_color(rgb(0x467bc6))
                }),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(label(if missing {
                tr(
                    "所存字体本机不可用，当前使用默认字体",
                    "The saved font is unavailable. Using the default font.",
                )
            } else {
                tr("本机字体 · 字重 · 颜色", "Font · Weight · Color")
            }))
            .child(
                Textarea::new(&filter)
                    .h(px(30.))
                    .aria_label(tr("搜索本机字体", "Search installed fonts")),
            )
            .child(list)
            .child(label(tr_format!(
                "字重 {weight} · 字体按本机可用款式匹配",
                "Font weight {weight} · Matches installed variants"
            )))
            .child(weights)
            .child(label(tr("文字颜色", "Text color")))
            .child(swatches)
            .child(
                div()
                    .flex()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                                if event.keystroke.key == "enter" {
                                    cx.stop_propagation();
                                    this.action(
                                        Action::TextAppearance(TypographyAction::ApplyColor),
                                        window,
                                        cx,
                                    );
                                }
                            }))
                            .child(
                                Textarea::new(&color)
                                    .h(px(30.))
                                    .aria_label(tr("文字十六进制颜色", "Hex text color")),
                            ),
                    )
                    .child(self.button(
                        "apply-text-color",
                        tr("应用颜色", "Apply color"),
                        Action::TextAppearance(TypographyAction::ApplyColor),
                        !editable,
                        cx,
                    )),
            )
            .child(div().text_size(px(10.)).text_color(rgb(0x90939d)).child(tr(
                "搜索筛选本机字体；每次修改可撤销 · Esc 关闭",
                "Search installed fonts · Changes can be undone · Esc to close",
            )))
    }
}
