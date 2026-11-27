//! A typed connection handle usable independently of the graph editor.
use super::PointerCallback;
use crate::FlowTheme;
use gpui::{prelude::*, *};
use gpui_flow_core::{DataType, Point as ModelPoint, PortDirection};

/// Pointer feedback is derived from the host's validated connection gesture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PortState {
    #[default]
    Idle,
    Source,
    Compatible,
    Target,
    Rejected,
    Disabled,
}

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
    pub state: PortState,
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
            data_type: data_type.clone(),
            label: "".into(),
            anchor: None,
            diameter: 12.,
            color: crate::default_port_color(&data_type),
            active: false,
            state: PortState::Idle,
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
    pub fn state(mut self, value: PortState) -> Self {
        self.state = value;
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
    /// The host starts a pointer gesture here and resolves it on pointer release.
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
        let state = if self.active && self.state == PortState::Idle {
            PortState::Source
        } else {
            self.state
        };
        let color = match state {
            PortState::Rejected => 0xf17c88,
            PortState::Source | PortState::Target | PortState::Compatible => self.color,
            PortState::Disabled | PortState::Idle => self.color,
        };
        let hit = self.diameter.max(22.);
        let size = if state == PortState::Target {
            self.diameter + 4.
        } else {
            self.diameter
        };
        let tip = if self.label.is_empty() {
            match self.direction {
                PortDirection::Input => "输入端口".into(),
                PortDirection::Output => "输出端口".into(),
            }
        } else {
            self.label.clone()
        };
        let mut handle = div()
            .id(self.id)
            .relative()
            .size(px(hit))
            .tooltip(move |window, cx| {
                gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
            })
            .cursor(match state {
                PortState::Rejected | PortState::Disabled => CursorStyle::OperationNotAllowed,
                PortState::Target => CursorStyle::DragLink,
                _ => CursorStyle::Crosshair,
            })
            .child(
                div()
                    .absolute()
                    .left(px((hit - size) * 0.5))
                    .top(px((hit - size) * 0.5))
                    .size(px(size))
                    .rounded_full()
                    .bg(rgb(color).alpha(match state {
                        PortState::Compatible => 0.65,
                        PortState::Disabled => 0.45,
                        _ => 1.,
                    }))
                    .when_else(
                        self.diameter < 9.,
                        |dot| dot.border_1(),
                        |dot| dot.border_2(),
                    )
                    .border_color(rgb(if state == PortState::Target {
                        self.theme.foreground
                    } else {
                        self.theme.background
                    })),
            );
        if let Some(anchor) = self.anchor {
            handle = handle
                .absolute()
                .left(px(anchor.x - hit * 0.5))
                .top(px(anchor.y - hit * 0.5));
        }
        if self.show_label {
            let label = div()
                .absolute()
                .top(px((hit - 12.) * 0.5))
                .text_xs()
                .whitespace_nowrap()
                .text_color(rgb(self.theme.muted))
                .child(self.label);
            handle = handle.child(match self.direction {
                PortDirection::Input => label.left(px(hit + 5.)),
                PortDirection::Output => label.right(px(hit + 5.)),
            });
        }
        if let Some(callback) = self.on_connect {
            handle = handle.on_mouse_down(MouseButton::Left, callback);
        }
        handle
    }
}
