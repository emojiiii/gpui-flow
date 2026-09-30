//! Reusable toolbar shell with leading content and action slots.
use crate::FlowTheme;
use gpui::{prelude::*, *};
#[derive(IntoElement)]
pub struct Toolbar {
    pub id: ElementId,
    pub theme: FlowTheme,
    pub height: f32,
    leading: Option<AnyElement>,
    actions: Vec<AnyElement>,
}
impl Toolbar {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            theme: FlowTheme::default(),
            height: 64.,
            leading: None,
            actions: vec![],
        }
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    pub fn height(mut self, value: f32) -> Self {
        self.height = value.max(0.);
        self
    }
    pub fn leading(mut self, value: impl IntoElement) -> Self {
        self.leading = Some(value.into_any_element());
        self
    }
    pub fn child(mut self, value: impl IntoElement) -> Self {
        self.actions.push(value.into_any_element());
        self
    }
}
impl RenderOnce for Toolbar {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        div()
            .id(self.id)
            .h(px(self.height))
            .flex_shrink_0()
            .px_5()
            .flex()
            .items_center()
            .justify_between()
            .bg(rgb(self.theme.panel))
            .border_b_1()
            .border_color(rgb(self.theme.border))
            .text_color(rgb(self.theme.foreground))
            .child(div().flex().items_center().gap_3().children(self.leading))
            .child(div().flex().items_center().gap_2().children(self.actions))
    }
}
