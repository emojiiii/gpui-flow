//! Small reusable, independently renderable canvas building blocks.
use crate::FlowTheme;
use gpui::{prelude::*, *};
pub fn badge(label: impl Into<SharedString>, color: u32) -> Div {
    div()
        .px_2()
        .py_1()
        .rounded_md()
        .text_xs()
        .bg(rgb(color).alpha(0.12))
        .text_color(rgb(color))
        .child(label.into())
}
pub fn panel_title(label: impl Into<SharedString>, theme: FlowTheme) -> Div {
    div()
        .text_xs()
        .text_color(rgb(theme.muted))
        .child(label.into())
}
pub fn progress_bar(progress: f32, theme: FlowTheme) -> Div {
    let progress = if progress.is_finite() {
        progress.clamp(0., 1.)
    } else {
        0.
    };
    div()
        .h(px(4.))
        .w_full()
        .rounded_full()
        .bg(rgb(theme.border))
        .child(
            div()
                .h_full()
                .w(relative(progress))
                .bg(rgb(theme.accent))
                .rounded_full(),
        )
}
