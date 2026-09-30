//! Small host-controlled action button shared by canvas chrome.
//!
//! GPUI Kit owns activation, keyboard focus, and button semantics; this adapter
//! owns the host's theme and keeps pointer presses from taking canvas focus.
use super::ClickCallback;
use crate::FlowTheme;
use gpui::{prelude::*, *};
use gpui_kit::base::Button;
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

    fn into_button(self, line_height: DefiniteLength) -> Button {
        let theme = self.theme;
        let actionable = !self.disabled && self.on_click.is_some();
        Button::new(self.id)
            .disabled(self.disabled)
            // A label without a callback must not become a dead keyboard stop.
            .focusable(actionable)
            .accessibility_label(self.label.clone())
            // Retain the original single-label block geometry rather than
            // adopting Base Button's centered flex layout.
            .block()
            .px_3()
            .py_2()
            .rounded_md()
            .bg(rgb(theme.card))
            .border_1()
            .border_color(rgb(theme.border))
            .text_sm()
            // Base Button supplies a neutral line height; retain the host's
            // inherited value so adopting it does not shrink existing chrome.
            .line_height(line_height)
            .text_color(rgb(if self.disabled {
                theme.muted
            } else {
                theme.foreground
            }))
            .child(self.label)
            // Keep this guard on disabled and callback-less controls too: a
            // press on canvas chrome must not start selection or a drag.
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .when(actionable, |button| {
                button
                    // Retain pointer focus in the canvas/input. This only
                    // suppresses GPUI's implicit focus transfer, not bubbling
                    // middle/right gestures or a host callback's explicit focus.
                    .on_any_mouse_down(|_, window, _| window.prevent_default())
                    .cursor_pointer()
                    .hover(move |style| style.bg(rgb(theme.border)))
                    .focus_visible(move |style| style.border_color(rgb(theme.accent)))
            })
            // One native click path handles pointer, Enter, and Space. Do not
            // synthesize a second click in a key or mouse-up listener.
            .when_some(self.on_click, |button, callback| button.on_click(callback))
    }
}
impl RenderOnce for ControlButton {
    fn render(self, window: &mut Window, _: &mut App) -> impl IntoElement {
        self.into_button(window.text_style().line_height)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn builder_preserves_host_owned_configuration() {
        let theme = FlowTheme {
            card: 0xabcdef,
            ..FlowTheme::default()
        };
        let button = ControlButton::new("save", "Save")
            .theme(theme)
            .disabled(true)
            .on_click(|_, _, _| {});
        assert_eq!(button.id, ElementId::from("save"));
        assert_eq!(button.label, SharedString::from("Save"));
        assert_eq!(button.theme.card, theme.card);
        assert!(button.disabled);
        assert!(button.on_click.is_some());
    }

    #[test]
    fn kit_adapter_preserves_theme_and_inherited_line_height() {
        let theme = FlowTheme {
            card: 0x123456,
            border: 0x345678,
            foreground: 0x56789a,
            ..FlowTheme::default()
        };
        let line_height = relative(1.75);
        let mut button: Button = ControlButton::new("host-theme", "Run")
            .theme(theme)
            .on_click(|_, _, _| {})
            .into_button(line_height);
        let style = button.style();
        assert_eq!(style.display, Some(Display::Block));
        assert_eq!(style.background, Some(rgb(theme.card).into()));
        assert_eq!(style.border_color, Some(rgb(theme.border).into()));
        assert_eq!(style.mouse_cursor, Some(CursorStyle::PointingHand));
        let text = &style.text;
        assert_eq!(text.color, Some(rgb(theme.foreground).into()));
        assert_eq!(text.line_height, Some(line_height));
    }

    #[test]
    fn disabled_and_callbackless_buttons_keep_inert_presentation() {
        let theme = FlowTheme::default();
        let mut disabled = ControlButton::new("disabled", "Run")
            .disabled(true)
            .on_click(|_, _, _| panic!("building a disabled control must not activate it"))
            .into_button(relative(1.5));
        assert_eq!(disabled.style().mouse_cursor, None);
        assert_eq!(disabled.style().text.color, Some(rgb(theme.muted).into()));
        let mut callbackless = ControlButton::new("label-only", "Run").into_button(relative(1.5));
        assert_eq!(callbackless.style().mouse_cursor, None);
        assert_eq!(
            callbackless.style().text.color,
            Some(rgb(theme.foreground).into())
        );
    }
}
