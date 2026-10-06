//! Standalone graph overview with world-coordinate navigation callbacks.
use crate::FlowTheme;
use gpui::{prelude::*, *};
use gpui_flow_core as model;
use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, Debug)]
pub struct MiniMapNode {
    pub bounds: model::Rect,
    pub color: u32,
    pub selected: bool,
}
impl MiniMapNode {
    pub fn new(bounds: model::Rect, color: u32) -> Self {
        Self {
            bounds,
            color,
            selected: false,
        }
    }
    pub fn selected(mut self, value: bool) -> Self {
        self.selected = value;
        self
    }
}
type NavigateCallback = Rc<dyn Fn(&model::Point, &mut Window, &mut App)>;

#[derive(Default)]
struct NavigationState {
    drag: Option<(model::MinimapTransform, model::Point)>,
    _activation: Option<Subscription>,
}
impl NavigationState {
    fn target(&self, position: model::Point) -> Option<model::Point> {
        let (map, origin) = self.drag?;
        if !position.is_finite() {
            return None;
        }
        let local = position - origin;
        Some(map.map_to_world(model::Point::new(
            local.x.clamp(
                map.map_bounds.origin.x,
                map.map_bounds.origin.x + map.map_bounds.size.width,
            ),
            local.y.clamp(
                map.map_bounds.origin.y,
                map.map_bounds.origin.y + map.map_bounds.size.height,
            ),
        )))
    }
    fn finish(&mut self) -> bool {
        self.drag.take().is_some()
    }
}
#[derive(IntoElement)]
pub struct MiniMap {
    pub id: ElementId,
    pub nodes: Vec<MiniMapNode>,
    pub visible: model::Rect,
    pub size: model::Size,
    pub theme: FlowTheme,
    pub padding: f32,
    on_navigate: Option<NavigateCallback>,
}
impl MiniMap {
    pub fn new(id: impl Into<ElementId>, visible: model::Rect) -> Self {
        Self {
            id: id.into(),
            visible,
            nodes: vec![],
            size: model::Size::new(190., 122.),
            theme: FlowTheme::default(),
            padding: 8.,
            on_navigate: None,
        }
    }
    pub fn node(mut self, value: MiniMapNode) -> Self {
        self.nodes.push(value);
        self
    }
    pub fn nodes(mut self, value: impl IntoIterator<Item = MiniMapNode>) -> Self {
        self.nodes.extend(value);
        self
    }
    pub fn size(mut self, value: model::Size) -> Self {
        self.size = value;
        self
    }
    pub fn theme(mut self, value: FlowTheme) -> Self {
        self.theme = value;
        self
    }
    pub fn padding(mut self, value: f32) -> Self {
        self.padding = value;
        self
    }
    /// Receive a world point; e.g. center the host viewport on it.
    pub fn on_navigate(
        mut self,
        callback: impl Fn(&model::Point, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_navigate = Some(Rc::new(callback));
        self
    }
    pub fn transform(&self) -> Option<model::MinimapTransform> {
        let bounds = self
            .nodes
            .iter()
            .filter(|n| n.bounds.is_valid())
            .fold(self.visible, |bounds, node| bounds.union(node.bounds));
        model::MinimapTransform::new(
            bounds,
            model::Rect::new(model::Point::ZERO, self.size),
            self.padding,
        )
    }
}
impl RenderOnce for MiniMap {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let navigation =
            window.use_keyed_state((self.id.clone(), "navigation"), cx, |window, cx| {
                let subscription = cx.observe_window_activation(
                    window,
                    |this: &mut NavigationState, window, cx| {
                        if !window.is_window_active() && this.finish() {
                            cx.notify();
                        }
                    },
                );
                NavigationState {
                    drag: None,
                    _activation: Some(subscription),
                }
            });
        // Freeze the transform for the gesture so viewport-induced re-renders
        // cannot change the mapping underneath the pointer.
        let map = navigation
            .read(cx)
            .drag
            .map(|(map, _)| map)
            .or_else(|| self.transform());
        let theme = self.theme;
        let origin = Rc::new(Cell::new(model::Point::ZERO));
        let paint_origin = origin.clone();
        let mut view = div()
            .id(self.id)
            .occlude()
            .relative()
            .w(px(self.size.width))
            .h(px(self.size.height))
            .bg(rgb(theme.panel).alpha(0.95))
            .border_1()
            .border_color(rgb(theme.border))
            .rounded_lg()
            .overflow_hidden();
        if let Some(map) = map {
            let visible = self.visible;
            let nodes = self.nodes;
            view = view.child(
                canvas(
                    move |bounds, _, _| {
                        paint_origin.set(model::Point::new(
                            bounds.origin.x.into(),
                            bounds.origin.y.into(),
                        ));
                    },
                    move |bounds, _, window, _| {
                        for node in &nodes {
                            if !node.bounds.is_valid() {
                                continue;
                            }
                            let rect = map.world_rect_to_map(node.bounds);
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin + point(px(rect.origin.x), px(rect.origin.y)),
                                    size(px(rect.size.width.max(1.)), px(rect.size.height.max(1.))),
                                ),
                                rgb(if node.selected {
                                    theme.accent
                                } else {
                                    node.color
                                })
                                .alpha(0.7),
                            ));
                        }
                        let r = map.world_rect_to_map(visible);
                        // A translucent viewport plus outline stays readable on dense graphs.
                        let b = Bounds::new(
                            bounds.origin + point(px(r.origin.x), px(r.origin.y)),
                            size(px(r.size.width), px(r.size.height)),
                        );
                        window.paint_quad(fill(b, rgb(theme.accent).alpha(0.1)));
                        for (x, y, w, h) in [
                            (r.origin.x, r.origin.y, r.size.width, 1.),
                            (
                                r.origin.x,
                                r.origin.y + r.size.height - 1.,
                                r.size.width,
                                1.,
                            ),
                            (r.origin.x, r.origin.y, 1., r.size.height),
                            (
                                r.origin.x + r.size.width - 1.,
                                r.origin.y,
                                1.,
                                r.size.height,
                            ),
                        ] {
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin + point(px(x), px(y)),
                                    size(px(w.max(0.)), px(h.max(0.))),
                                ),
                                rgb(theme.accent),
                            ));
                        }
                    },
                )
                .absolute()
                .size_full(),
            );
            if let Some(callback) = self.on_navigate {
                let down_state = navigation.clone();
                let down_callback = callback.clone();
                let move_state = navigation.clone();
                let move_callback = callback.clone();
                let up_state = navigation.clone();
                view = view
                    .cursor_pointer()
                    .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                        cx.stop_propagation();
                        let target = down_state.update(cx, |state, _| {
                            state.drag = Some((map, origin.get()));
                            state.target(model::Point::new(
                                event.position.x.into(),
                                event.position.y.into(),
                            ))
                        });
                        if let Some(target) = target {
                            down_callback(&target, window, cx);
                        }
                    })
                    .on_mouse_move(move |event, window, cx| {
                        let target = move_state.update(cx, |state, cx| {
                            if event.pressed_button != Some(MouseButton::Left) {
                                if state.finish() {
                                    cx.notify();
                                }
                                return None;
                            }
                            state.target(model::Point::new(
                                event.position.x.into(),
                                event.position.y.into(),
                            ))
                        });
                        if let Some(target) = target {
                            cx.stop_propagation();
                            move_callback(&target, window, cx);
                        }
                    })
                    .on_mouse_up(MouseButton::Left, move |event, window, cx| {
                        let target = up_state.update(cx, |state, cx| {
                            let target = state.target(model::Point::new(
                                event.position.x.into(),
                                event.position.y.into(),
                            ));
                            if state.finish() {
                                cx.notify();
                            }
                            target
                        });
                        if let Some(target) = target {
                            cx.stop_propagation();
                            callback(&target, window, cx);
                        }
                    })
                    .on_mouse_up_out(MouseButton::Left, move |_, _, cx| {
                        navigation.update(cx, |state, cx| {
                            if state.finish() {
                                cx.notify();
                            }
                        });
                    });
            } else {
                view = view.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
            }
        }
        view
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ::core::prelude::v1::test;

    #[test]
    fn drag_keeps_its_mapping_across_updates_and_clears_on_release() {
        let map = model::MinimapTransform::new(
            model::Rect::new(
                model::Point::new(-500., -100.),
                model::Size::new(2000., 1000.),
            ),
            model::Rect::new(model::Point::ZERO, model::Size::new(200., 100.)),
            0.,
        )
        .unwrap();
        let mut state = NavigationState {
            drag: Some((map, model::Point::new(30., 40.))),
            ..Default::default()
        };
        assert_eq!(
            state.target(model::Point::new(80., 90.)),
            Some(model::Point::new(0., 400.))
        );
        assert_eq!(
            state.target(model::Point::new(180., 90.)),
            Some(model::Point::new(1000., 400.))
        );
        assert_eq!(
            state.target(model::Point::new(1000., 90.)),
            Some(model::Point::new(1500., 400.))
        );
        assert!(state.target(model::Point::new(f32::NAN, 90.)).is_none());
        assert!(state.finish());
        assert!(!state.finish());
        assert!(state.target(model::Point::new(180., 90.)).is_none());
    }
}
