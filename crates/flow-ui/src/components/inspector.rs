//! Inspector shell for arbitrary host-owned fields and editors.
use crate::{FlowTheme, panel_title};
use gpui::{prelude::*, *};
#[derive(IntoElement)]
pub struct Inspector {
    pub id: ElementId,
    pub title: SharedString,
    pub width: f32,
    pub theme: FlowTheme,
    children: Vec<AnyElement>,
}
impl Inspector {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            title: "INSPECTOR".into(),
            width: 235.,
            theme: FlowTheme::default(),
            children: vec![],
        }
    }
    pub fn title(mut self, value: impl Into<SharedString>) -> Self {
        self.title = value.into();
        self
    }
    pub fn width(mut self, value: f32) -> Self {
        self.width = value.max(0.);
        self
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    pub fn child(mut self, value: impl IntoElement) -> Self {
        self.children.push(value.into_any_element());
        self
    }
    /// Add an ordinary read-only field. Supply an input element with `child` to edit.
    pub fn field(self, label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        self.child(
            div()
                .flex()
                .flex_col()
                .gap_1()
                .child(div().text_xs().child(label.into()))
                .child(div().text_sm().child(value.into())),
        )
    }
}
impl RenderOnce for Inspector {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .w(px(self.width))
            .flex_shrink_0()
            .bg(rgb(self.theme.panel))
            .border_l_1()
            .border_color(rgb(self.theme.border))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .text_color(rgb(self.theme.foreground))
            .child(panel_title(self.title, self.theme))
            .children(self.children)
    }
}
