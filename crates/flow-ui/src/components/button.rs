//! Small host-controlled action button shared by canvas chrome.
use super::ClickCallback;
use crate::FlowTheme;
use gpui::{prelude::*, *};
#[derive(IntoElement)]
pub struct ControlButton {
    pub id: ElementId,
    pub label: SharedString,
    pub theme: FlowTheme,
    pub disabled: bool,
    on_click: Option<ClickCallback>,
}
impl ControlButton {
    pub fn new(id: impl Into<ElementId>, label: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            label: label.into(),
            theme: FlowTheme::default(),
            disabled: false,
            on_click: None,
        }
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    pub fn disabled(mut self, value: bool) -> Self {
        self.disabled = value;
        self
    }
    pub fn on_click(
        mut self,
        callback: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(callback));
        self
    }
}
impl RenderOnce for ControlButton {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let theme = self.theme;
        let mut button = div()
            .id(self.id)
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(theme.card))
            .border_1()
            .border_color(rgb(theme.border))
            .text_sm()
            .text_color(rgb(if self.disabled {
                theme.muted
            } else {
                theme.foreground
            }))
            .child(self.label)
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
        if !self.disabled
            && let Some(callback) = self.on_click
        {
            button = button
                .cursor_pointer()
                .hover(move |style| style.bg(rgb(theme.border)))
                .on_click(callback);
        }
        button
    }
}
