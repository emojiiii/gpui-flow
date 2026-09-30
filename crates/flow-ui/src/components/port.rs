//! A typed connection handle usable independently of the graph editor.
use super::PointerCallback;
use crate::FlowTheme;
use gpui::{prelude::*, *};
use gpui_flow_core::{DataType, Point as ModelPoint, PortDirection};

#[derive(IntoElement)]
pub struct PortHandle {
    pub id: ElementId,
    pub direction: PortDirection,
    pub data_type: DataType,
    pub label: SharedString,
    /// Center of the handle, in parent-local screen pixels.
    pub anchor: Option<ModelPoint>,
    pub diameter: f32,
    pub color: u32,
    pub active: bool,
    pub show_label: bool,
    pub theme: FlowTheme,
    on_connect: Option<PointerCallback>,
}
/// Short alias for the same reusable typed port component.
pub type Handle = PortHandle;
impl PortHandle {
    pub fn new(id: impl Into<ElementId>, direction: PortDirection, data_type: DataType) -> Self {
        Self {
            id: id.into(),
            direction,
            data_type,
            label: "".into(),
            anchor: None,
            diameter: 12.,
            color: 0x9cafc9,
            active: false,
            show_label: false,
            theme: FlowTheme::default(),
            on_connect: None,
        }
    }
    pub fn from_port(id: impl Into<ElementId>, port: &gpui_flow_core::Port) -> Self {
        Self::new(id, port.direction, port.data_type.clone()).label(port.label.clone())
    }
    pub fn label(mut self, value: impl Into<SharedString>) -> Self {
        self.label = value.into();
        self
    }
    pub fn anchor(mut self, value: ModelPoint) -> Self {
        self.anchor = Some(value);
        self
    }
    pub fn diameter(mut self, value: f32) -> Self {
        if value.is_finite() && value > 0. {
            self.diameter = value;
        }
        self
    }
    pub fn color(mut self, value: u32) -> Self {
        self.color = value;
        self
    }
    pub fn active(mut self, value: bool) -> Self {
        self.active = value;
        self
    }
    pub fn show_label(mut self, value: bool) -> Self {
        self.show_label = value;
        self
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    /// The host decides whether a click starts, completes, or rejects a connection.
    pub fn on_connect(
        mut self,
        callback: impl Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_connect = Some(Box::new(callback));
        self
    }
}
impl RenderOnce for PortHandle {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut handle = div()
            .id(self.id)
            .relative()
            .size(px(self.diameter))
            .rounded_full()
            .bg(rgb(if self.active {
                self.theme.accent
            } else {
                self.color
            }))
            .border_2()
            .border_color(rgb(self.theme.background));
        if let Some(anchor) = self.anchor {
            handle = handle
                .absolute()
                .left(px(anchor.x - self.diameter * 0.5))
                .top(px(anchor.y - self.diameter * 0.5));
        }
        if self.show_label {
            let label = div()
                .absolute()
                .top(px(-2.))
                .text_xs()
                .text_color(rgb(self.theme.muted))
                .child(self.label);
            handle = handle.child(match self.direction {
                PortDirection::Input => label.left(px(self.diameter + 5.)),
                PortDirection::Output => label.right(px(self.diameter + 5.)),
            });
        }
        if let Some(callback) = self.on_connect {
            handle = handle
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, callback);
        }
        handle
    }
}
