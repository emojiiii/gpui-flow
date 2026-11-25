//! Custom renderer extension points. Renderers own the body; the canvas owns drag/ports/selection.
use crate::FlowTheme;
use gpui::*;
use gpui_flow_core::Node;
use lyon::geom::{CubicBezierSegment, LineSegment};
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

    /// Return whether the pointer hits this edge in canvas-local logical pixels.
    /// Implement this with the same geometry as `paint` to enable pointer selection.
    fn hit_test(&self, _source: Point<Pixels>, _target: Point<Pixels>, _at: Point<Pixels>) -> bool {
        false
    }
}

const EDGE_HIT_RADIUS: f32 = 6.;

fn lyon_point(p: Point<Pixels>) -> lyon::math::Point {
    lyon::math::point(p.x.into(), p.y.into())
}

pub struct BezierEdge;
impl BezierEdge {
    fn controls(source: Point<Pixels>, target: Point<Pixels>) -> [Point<Pixels>; 2] {
        let reach = ((target.x - source.x).abs() * 0.5).max(px(50.));
        [
            point(source.x + reach, source.y),
            point(target.x - reach, target.y),
        ]
    }
}
impl EdgeRenderer for BezierEdge {
    fn paint(
        &self,
        source: Point<Pixels>,
        target: Point<Pixels>,
        color: Rgba,
        window: &mut Window,
    ) {
        let [first, second] = Self::controls(source, target);
        let mut path = PathBuilder::stroke(px(2.));
        path.move_to(source);
        path.cubic_bezier_to(target, first, second);
        if let Ok(path) = path.build() {
            window.paint_path(path, color);
        }
    }

    fn hit_test(&self, source: Point<Pixels>, target: Point<Pixels>, at: Point<Pixels>) -> bool {
        let [first, second] = Self::controls(source, target);
        let curve = CubicBezierSegment {
            from: lyon_point(source),
            ctrl1: lyon_point(first),
            ctrl2: lyon_point(second),
            to: lyon_point(target),
        };
        let at = lyon_point(at);
        if !curve
            .fast_bounding_box()
            .inflate(EDGE_HIT_RADIUS, EDGE_HIT_RADIUS)
            .contains(at)
        {
            return false;
        }
        let mut from = curve.from;
        curve.flattened(0.5).any(|to| {
            let segment = LineSegment { from, to };
            from = to;
            segment.square_distance_to_point(at) <= EDGE_HIT_RADIUS.powi(2)
        })
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

    fn hit_test(&self, source: Point<Pixels>, target: Point<Pixels>, at: Point<Pixels>) -> bool {
        let segment = LineSegment {
            from: lyon_point(source),
            to: lyon_point(target),
        };
        segment.square_distance_to_point(lyon_point(at)) <= EDGE_HIT_RADIUS.powi(2)
    }
}
