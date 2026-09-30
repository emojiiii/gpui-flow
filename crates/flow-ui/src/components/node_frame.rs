//! Graph-agnostic node shell with body and overlay slots.
use super::{PointerCallback, valid_zoom};
use crate::FlowTheme;
use gpui::{prelude::*, *};
use gpui_flow_core as model;

#[derive(IntoElement)]
pub struct NodeFrame {
    pub id: ElementId,
    pub title: SharedString,
    /// Screen-space origin; `None` participates in ordinary GPUI layout.
    pub position: Option<model::Point>,
    /// Unscaled logical size.
    pub size: model::Size,
    pub zoom: f32,
    pub selected: bool,
    /// Active drag feedback supplied by the host interaction controller.
    pub dragging: bool,
    pub header_cursor: CursorStyle,
    pub theme: FlowTheme,
    pub color: u32,
    pub header_height: f32,
    body: Option<AnyElement>,
    overlays: Vec<AnyElement>,
    on_header_mouse_down: Option<PointerCallback>,
    on_resize_mouse_down: Option<PointerCallback>,
    on_mouse_down: Option<PointerCallback>,
}
impl NodeFrame {
    pub fn new(id: impl Into<ElementId>, title: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            title: title.into(),
            position: None,
            size: model::Size::new(252., 190.),
            zoom: 1.,
            selected: false,
            dragging: false,
            header_cursor: CursorStyle::Arrow,
            theme: FlowTheme::default(),
            color: 0x9cafc9,
            header_height: 36.,
            body: None,
            overlays: Vec::new(),
            on_header_mouse_down: None,
            on_resize_mouse_down: None,
            on_mouse_down: None,
        }
    }
    pub fn position(mut self, value: model::Point) -> Self {
        self.position = Some(value);
        self
    }
    pub fn size(mut self, value: model::Size) -> Self {
        self.size = value;
        self
    }
    pub fn zoom(mut self, value: f32) -> Self {
        self.zoom = valid_zoom(value);
        self
    }
    pub fn selected(mut self, value: bool) -> Self {
        self.selected = value;
        self
    }
    pub fn dragging(mut self, value: bool) -> Self {
        self.dragging = value;
        self
    }
    pub fn header_cursor(mut self, value: CursorStyle) -> Self {
        self.header_cursor = value;
        self
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    pub fn color(mut self, value: u32) -> Self {
        self.color = value;
        self
    }
    pub fn header_height(mut self, value: f32) -> Self {
        self.header_height = value.max(0.);
        self
    }
    pub fn body(mut self, value: impl IntoElement) -> Self {
        self.body = Some(value.into_any_element());
        self
    }
    /// Add an overlay such as a port, status badge, or host-owned affordance.
    pub fn child(mut self, value: impl IntoElement) -> Self {
        self.overlays.push(value.into_any_element());
        self
    }
    pub fn on_header_mouse_down(
        mut self,
        callback: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_header_mouse_down = Some(Box::new(callback));
        self
    }
    pub fn on_resize_mouse_down(
        mut self,
        callback: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_resize_mouse_down = Some(Box::new(callback));
        self
    }
    pub fn on_mouse_down(
        mut self,
        callback: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_mouse_down = Some(Box::new(callback));
        self
    }
}
impl RenderOnce for NodeFrame {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let z = valid_zoom(self.zoom);
        let theme = self.theme;
        let mut header = div()
            .h(px(self.header_height * z))
            .px(px(13. * z))
            .flex()
            .items_center()
            .gap_2()
            .border_b_1()
            .border_color(rgb(theme.border))
            .child(div().size(px(7. * z)).rounded_full().bg(rgb(self.color)))
            .child(
                div()
                    .text_size(px(12. * z))
                    .text_color(rgb(theme.foreground))
                    .child(self.title),
            );
        if let Some(callback) = self.on_header_mouse_down {
            header = header
                .cursor(if self.dragging {
                    CursorStyle::ClosedHand
                } else {
                    self.header_cursor
                })
                .on_mouse_down(MouseButton::Left, callback);
        }
        let mut frame = div()
            .id(self.id)
            .relative()
            .cursor(if self.dragging {
                CursorStyle::ClosedHand
            } else {
                self.header_cursor
            })
            .w(px(self.size.width * z))
            .h(px(self.size.height * z))
            .bg(rgb(theme.card))
            .rounded(px(10. * z))
            .border_1()
            .border_color(rgb(if self.selected {
                theme.accent
            } else {
                theme.border
            }))
            .shadow_md()
            .child(header)
            .child(
                div()
                    .overflow_hidden()
                    .h(px((self.size.height - self.header_height - 3.).max(0.) * z))
                    .children(self.body),
            );
        if let Some(position) = self.position {
            frame = frame.absolute().left(px(position.x)).top(px(position.y));
        }
        if let Some(callback) = self.on_mouse_down {
            frame = frame.on_mouse_down(MouseButton::Left, callback);
        }
        if self.selected
            && self.size.width * z >= 36.
            && self.size.height * z >= 36.
            && let Some(callback) = self.on_resize_mouse_down
        {
            // Fixed screen-pixel hit target, inside the card border. The square
            // is an actual resize affordance, not a font-dependent corner glyph.
            frame = frame.child(
                div()
                    .absolute()
                    .right(px(2.))
                    .bottom(px(2.))
                    .size(px(20.))
                    .rounded_sm()
                    .cursor(CursorStyle::ResizeUpLeftDownRight)
                    .hover(move |style| style.bg(rgb(theme.accent).alpha(0.12)))
                    .child(
                        div()
                            .absolute()
                            .right(px(3.))
                            .bottom(px(3.))
                            .size(px(8.))
                            .rounded_sm()
                            .bg(rgb(theme.panel))
                            .border_2()
                            .border_color(rgb(theme.accent)),
                    )
                    .on_mouse_down(MouseButton::Left, callback),
            );
        }
        frame.children(self.overlays)
    }
}
