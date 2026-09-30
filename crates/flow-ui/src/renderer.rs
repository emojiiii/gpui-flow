//! Custom renderer extension points. Renderers own the body; the canvas owns drag/ports/selection.
use crate::FlowTheme;
use gpui::*;
use gpui_flow_core::Node;
use std::{collections::BTreeMap, sync::Arc};
#[derive(Clone, Copy)]
pub struct NodeRenderContext {
    pub selected: bool,
    pub zoom: f32,
    pub theme: FlowTheme,
}
pub trait NodeRenderer: Send + Sync {
    fn render(
        &self,
        node: &Node,
        context: NodeRenderContext,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement;
}
#[derive(Default, Clone)]
pub struct NodeRegistry {
    renderers: BTreeMap<String, Arc<dyn NodeRenderer>>,
}
impl NodeRegistry {
    pub fn register(&mut self, kind: impl Into<String>, renderer: impl NodeRenderer + 'static) {
        self.renderers.insert(kind.into(), Arc::new(renderer));
    }
    pub fn get(&self, kind: &str) -> Option<Arc<dyn NodeRenderer>> {
        self.renderers.get(kind).cloned()
    }
}
/// Override edge geometry (e.g. straight, orthogonal, Bezier) without changing the graph.
pub trait EdgeRenderer: Send + Sync {
    fn paint(&self, source: Point<Pixels>, target: Point<Pixels>, color: Rgba, window: &mut Window);
}
pub struct BezierEdge;
impl EdgeRenderer for BezierEdge {
    fn paint(
        &self,
        source: Point<Pixels>,
        target: Point<Pixels>,
        color: Rgba,
        window: &mut Window,
    ) {
        let reach = ((target.x - source.x).abs() * 0.5).max(px(50.));
        let mut path = PathBuilder::stroke(px(2.));
        path.move_to(source);
        path.cubic_bezier_to(
            target,
            point(source.x + reach, source.y),
            point(target.x - reach, target.y),
        );
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }
}
pub struct StraightEdge;
impl EdgeRenderer for StraightEdge {
    fn paint(
        &self,
        source: Point<Pixels>,
        target: Point<Pixels>,
        color: Rgba,
        window: &mut Window,
    ) {
        let mut path = PathBuilder::stroke(px(2.));
        path.move_to(source);
        path.line_to(target);
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }
}
