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
    /// Opt into a media-first object: content fills the node, with its caption
    /// outside the top edge. Existing renderers keep the framed presentation.
    fn full_bleed(&self) -> bool {
        false
    }
    /// Draw content directly on the canvas. Selection and resize affordances
    /// remain visible; existing renderers retain their normal background.
    fn transparent_background(&self, _node: &Node) -> bool {
        false
    }
    /// Override the external media caption. Empty text hides it; None preserves
    /// the frame's title/geometry caption for existing renderers.
    fn object_caption(&self, _node: &Node) -> Option<SharedString> {
        None
    }
    /// Opt into exact logical geometry with no body inset or clipping. The
    /// canvas paints selection as an overlay. Existing card renderers keep
    /// their frame and clipped body.
    fn unframed_body(&self, _node: &Node) -> bool {
        false
    }
    /// Optional body picking in node-local world units. None preserves the
    /// default rectangular hit area; ports and resize controls stay separate.
    fn body_hit_test(
        &self,
        _node: &Node,
        _local: gpui_flow_core::Point,
        _zoom: f32,
    ) -> Option<bool> {
        None
    }
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

    /// Pick the painted edge in window logical pixels. The endpoints match those
    /// passed to `paint`. Paint-only renderers can use the default.
    fn hit_test(&self, _source: Point<Pixels>, _target: Point<Pixels>, _at: Point<Pixels>) -> bool {
        false
    }
}

const EDGE_HIT_RADIUS: f32 = 6.;

fn lyon_edge_point(point: Point<Pixels>) -> lyon::math::Point {
    lyon::math::point(point.x.into(), point.y.into())
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
            from: lyon_edge_point(source),
            ctrl1: lyon_edge_point(first),
            ctrl2: lyon_edge_point(second),
            to: lyon_edge_point(target),
        };
        let at = lyon_edge_point(at);
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
            from: lyon_edge_point(source),
            to: lyon_edge_point(target),
        };
        segment.square_distance_to_point(lyon_edge_point(at)) <= EDGE_HIT_RADIUS.powi(2)
    }
}
