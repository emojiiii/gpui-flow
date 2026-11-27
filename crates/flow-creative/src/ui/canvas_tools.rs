use super::*;
use crate::canvas_preferences::{
    CanvasPreferences, read_canvas_preferences, write_canvas_preferences,
};

#[derive(Clone)]
pub(super) enum CanvasAppearanceAction {
    BackgroundPanel,
    Background(u32),
    ApplyBackground,
    ToggleMinimap,
}

pub(super) struct BackgroundPanel {
    input: Entity<TextareaState>,
    source_color: u32,
    _subscription: Subscription,
}

impl CreativeCanvas {
    pub(super) fn rail_button(
        &self,
        id: impl Into<ElementId>,
        icon: &'static str,
        title: &'static str,
        action: Action,
        disabled: bool,
        cx: &mut Context<Self>,
    ) -> Button {
        self.icon_button(id, icon, title, action, disabled, cx)
            .flex_shrink_0()
    }

    pub(super) fn sync_canvas_appearance(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let preferences =
            read_canvas_preferences(self.canvas.read(cx).editor.graph()).unwrap_or_default();
        self.canvas.update(cx, |canvas, cx| {
            if canvas.theme.background != preferences.background_rgb
                || canvas.options.show_minimap != preferences.show_minimap
            {
                canvas.theme.background = preferences.background_rgb;
                canvas.options.show_minimap = preferences.show_minimap;
                cx.notify();
            }
        });
        if let Some(panel) = &mut self.background_panel
            && panel.source_color != preferences.background_rgb
        {
            panel.source_color = preferences.background_rgb;
            panel.input.update(cx, |input, cx| {
                input.set_value(format!("#{:06X}", preferences.background_rgb), window, cx)
            });
        }
    }

    pub(super) fn canvas_appearance_action(
        &mut self,
        action: CanvasAppearanceAction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if matches!(action, CanvasAppearanceAction::BackgroundPanel) {
            if self.background_panel.is_some() {
                self.background_panel = None;
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
            } else {
                let color = self.canvas.read(cx).theme.background;
                let input = cx.new(|cx| {
                    TextareaState::new(window, cx)
                        .default_value(format!("#{color:06X}"))
                        .context_menu(false)
                });
                let subscription = cx.subscribe(&input, |_, _, event, cx| {
                    if matches!(event, InputEvent::Change) {
                        cx.notify();
                    }
                });
                input.read(cx).focus_handle(cx).focus(window, cx);
                self.background_panel = Some(BackgroundPanel {
                    input,
                    source_color: color,
                    _subscription: subscription,
                });
            }
            cx.notify();
            return;
        }
        if self.importing || self.saving || !self.canvas.read(cx).is_editable() {
            return;
        }
        let result = (|| {
            let mut graph = self.canvas.read(cx).editor.graph().clone();
            let mut preferences = read_canvas_preferences(&graph).map_err(|e| e.to_string())?;
            match action {
                CanvasAppearanceAction::Background(color) => preferences.background_rgb = color,
                CanvasAppearanceAction::ToggleMinimap => {
                    preferences.show_minimap = !preferences.show_minimap
                }
                CanvasAppearanceAction::ApplyBackground => {
                    let panel = self.background_panel.as_ref().ok_or(tr(
                        "请先打开画布背景",
                        "Open the canvas background panel first",
                    ))?;
                    preferences.background_rgb = crate::text_appearance::parse_text_color(
                        panel.input.read(cx).value().as_ref(),
                    )?;
                }
                CanvasAppearanceAction::BackgroundPanel => unreachable!(),
            }
            let changed =
                write_canvas_preferences(&mut graph, preferences).map_err(|e| e.to_string())?;
            Ok::<_, String>(changed.then_some(graph))
        })();
        match result {
            Ok(Some(graph)) => {
                self.canvas.read(cx).focus_handle(cx).focus(window, cx);
                self.command(CanvasCommand::ImportGraph(graph), cx);
            }
            Ok(None) => {}
            Err(error) => self.notice = Some(error),
        }
        cx.notify();
    }

    pub(super) fn background_panel(&self, editable: bool, cx: &mut Context<Self>) -> Div {
        let Some(panel) = &self.background_panel else {
            return div();
        };
        let input = panel.input.clone();
        let current = self.canvas.read(cx).theme.background;
        let mut swatches = div().flex().gap_2();
        for (index, color) in [0xf5f6f8, 0xffffff, 0xece7de, 0xdfe9e3, 0xe5e8f3, 0x23252b]
            .into_iter()
            .enumerate()
        {
            swatches = swatches.child(
                self.button(
                    SharedString::from(format!("background-{index}")),
                    "",
                    Action::CanvasAppearance(CanvasAppearanceAction::Background(color)),
                    !editable,
                    cx,
                )
                .accessibility_label(tr_format!(
                    "画布背景 #{color:06X}",
                    "Canvas background #{color:06X}"
                ))
                .size(px(28.))
                .px_0()
                .border_1()
                .border_color(rgb(if current == color { 0x4d8bea } else { 0xdfe2e7 }))
                .child(div().size(px(20.)).rounded_sm().bg(rgb(color))),
            );
        }
        let body = div()
            .min_w(px(256.))
            .p_3()
            .flex()
            .flex_col()
            .gap_2()
            .child(label(tr("画布背景", "Canvas background")))
            .child(swatches)
            .child(
                div()
                    .flex()
                    .gap_1()
                    .child(
                        div()
                            .flex_1()
                            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                                if event.keystroke.key == "enter" {
                                    cx.stop_propagation();
                                    this.action(
                                        Action::CanvasAppearance(
                                            CanvasAppearanceAction::ApplyBackground,
                                        ),
                                        window,
                                        cx,
                                    );
                                }
                            }))
                            .child(Textarea::new(&input).h(px(30.)).aria_label(tr(
                                "画布背景十六进制颜色",
                                "Canvas background hex color",
                            ))),
                    )
                    .child(self.button(
                        "apply-canvas-background",
                        tr("应用", "Apply"),
                        Action::CanvasAppearance(CanvasAppearanceAction::ApplyBackground),
                        !editable,
                        cx,
                    )),
            )
            .child(self.button(
                "reset-canvas-background",
                tr("恢复默认背景", "Restore default background"),
                Action::CanvasAppearance(CanvasAppearanceAction::Background(
                    CanvasPreferences::default().background_rgb,
                )),
                !editable,
                cx,
            ))
            .child(div().text_size(px(10.)).text_color(rgb(0x90939d)).child(tr(
                "只改变工作区背景 · 随项目保存 · 可撤销",
                "Canvas background only · Saved with project · Undoable",
            )));
        let size = bounded_panel_size(self.canvas_size, WorldSize::new(280., 178.));
        div()
            .occlude()
            .absolute()
            .left(px(18.))
            .bottom(px(60.))
            .w(px(size.width))
            .rounded(px(12.))
            .bg(rgb(0xffffff))
            .shadow_sm()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .id("canvas-background-scroll")
                    .max_h(px(size.height))
                    .overflow_x_scroll()
                    .overflow_y_scroll()
                    .child(body),
            )
    }
}
