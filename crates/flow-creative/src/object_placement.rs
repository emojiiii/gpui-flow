//! Bounded, read-only placement for objects created without an explicit drop point.
//!
//! Existing world coordinates, frame membership and zoom are never changed. The
//! caller applies the position in its normal editor transaction and pans only
//! after creation succeeds. Explicit drawing/drop gestures should keep their
//! own exact-position behavior.

use gpui_flow_core::{Graph, GraphError, Point, Rect, Size, Viewport, hidden_node_ids};

/// Space between visual bounds, measured in screen pixels at every zoom.
pub const OBJECT_GAP: f32 = 28.0;
const SAFE_MARGIN: f32 = 16.0;
const LEFT_RAIL: f32 = 78.0;
// The floating toolbar clamps at y=62 and is 38px high. Reserve its full
// bottom edge, then leave the normal margin for the object's external caption.
const TOP_TOOLBAR: f32 = 100.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ObjectPlacement {
    pub position: Point,
    /// Screen-pixel delta for `Viewport::pan_by`, not a world-space offset.
    pub pan_delta: Point,
}

/// Find the nearest free origin to `preferred`, favoring fully visible space.
///
/// Visible nodes (including locked nodes) and locked frames are obstacles.
/// Hidden nodes do not consume visual space; unlocked frames remain containers.
/// The unobscured region reserves 78px for the left rail, 100px for the toolbars,
/// and 16px of safe margin on every side. An object too large for that region
/// keeps its size/zoom and reveals its origin on each oversized axis.
///
/// The search visits obstacle edges and merges blocked vertical intervals,
/// rather than walking a grid. With Creative's existing limit of 128 nodes and
/// 128 groups, it takes bounded O(n² log n) work and O(n) scratch space. Exterior
/// obstacle edges are always searched: exhausting visible space cannot silently
/// return an overlap, or miss a distant free side because a search budget ran out.
/// Invalid geometry, capacity overflow, an unusably small canvas, or geometry
/// that cannot be represented/revealed with finite f32 coordinates returns an
/// error. This validates geometry only; the caller still owns graph validation.
pub fn place_object(
    graph: &Graph,
    preferred: Point,
    size: Size,
    viewport: Viewport,
    canvas: Size,
) -> Result<ObjectPlacement, GraphError> {
    if graph.nodes.len() > crate::MAX_CARDS || graph.groups.len() > crate::MAX_CARDS {
        return Err(invalid(
            "object placement supports at most 128 nodes and 128 frames",
        ));
    }
    if !preferred.is_finite()
        || !size.is_positive()
        || !viewport.is_valid()
        || !canvas.is_positive()
    {
        return Err(invalid(
            "object placement needs finite geometry and positive sizes",
        ));
    }
    let screen_size = Size::new(size.width * viewport.zoom, size.height * viewport.zoom);
    if !screen_size.is_positive() {
        return Err(invalid(
            "object size cannot be represented at the current zoom",
        ));
    }
    let safe = Area {
        left: f64::from(LEFT_RAIL + SAFE_MARGIN),
        top: f64::from(TOP_TOOLBAR + SAFE_MARGIN),
        right: f64::from(canvas.width) - f64::from(SAFE_MARGIN),
        bottom: f64::from(canvas.height) - f64::from(SAFE_MARGIN),
    };
    if safe.left >= safe.right || safe.top >= safe.bottom {
        return Err(invalid(
            "canvas is too small to place an object clear of the toolbar and rail",
        ));
    }

    let hidden = hidden_node_ids(graph);
    let gap = f64::from(OBJECT_GAP) / f64::from(viewport.zoom);
    let mut forbidden = Vec::with_capacity(graph.nodes.len() + graph.groups.len());
    for node in graph.nodes.values() {
        validate_bounds(node.bounds())?;
        if !hidden.contains(&node.id) {
            forbidden.push(Area::blocked_origins(node.bounds(), size, gap));
        }
    }
    for group in graph.groups.values() {
        validate_bounds(group.bounds)?;
        if group.locked {
            forbidden.push(Area::blocked_origins(group.bounds, size, gap));
        }
    }
    let graph_bounds = graph.bounds();
    if graph_bounds.is_some_and(|bounds| !bounds.is_valid()) {
        return Err(invalid("aggregate graph bounds overflow"));
    }
    let world = world_domain(size, viewport.zoom, graph_bounds);
    let search = Search {
        preferred,
        size,
        screen_size,
        viewport,
        safe,
        forbidden: &forbidden,
        graph_bounds,
    };

    if f64::from(screen_size.width) <= safe.right - safe.left
        && f64::from(screen_size.height) <= safe.bottom - safe.top
    {
        let zoom = f64::from(viewport.zoom);
        let visible = Area {
            left: (safe.left - f64::from(viewport.pan.x)) / zoom,
            top: (safe.top - f64::from(viewport.pan.y)) / zoom,
            right: (safe.right - f64::from(screen_size.width) - f64::from(viewport.pan.x)) / zoom,
            bottom: (safe.bottom - f64::from(screen_size.height) - f64::from(viewport.pan.y))
                / zoom,
        };
        if let Some(placement) = search.nearest(world.intersection(visible), true) {
            return Ok(placement);
        }
    }

    search.nearest(world, false).ok_or_else(|| {
        invalid("no collision-free object position can be revealed within the supported coordinate range")
    })
}

#[derive(Clone, Copy)]
struct Area {
    left: f64,
    top: f64,
    right: f64,
    bottom: f64,
}

impl Area {
    /// A point strictly inside this rectangle would collide or violate spacing.
    fn blocked_origins(bounds: Rect, size: Size, gap: f64) -> Self {
        Self {
            left: f64::from(bounds.origin.x) - f64::from(size.width) - gap,
            top: f64::from(bounds.origin.y) - f64::from(size.height) - gap,
            right: f64::from(bounds.origin.x) + f64::from(bounds.size.width) + gap,
            bottom: f64::from(bounds.origin.y) + f64::from(bounds.size.height) + gap,
        }
    }

    fn intersection(self, other: Self) -> Self {
        Self {
            left: self.left.max(other.left),
            top: self.top.max(other.top),
            right: self.right.min(other.right),
            bottom: self.bottom.min(other.bottom),
        }
    }

    fn contains_strict(self, point: Point) -> bool {
        f64::from(point.x) > self.left
            && f64::from(point.x) < self.right
            && f64::from(point.y) > self.top
            && f64::from(point.y) < self.bottom
    }
}

fn invalid(message: &str) -> GraphError {
    GraphError::InvalidGeometry(message.into())
}

fn validate_bounds(bounds: Rect) -> Result<(), GraphError> {
    if bounds.is_valid() && bounds.size.is_positive() {
        Ok(())
    } else {
        Err(invalid(
            "existing objects and frames must have finite, positive bounds",
        ))
    }
}

fn world_domain(size: Size, zoom: f32, graph_bounds: Option<Rect>) -> Area {
    let limit = f64::from(f32::MAX);
    // Multiplication happens before pan in Viewport::world_to_screen.
    let projected_limit = limit.min(limit / f64::from(zoom));
    let mut domain = Area {
        left: -projected_limit,
        top: -projected_limit,
        right: projected_limit.min(limit - f64::from(size.width)),
        bottom: projected_limit.min(limit - f64::from(size.height)),
    };
    if let Some(bounds) = graph_bounds {
        // Include hidden objects and unlocked frames in the document's numeric
        // envelope, even though they do not block visible placement.
        domain.left = domain.left.max(f64::from(bounds.max_x()) - limit);
        domain.top = domain.top.max(f64::from(bounds.max_y()) - limit);
        domain.right = domain
            .right
            .min(f64::from(bounds.min_x()) + limit - f64::from(size.width));
        domain.bottom = domain
            .bottom
            .min(f64::from(bounds.min_y()) + limit - f64::from(size.height));
    }
    domain
}

struct Search<'a> {
    preferred: Point,
    size: Size,
    screen_size: Size,
    viewport: Viewport,
    safe: Area,
    forbidden: &'a [Area],
    graph_bounds: Option<Rect>,
}

impl Search<'_> {
    fn nearest(&self, domain: Area, require_visible: bool) -> Option<ObjectPlacement> {
        // Directed rounding preserves both the required gap and safe insets.
        let left = ceil_f32(domain.left);
        let right = floor_f32(domain.right);
        let top = ceil_f32(domain.top);
        let bottom = floor_f32(domain.bottom);
        if ![left, right, top, bottom]
            .iter()
            .all(|value| value.is_finite())
            || left > right
            || top > bottom
        {
            return None;
        }
        let mut xs = Vec::with_capacity(self.forbidden.len() * 4 + 3);
        xs.extend([left, right, self.preferred.x.clamp(left, right)]);
        for area in self.forbidden {
            for edge in [area.left, area.right] {
                for x in [floor_f32(edge), ceil_f32(edge)] {
                    if x >= left && x <= right {
                        xs.push(x);
                    }
                }
            }
        }
        xs.sort_by(|a, b| a.total_cmp(b));
        xs.dedup();
        xs.sort_by(|a, b| {
            (f64::from(*a) - f64::from(self.preferred.x))
                .abs()
                .total_cmp(&(f64::from(*b) - f64::from(self.preferred.x)).abs())
                .then_with(|| b.total_cmp(a))
        });

        let mut best: Option<(f64, ObjectPlacement)> = None;
        let mut intervals = Vec::with_capacity(self.forbidden.len());
        for x in xs {
            let dx = f64::from(x) - f64::from(self.preferred.x);
            if best.is_some_and(|(distance, _)| dx * dx > distance) {
                break;
            }
            intervals.clear();
            for area in self.forbidden {
                if f64::from(x) > area.left && f64::from(x) < area.right {
                    // Between these outward-rounded endpoints every representable
                    // f32 is blocked. Merge them so sub-ULP gaps cannot trap a search.
                    intervals.push((floor_f32(area.top), ceil_f32(area.bottom)));
                }
            }
            let ys = nearest_free_y(&mut intervals, self.preferred.y.clamp(top, bottom));
            for y in ys.into_iter().flatten() {
                if y < top || y > bottom || !y.is_finite() {
                    continue;
                }
                let position = Point::new(x, y);
                let Some(placement) = self.placement(position) else {
                    continue;
                };
                if require_visible && placement.pan_delta != Point::ZERO {
                    continue;
                }
                let dy = f64::from(y) - f64::from(self.preferred.y);
                let distance = dx * dx + dy * dy;
                if best.is_none_or(|(previous, candidate)| {
                    distance < previous
                        || (distance == previous
                            && (x > candidate.position.x
                                || (x == candidate.position.x && y > candidate.position.y)))
                }) {
                    best = Some((distance, placement));
                }
            }
        }
        best.map(|(_, placement)| placement)
    }

    fn placement(&self, position: Point) -> Option<ObjectPlacement> {
        let bounds = Rect::new(position, self.size);
        if !bounds.is_valid()
            || self
                .graph_bounds
                .is_some_and(|existing| !existing.union(bounds).is_valid())
            || self
                .forbidden
                .iter()
                .any(|area| area.contains_strict(position))
        {
            return None;
        }
        let pan_delta = Point::new(
            reveal_axis(
                position.x * self.viewport.zoom,
                self.screen_size.width,
                self.viewport.pan.x,
                self.safe.left,
                self.safe.right,
            )?,
            reveal_axis(
                position.y * self.viewport.zoom,
                self.screen_size.height,
                self.viewport.pan.y,
                self.safe.top,
                self.safe.bottom,
            )?,
        );
        Some(ObjectPlacement {
            position,
            pan_delta,
        })
    }
}

/// The closest available y is either the preferred y, or an endpoint of the
/// merged open interval containing it. Touching intervals retain their free edge.
fn nearest_free_y(intervals: &mut [(f32, f32)], preferred: f32) -> [Option<f32>; 2] {
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    let mut merged: Option<(f32, f32)> = None;
    for &(low, high) in intervals.iter() {
        if let Some((start, end)) = merged {
            if low < end {
                merged = Some((start, end.max(high)));
                continue;
            }
            if preferred > start && preferred < end {
                return [Some(start), Some(end)];
            }
        }
        merged = Some((low, high));
    }
    if let Some((low, high)) = merged
        && preferred > low
        && preferred < high
    {
        return [Some(low), Some(high)];
    }
    [Some(preferred), None]
}

fn floor_f32(value: f64) -> f32 {
    let rounded = value as f32;
    if f64::from(rounded) > value {
        rounded.next_down()
    } else {
        rounded
    }
}

fn ceil_f32(value: f64) -> f32 {
    let rounded = value as f32;
    if f64::from(rounded) < value {
        rounded.next_up()
    } else {
        rounded
    }
}

/// Clamp only the axis that needs revealing. Recheck actual f32 viewport math:
/// very distant positions may have no representable pan that reveals an origin.
fn reveal_axis(projected: f32, extent: f32, pan: f32, low: f64, high: f64) -> Option<f32> {
    if !projected.is_finite() {
        return None;
    }
    let oversized = f64::from(extent) > high - low;
    let last_origin = if oversized {
        low
    } else {
        high - f64::from(extent)
    };
    let displayed = f64::from(projected + pan);
    if displayed >= low && displayed <= last_origin {
        return Some(0.0);
    }
    let current = f64::from(projected) + f64::from(pan);
    let target = current.clamp(low, last_origin);
    let mut delta = (target - current) as f32;
    for _ in 0..4 {
        let next_pan = pan + delta;
        let actual = f64::from(projected + next_pan);
        if !delta.is_finite() || !next_pan.is_finite() || !actual.is_finite() {
            return None;
        }
        // An oversized object exposes its origin as close to the leading safe
        // edge as f32 permits; its trailing edge necessarily remains offscreen.
        let permitted_end = if oversized { high } else { last_origin };
        if actual >= low && actual <= permitted_end {
            return Some(if delta == 0.0 { 0.0 } else { delta });
        }
        let next = if actual < low {
            next_pan.next_up()
        } else {
            next_pan.next_down()
        };
        let corrected = (f64::from(next) - f64::from(pan)) as f32;
        delta = if corrected != delta {
            corrected
        } else if actual < low {
            delta.next_up()
        } else {
            delta.next_down()
        };
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{Group, Node, set_node_visible};

    const CANVAS: Size = Size::new(1200.0, 900.0);
    const CARD: Size = Size::new(220.0, 160.0);

    fn node(graph: &mut Graph, id: &str, position: Point, size: Size) {
        graph
            .add_node(Node::new(id, id, position).with_size(size))
            .unwrap();
    }

    fn visible(placement: ObjectPlacement, size: Size, mut viewport: Viewport, canvas: Size) {
        assert!(viewport.pan_by(placement.pan_delta));
        let bounds = viewport.world_rect_to_screen(Rect::new(placement.position, size));
        assert!(bounds.min_x() >= LEFT_RAIL + SAFE_MARGIN, "{bounds:?}");
        assert!(bounds.min_y() >= TOP_TOOLBAR + SAFE_MARGIN, "{bounds:?}");
        assert!(bounds.max_x() <= canvas.width - SAFE_MARGIN, "{bounds:?}");
        assert!(bounds.max_y() <= canvas.height - SAFE_MARGIN, "{bounds:?}");
    }

    fn separated(a: Rect, b: Rect, zoom: f32) {
        let gap = f64::from(OBJECT_GAP) / f64::from(zoom);
        assert!(
            !Area::blocked_origins(a, b.size, gap).contains_strict(b.origin),
            "{a:?} versus {b:?}"
        );
    }

    #[test]
    fn free_visible_preference_is_stable_and_read_only() {
        let mut graph = Graph::new();
        node(&mut graph, "existing", Point::new(900.0, 600.0), CARD);
        let before = graph.clone();
        let preferred = Point::new(250.0, 240.0);
        let result = place_object(&graph, preferred, CARD, Viewport::default(), CANVAS).unwrap();
        assert_eq!(
            result,
            ObjectPlacement {
                position: preferred,
                pan_delta: Point::ZERO
            }
        );
        assert_eq!(graph, before);
        assert_eq!(
            place_object(&graph, preferred, CARD, Viewport::default(), CANVAS).unwrap(),
            result
        );
    }

    #[test]
    fn repeated_insertions_never_overlap_and_preserve_existing_positions() {
        for zoom in [0.5, 1.0, 2.0] {
            let mut graph = Graph::new();
            let mut viewport = Viewport {
                zoom,
                pan: Point::new(43.0, -31.0),
                ..Viewport::default()
            };
            for index in 0..128 {
                let preferred = viewport.screen_to_world(Point::new(350.0, 260.0));
                let before = graph.clone();
                let result = place_object(&graph, preferred, CARD, viewport, CANVAS).unwrap();
                for existing in graph.nodes.values() {
                    separated(existing.bounds(), Rect::new(result.position, CARD), zoom);
                }
                visible(result, CARD, viewport, CANVAS);
                assert_eq!(graph, before);
                assert!(viewport.pan_by(result.pan_delta));
                assert_eq!(viewport.zoom, zoom);
                node(&mut graph, &format!("n{index}"), result.position, CARD);
                graph.validate().unwrap();
            }
        }
    }

    #[test]
    fn visible_space_wins_over_closer_offscreen_space() {
        let mut graph = Graph::new();
        node(&mut graph, "obstacle", Point::new(900.0, 250.0), CARD);
        let result = place_object(
            &graph,
            Point::new(1000.0, 250.0),
            CARD,
            Viewport::default(),
            CANVAS,
        )
        .unwrap();
        assert_eq!(result.pan_delta, Point::ZERO);
        visible(result, CARD, Viewport::default(), CANVAS);
        separated(
            graph.nodes[&"obstacle".into()].bounds(),
            Rect::new(result.position, CARD),
            1.0,
        );
    }

    #[test]
    fn adjacent_space_reveals_with_screen_pixel_pan_at_half_and_double_zoom() {
        for zoom in [0.5, 2.0] {
            let viewport = Viewport {
                zoom,
                pan: Point::new(37.0, -29.0),
                ..Viewport::default()
            };
            let canvas = Size::new(510.0, 414.0);
            let size = Size::new(400.0 / zoom, 280.0 / zoom);
            let preferred = viewport.screen_to_world(Point::new(94.0, 116.0));
            let mut graph = Graph::new();
            node(&mut graph, "fills_view", preferred, size);
            let result = place_object(&graph, preferred, size, viewport, canvas).unwrap();
            // The shortest escape is below the old object: 280px + 28px gap.
            assert_eq!(result.position.x, preferred.x);
            assert!(((result.position.y - preferred.y) * zoom - 308.0).abs() < 0.001);
            assert_eq!(result.pan_delta, Point::new(0.0, -306.0));
            visible(result, size, viewport, canvas);
        }
    }

    #[test]
    fn hidden_nodes_do_not_block_but_locked_nodes_and_frames_do() {
        let preferred = Point::new(240.0, 220.0);
        let mut graph = Graph::new();
        node(&mut graph, "object", preferred, CARD);
        graph.nodes.get_mut(&"object".into()).unwrap().locked = true;
        set_node_visible(&mut graph, &"object".into(), false).unwrap();
        let free = place_object(&graph, preferred, CARD, Viewport::default(), CANVAS).unwrap();
        assert_eq!(free.position, preferred);
        set_node_visible(&mut graph, &"object".into(), true).unwrap();
        let blocked = place_object(&graph, preferred, CARD, Viewport::default(), CANVAS).unwrap();
        separated(
            graph.nodes[&"object".into()].bounds(),
            Rect::new(blocked.position, CARD),
            1.0,
        );
        set_node_visible(&mut graph, &"object".into(), false).unwrap();
        let mut group = Group::new(
            "frame",
            "Frame",
            Rect::new(Point::new(180.0, 160.0), Size::new(500.0, 400.0)),
        );
        graph.add_group(group.clone()).unwrap();
        assert_eq!(
            place_object(&graph, preferred, CARD, Viewport::default(), CANVAS)
                .unwrap()
                .position,
            preferred
        );
        group.locked = true;
        graph.groups.insert(group.id.clone(), group.clone());
        let blocked = place_object(&graph, preferred, CARD, Viewport::default(), CANVAS).unwrap();
        separated(group.bounds, Rect::new(blocked.position, CARD), 1.0);
    }

    #[test]
    fn oversized_object_aligns_reachable_origin_without_changing_zoom() {
        let viewport = Viewport {
            zoom: 2.0,
            pan: Point::new(50.0, -30.0),
            ..Viewport::default()
        };
        let preferred = Point::new(600.0, 500.0);
        let size = Size::new(1800.0, 1300.0);
        let result = place_object(&Graph::new(), preferred, size, viewport, CANVAS).unwrap();
        assert_eq!(result.position, preferred);
        assert_eq!(result.pan_delta, Point::new(-1156.0, -854.0));
        let mut revealed = viewport;
        assert!(revealed.pan_by(result.pan_delta));
        assert_eq!(revealed.zoom, viewport.zoom);
        assert_eq!(
            revealed.world_to_screen(result.position),
            Point::new(94.0, 116.0)
        );
    }

    #[test]
    fn dense_canvas_keeps_global_escape_sides_and_accepts_separate_capacity_limits() {
        let mut graph = Graph::new();
        for index in 0..128 {
            node(
                &mut graph,
                &format!("n{index}"),
                Point::new(-5000.0, -5000.0),
                Size::new(10000.0, 10000.0),
            );
            let mut group = Group::new(
                format!("g{index}"),
                "Frame",
                Rect::new(Point::new(-6000.0, -6000.0), Size::new(12000.0, 12000.0)),
            );
            group.locked = true;
            graph.groups.insert(group.id.clone(), group);
        }
        let result = place_object(
            &graph,
            Point::new(300.0, 250.0),
            CARD,
            Viewport::default(),
            CANVAS,
        )
        .unwrap();
        assert_eq!(result.position, Point::new(6028.0, 250.0));
        visible(result, CARD, Viewport::default(), CANVAS);
        for group in graph.groups.values() {
            separated(group.bounds, Rect::new(result.position, CARD), 1.0);
        }
    }

    #[test]
    fn far_away_graph_does_not_pull_new_object_out_of_view() {
        let mut graph = Graph::new();
        node(&mut graph, "distant", Point::new(1.0e20, -1.0e20), CARD);
        let preferred = Point::new(300.0, 200.0);
        let result = place_object(&graph, preferred, CARD, Viewport::default(), CANVAS).unwrap();
        assert_eq!(
            result,
            ObjectPlacement {
                position: preferred,
                pan_delta: Point::ZERO
            }
        );
    }

    #[test]
    fn invalid_and_unrepresentable_geometry_is_rejected_without_mutation() {
        let graph = Graph::new();
        for (preferred, size, viewport, canvas) in [
            (Point::new(f32::NAN, 0.0), CARD, Viewport::default(), CANVAS),
            (
                Point::ZERO,
                Size::new(0.0, 5.0),
                Viewport::default(),
                CANVAS,
            ),
            (
                Point::ZERO,
                CARD,
                Viewport {
                    zoom: 0.0,
                    ..Viewport::default()
                },
                CANVAS,
            ),
            (
                Point::ZERO,
                CARD,
                Viewport::default(),
                Size::new(110.0, 500.0),
            ),
            (
                Point::ZERO,
                CARD,
                Viewport::default(),
                Size::new(500.0, 108.0),
            ),
            (
                Point::ZERO,
                Size::new(f32::MAX, 100.0),
                Viewport {
                    zoom: 2.0,
                    ..Viewport::default()
                },
                CANVAS,
            ),
        ] {
            assert!(place_object(&graph, preferred, size, viewport, canvas).is_err());
            assert_eq!(graph, Graph::new());
        }
        let mut malformed = Graph::new();
        let invalid_node = Node::new("bad", "Bad", Point::new(f32::INFINITY, 0.0));
        malformed
            .nodes
            .insert(invalid_node.id.clone(), invalid_node);
        assert!(place_object(&malformed, Point::ZERO, CARD, Viewport::default(), CANVAS).is_err());
        let mut over_limit = Graph::new();
        for index in 0..129 {
            node(&mut over_limit, &format!("n{index}"), Point::ZERO, CARD);
        }
        let before = over_limit.clone();
        assert!(place_object(&over_limit, Point::ZERO, CARD, Viewport::default(), CANVAS).is_err());
        assert_eq!(over_limit, before);
    }

    #[test]
    fn no_representable_escape_reports_an_error_instead_of_overlap() {
        let mut graph = Graph::new();
        let mut frame = Group::new(
            "huge",
            "Huge",
            Rect::new(
                Point::new(-f32::MAX / 2.0, -f32::MAX / 2.0),
                Size::new(f32::MAX, f32::MAX),
            ),
        );
        frame.locked = true;
        graph.add_group(frame).unwrap();
        let before = graph.clone();
        assert!(
            place_object(
                &graph,
                Point::new(300.0, 200.0),
                CARD,
                Viewport::default(),
                CANVAS
            )
            .is_err()
        );
        assert_eq!(graph, before);
    }

    #[test]
    fn touching_blocked_intervals_keep_their_exact_free_edge() {
        let mut intervals = [(0.0, 10.0), (10.0, 20.0), (-20.0, -10.0)];
        assert_eq!(nearest_free_y(&mut intervals, 10.0), [Some(10.0), None]);
        assert_eq!(nearest_free_y(&mut intervals, 5.0), [Some(0.0), Some(10.0)]);
        let mut overlapping = [(0.0, 12.0), (10.0, 20.0)];
        assert_eq!(
            nearest_free_y(&mut overlapping, 11.0),
            [Some(0.0), Some(20.0)]
        );
    }
}
