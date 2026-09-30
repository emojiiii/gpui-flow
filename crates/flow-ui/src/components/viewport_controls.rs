//! Viewport action strip; the host owns and mutates the viewport.
use super::ControlButton;
use crate::FlowTheme;
use gpui::{prelude::*, *};
use std::rc::Rc;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ViewportAction {
    ZoomOut,
    Reset,
    ZoomIn,
    Fit,
}
type ActionCallback = Rc<dyn Fn(&ViewportAction, &mut Window, &mut App)>;
#[derive(IntoElement)]
pub struct ViewportControls {
    pub id: ElementId,
    pub zoom: f32,
    pub theme: FlowTheme,
    on_action: Option<ActionCallback>,
}
impl ViewportControls {
    pub fn new(id: impl Into<ElementId>, zoom: f32) -> Self {
        Self {
            id: id.into(),
            zoom,
            theme: FlowTheme::default(),
            on_action: None,
        }
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    pub fn on_action(
        mut self,
        callback: impl Fn(&ViewportAction, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_action = Some(Rc::new(callback));
        self
    }
}
impl RenderOnce for ViewportControls {
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let mut controls = div().id(self.id).flex().gap_2();
        for (id, label, action) in [
            ("zoom-out", "−".to_string(), ViewportAction::ZoomOut),
            (
                "zoom-reset",
                format!("{:.0}%", super::valid_zoom(self.zoom) * 100.),
                ViewportAction::Reset,
            ),
            ("zoom-in", "+".into(), ViewportAction::ZoomIn),
            ("zoom-fit", "Fit".into(), ViewportAction::Fit),
        ] {
            let mut button = ControlButton::new(id, label)
                .theme(self.theme)
                .disabled(self.on_action.is_none());
            if let Some(callback) = self.on_action.clone() {
                button = button.on_click(move |_, window, cx| callback(&action, window, cx));
            }
            controls = controls.child(button);
        }
        controls
    }
}
