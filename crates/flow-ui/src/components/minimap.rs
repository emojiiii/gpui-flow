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
type NavigateCallback = Box<dyn Fn(&model::Point, &mut Window, &mut App)>;
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
        self.on_navigate = Some(Box::new(callback));
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
    fn render(self, _: &mut Window, _: &mut App) -> impl IntoElement {
        let map = self.transform();
        let theme = self.theme;
        let origin = Rc::new(Cell::new(model::Point::ZERO));
        let paint_origin = origin.clone();
        let mut view = div()
            .id(self.id)
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
                view = view.cursor_pointer().on_mouse_down(
                    MouseButton::Left,
                    move |event, window, cx| {
                        cx.stop_propagation();
                        let local =
                            model::Point::new(event.position.x.into(), event.position.y.into())
                                - origin.get();
                        callback(&map.map_to_world(local), window, cx);
                    },
                );
            } else {
                view = view.on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());
            }
        }
        view
    }
}
