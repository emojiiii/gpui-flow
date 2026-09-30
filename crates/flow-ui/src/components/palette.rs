//! Data-driven palette; node creation belongs to its host callback.
use crate::{FlowTheme, panel_title};
use gpui::{prelude::*, *};
use std::rc::Rc;
#[derive(Clone, Debug)]
pub struct PaletteEntry {
    pub kind: SharedString,
    pub label: SharedString,
    pub color: u32,
}
impl PaletteEntry {
    pub fn new(kind: impl Into<SharedString>, label: impl Into<SharedString>, color: u32) -> Self {
        Self {
            kind: kind.into(),
            label: label.into(),
            color,
        }
    }
}
type SelectCallback = Rc<dyn Fn(&str, &mut Window, &mut App)>;
#[derive(IntoElement)]
pub struct Palette {
    pub id: ElementId,
    pub title: SharedString,
    pub width: f32,
    pub theme: FlowTheme,
    pub entries: Vec<PaletteEntry>,
    pub disabled: bool,
    footer: Option<AnyElement>,
    on_select: Option<SelectCallback>,
}
impl Palette {
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            title: "COMPONENTS".into(),
            width: 170.,
            theme: FlowTheme::default(),
            entries: vec![],
            disabled: false,
            footer: None,
            on_select: None,
        }
    }
    pub fn disabled(mut self, value: bool) -> Self {
        self.disabled = value;
        self
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
    pub fn entry(mut self, value: PaletteEntry) -> Self {
        self.entries.push(value);
        self
    }
    pub fn entries(mut self, value: impl IntoIterator<Item = PaletteEntry>) -> Self {
        self.entries.extend(value);
        self
    }
    pub fn footer(mut self, value: impl IntoElement) -> Self {
        self.footer = Some(value.into_any_element());
        self
    }
    pub fn on_select(mut self, callback: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_select = Some(Rc::new(callback));
        self
    }
}
impl RenderOnce for Palette {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut palette = div()
            .id(self.id)
            .w(px(self.width))
            .flex_shrink_0()
            .bg(rgb(self.theme.panel))
            .border_r_1()
            .border_color(rgb(self.theme.border))
            .p_4()
            .flex()
            .flex_col()
            .gap_3()
            .child(panel_title(self.title, self.theme));
        for entry in self.entries {
            let mut item = div()
                .id(entry.kind.clone())
                .p_3()
                .rounded_md()
                .border_1()
                .border_color(rgb(self.theme.border))
                .text_sm()
                .text_color(rgb(if self.disabled {
                    self.theme.muted
                } else {
                    entry.color
                }))
                .child(entry.label);
            if !self.disabled
                && let Some(callback) = self.on_select.clone()
            {
                item = item
                    .cursor_pointer()
                    .on_click(move |_, window, cx| callback(entry.kind.as_ref(), window, cx));
            }
            palette = palette.child(item);
        }
        palette.children(self.footer)
    }
}
