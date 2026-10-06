//! Pure geometry for placing text on a canvas. All inputs are canvas-local
//! screen coordinates or world coordinates, never window coordinates.
//!
//! A draft captures the viewport at pointer-down and produces preview bounds
//! without touching a graph, selection or history. Dropping it on Escape is
//! sufficient to cancel. The UI owns pointer capture and the final transaction.

use gpui_flow_core::{Graph, GraphError, Group, GroupId, Point, Rect, Size, Viewport};

pub const DEFAULT_TEXT_SIZE: Size = Size::new(360.0, 120.0);
pub const MIN_TEXT_SIZE: Size = Size::new(24.0, 32.0);
pub const MAX_TEXT_SIZE: f32 = 16_384.0;
/// A displacement strictly below this screen-pixel distance is a click.
pub const TEXT_DRAG_THRESHOLD: f32 = 3.0;

/// Transient, immutable placement state. Keep it outside the document and drop
/// it on cancellation. The viewport snapshot keeps previews and the eventual
/// placement consistent; the UI may cancel if its canvas transform changes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextPlacementDraft {
    start_screen: Point,
    viewport: Viewport,
}

impl TextPlacementDraft {
    pub fn new(start_screen: Point, viewport: Viewport) -> Result<Self, GraphError> {
        checked_world_point(start_screen, viewport)?;
        Ok(Self {
            start_screen,
            viewport,
        })
    }

    /// Preview and commit use the same calculation. No document is modified.
    pub fn bounds(self, end_screen: Point) -> Result<Rect, GraphError> {
        text_bounds_from_screen_points(self.start_screen, end_screen, self.viewport)
    }

    pub fn start_screen(self) -> Point {
        self.start_screen
    }

    pub fn viewport(self) -> Viewport {
        self.viewport
    }
}

/// A click places a 360 by 120 world-unit rectangle at pointer-down. A drag
/// normalizes both axes, then expands short axes toward positive coordinates
/// to the 24 by 32 minimum. Larger-than-maximum bounds are rejected, not clipped.
///
/// The click threshold is Euclidean distance in screen pixels, independent of
/// zoom. Subtract the canvas's window origin before passing either pointer.
pub fn text_bounds_from_screen_points(
    start_screen: Point,
    end_screen: Point,
    viewport: Viewport,
) -> Result<Rect, GraphError> {
    let start = checked_world_point(start_screen, viewport)?;
    let end = checked_world_point(end_screen, viewport)?;
    let distance = (f64::from(end_screen.x) - f64::from(start_screen.x))
        .hypot(f64::from(end_screen.y) - f64::from(start_screen.y));
    if distance < f64::from(TEXT_DRAG_THRESHOLD) {
        default_text_bounds(start)
    } else {
        text_bounds_from_points(start, end)
    }
}

pub fn default_text_bounds(origin: Point) -> Result<Rect, GraphError> {
    validate_bounds(Rect::new(origin, DEFAULT_TEXT_SIZE))
}

/// World-coordinate drag geometry, without click detection. Both endpoints
/// remain inside the result, including horizontal, vertical and reverse drags.
pub fn text_bounds_from_points(start: Point, end: Point) -> Result<Rect, GraphError> {
    // Check first: f32::min/max can otherwise discard a NaN endpoint.
    if !start.is_finite() || !end.is_finite() {
        return Err(invalid_bounds());
    }
    let width = (f64::from(end.x) - f64::from(start.x)).abs();
    let height = (f64::from(end.y) - f64::from(start.y)).abs();
    if width > f64::from(MAX_TEXT_SIZE) || height > f64::from(MAX_TEXT_SIZE) {
        return Err(invalid_bounds());
    }
    validate_bounds(Rect::new(
        Point::new(start.x.min(end.x), start.y.min(end.y)),
        Size::new(
            width.max(f64::from(MIN_TEXT_SIZE.width)) as f32,
            height.max(f64::from(MIN_TEXT_SIZE.height)) as f32,
        ),
    ))
}

/// Find an unambiguous containing Frame without changing membership. Creative
/// renders every core group as a Frame, including legacy groups without its
/// metadata marker. Core Frames are always visible: hiding their child nodes
/// does not hide the Frame rectangle. Hosts that hide Frames independently
/// should pass only their visible Frames to `containing_text_frame_in` instead.
pub fn containing_text_frame(graph: &Graph, bounds: Rect) -> Option<GroupId> {
    containing_text_frame_in(graph.groups.values(), bounds)
}

/// The caller supplies visible Frames. Full containment includes touching an
/// edge. Select only a single containing Frame, and only if it is unlocked.
/// Two containing Frames are ambiguous even when one is locked; choosing by
/// map order, size or lock status could silently assign unintended ownership.
/// Partial overlaps and malformed Frame bounds do not qualify.
pub fn containing_text_frame_in<'a>(
    visible_frames: impl IntoIterator<Item = &'a Group>,
    bounds: Rect,
) -> Option<GroupId> {
    validate_bounds(bounds).ok()?;
    let mut candidates = visible_frames.into_iter().filter(|frame| {
        frame.bounds.is_valid()
            && frame.bounds.size.is_positive()
            && frame.bounds.contains_rect(bounds)
    });
    let frame = candidates.next()?;
    if frame.locked || candidates.next().is_some() {
        return None;
    }
    Some(frame.id.clone())
}

fn checked_world_point(screen: Point, viewport: Viewport) -> Result<Point, GraphError> {
    if !viewport.is_valid() {
        return Err(GraphError::InvalidGeometry(
            "Text placement requires a finite, valid viewport".into(),
        ));
    }
    if !screen.is_finite() {
        return Err(invalid_bounds());
    }
    // Widen intermediate subtraction/division so finite inputs cannot overflow
    // before conversion. Reject a result outside the core coordinate domain.
    let world = Point::new(
        ((f64::from(screen.x) - f64::from(viewport.pan.x)) / f64::from(viewport.zoom)) as f32,
        ((f64::from(screen.y) - f64::from(viewport.pan.y)) / f64::from(viewport.zoom)) as f32,
    );
    if !world.is_finite() {
        return Err(invalid_bounds());
    }
    Ok(world)
}

fn validate_bounds(bounds: Rect) -> Result<Rect, GraphError> {
    if !bounds.is_valid()
        || !(MIN_TEXT_SIZE.width..=MAX_TEXT_SIZE).contains(&bounds.size.width)
        || !(MIN_TEXT_SIZE.height..=MAX_TEXT_SIZE).contains(&bounds.size.height)
        // Finite, extreme origins can absorb a positive size through rounding.
        // Such a rectangle would have collapsed hit-test/containment geometry.
        || bounds.max_x() <= bounds.min_x()
        || bounds.max_y() <= bounds.min_y()
    {
        return Err(invalid_bounds());
    }
    Ok(bounds)
}

fn invalid_bounds() -> GraphError {
    GraphError::InvalidGeometry(
        "Text bounds must be finite and representable, with width in 24..=16384 and height in 32..=16384"
            .into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{Node, set_node_visible};

    fn viewport(zoom: f32) -> Viewport {
        Viewport {
            pan: Point::new(80.0, -40.0),
            zoom,
            ..Viewport::default()
        }
    }

    fn frame(id: &str, bounds: Rect) -> Group {
        Group::new(id, "Frame", bounds)
    }

    fn text_bounds() -> Rect {
        Rect::new(Point::new(50.0, 60.0), Size::new(100.0, 40.0))
    }

    fn frame_bounds() -> Rect {
        Rect::new(Point::ZERO, Size::new(150.0, 100.0))
    }

    #[test]
    fn clicks_keep_default_world_size_under_pan_and_zoom() {
        for zoom in [0.5, 2.0] {
            let viewport = viewport(zoom);
            let origin = Point::new(-25.0, 90.0);
            let start = viewport.world_to_screen(origin);
            for delta in [Point::ZERO, Point::new(2.9, 0.0), Point::new(2.0, 2.0)] {
                assert_eq!(
                    text_bounds_from_screen_points(start, start + delta, viewport).unwrap(),
                    Rect::new(origin, DEFAULT_TEXT_SIZE)
                );
            }
        }
    }

    #[test]
    fn threshold_is_three_screen_pixels_including_diagonal_distance() {
        for zoom in [0.5, 2.0] {
            let viewport = viewport(zoom);
            let start = viewport.pan;
            for delta in [Point::new(3.0, 0.0), Point::new(2.2, 2.2)] {
                assert_eq!(
                    text_bounds_from_screen_points(start, start + delta, viewport).unwrap(),
                    Rect::new(Point::ZERO, MIN_TEXT_SIZE)
                );
            }
        }
    }

    #[test]
    fn screen_drags_map_to_world_at_fifty_and_two_hundred_percent() {
        for zoom in [0.5, 2.0] {
            let viewport = viewport(zoom);
            let start = Point::new(100.0, 80.0);
            let end = Point::new(300.0, 180.0);
            assert_eq!(
                text_bounds_from_screen_points(start, end, viewport).unwrap(),
                Rect::new(
                    Point::new(20.0 / zoom, 120.0 / zoom),
                    Size::new(200.0 / zoom, 100.0 / zoom),
                )
            );
        }
    }

    #[test]
    fn drags_normalize_all_four_directions() {
        let expected = Rect::new(Point::new(-50.0, -10.0), Size::new(250.0, 90.0));
        for (start, end) in [
            (Point::new(-50.0, -10.0), Point::new(200.0, 80.0)),
            (Point::new(200.0, 80.0), Point::new(-50.0, -10.0)),
            (Point::new(-50.0, 80.0), Point::new(200.0, -10.0)),
            (Point::new(200.0, -10.0), Point::new(-50.0, 80.0)),
        ] {
            assert_eq!(text_bounds_from_points(start, end).unwrap(), expected);
        }
    }

    #[test]
    fn short_axes_expand_without_losing_either_pointer() {
        for (start, end, size) in [
            (Point::ZERO, Point::new(100.0, 0.0), Size::new(100.0, 32.0)),
            (Point::ZERO, Point::new(0.0, -100.0), Size::new(24.0, 100.0)),
            (Point::new(10.0, 10.0), Point::ZERO, MIN_TEXT_SIZE),
            (Point::ZERO, Point::ZERO, MIN_TEXT_SIZE),
        ] {
            let bounds = text_bounds_from_points(start, end).unwrap();
            assert_eq!(bounds.size, size);
            assert!(bounds.contains(start));
            assert!(bounds.contains(end));
        }
    }

    #[test]
    fn maximum_size_is_allowed_and_larger_drags_are_rejected() {
        assert_eq!(
            text_bounds_from_points(Point::ZERO, Point::new(MAX_TEXT_SIZE, MAX_TEXT_SIZE))
                .unwrap()
                .size,
            Size::new(MAX_TEXT_SIZE, MAX_TEXT_SIZE)
        );
        for end in [
            Point::new(MAX_TEXT_SIZE + 1.0, 100.0),
            Point::new(100.0, -MAX_TEXT_SIZE - 1.0),
            Point::new(f32::MAX, f32::MAX),
        ] {
            assert!(text_bounds_from_points(Point::ZERO, end).is_err());
        }
        assert!(
            text_bounds_from_points(Point::new(-f32::MAX, 0.0), Point::new(f32::MAX, 0.0)).is_err()
        );
        assert!(
            text_bounds_from_screen_points(Point::ZERO, Point::new(10_000.0, 0.0), viewport(0.5),)
                .is_err()
        );
    }

    #[test]
    fn nonfinite_points_and_unrepresentable_bounds_are_rejected() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            for point in [Point::new(value, 0.0), Point::new(0.0, value)] {
                assert!(default_text_bounds(point).is_err());
                assert!(text_bounds_from_points(point, Point::ZERO).is_err());
                assert!(text_bounds_from_points(Point::ZERO, point).is_err());
                assert!(TextPlacementDraft::new(point, Viewport::default()).is_err());
                assert!(
                    text_bounds_from_screen_points(Point::ZERO, point, Viewport::default())
                        .is_err()
                );
            }
        }
        for point in [
            Point::new(f32::MAX, 0.0),
            Point::new(-f32::MAX, 0.0),
            Point::new(0.0, 1.0e30),
        ] {
            assert!(default_text_bounds(point).is_err());
        }
        assert!(
            text_bounds_from_screen_points(
                Point::new(f32::MAX, 0.0),
                Point::new(f32::MAX, 0.0),
                Viewport {
                    pan: Point::new(-f32::MAX, 0.0),
                    zoom: 0.5,
                    ..Viewport::default()
                },
            )
            .is_err()
        );
    }

    #[test]
    fn every_invalid_viewport_component_is_rejected() {
        let mut invalid = Vec::new();
        for zoom in [0.0, -1.0, f32::NAN, f32::INFINITY, 0.05, 4.0] {
            invalid.push(Viewport {
                zoom,
                ..Viewport::default()
            });
        }
        invalid.extend([
            Viewport {
                pan: Point::new(f32::NAN, 0.0),
                ..Viewport::default()
            },
            Viewport {
                pan: Point::new(0.0, f32::INFINITY),
                ..Viewport::default()
            },
            Viewport {
                min_zoom: 0.0,
                ..Viewport::default()
            },
            Viewport {
                min_zoom: f32::NAN,
                ..Viewport::default()
            },
            Viewport {
                max_zoom: f32::INFINITY,
                ..Viewport::default()
            },
            Viewport {
                min_zoom: 2.0,
                max_zoom: 1.0,
                ..Viewport::default()
            },
        ]);
        for viewport in invalid {
            assert!(TextPlacementDraft::new(Point::ZERO, viewport).is_err());
            assert!(text_bounds_from_screen_points(Point::ZERO, Point::ZERO, viewport).is_err());
        }
    }

    #[test]
    fn draft_preview_is_repeatable_uses_snapshot_and_never_changes_graph() {
        let mut graph = Graph::new();
        graph.add_group(frame("frame", frame_bounds())).unwrap();
        let before = graph.clone();
        let mut viewport = viewport(0.5);
        let draft = TextPlacementDraft::new(viewport.pan, viewport).unwrap();
        let end = viewport.pan + Point::new(50.0, 25.0);
        let expected = Rect::new(Point::ZERO, Size::new(100.0, 50.0));
        viewport.zoom = 2.0;
        assert_eq!(draft.bounds(end).unwrap(), expected);
        assert_eq!(draft.bounds(end).unwrap(), expected);
        assert_eq!(draft.viewport().zoom, 0.5);
        assert_eq!(viewport.zoom, 2.0);
        assert_eq!(draft.start_screen(), Point::new(80.0, -40.0));
        assert_eq!(
            containing_text_frame(&graph, expected),
            Some("frame".into())
        );
        assert!(draft.bounds(Point::new(f32::NAN, 0.0)).is_err());
        assert_eq!(draft.bounds(end).unwrap(), expected);
        // Cancellation has no graph operation to undo: simply discard `draft`.
        assert_eq!(graph, before);
    }

    #[test]
    fn containing_frame_requires_full_containment_and_accepts_edge_contact() {
        let mut graph = Graph::new();
        graph.add_group(frame("legacy", frame_bounds())).unwrap();
        assert_eq!(
            containing_text_frame(&graph, text_bounds()),
            Some("legacy".into())
        );
        let partial = text_bounds().translated(Point::new(1.0, 0.0));
        assert_eq!(containing_text_frame(&graph, partial), None);
        assert_eq!(
            containing_text_frame(&graph, Rect::new(Point::ZERO, Size::new(1.0, 1.0))),
            None
        );
    }

    #[test]
    fn overlapping_or_locked_containing_frames_never_gain_implicit_membership() {
        let mut graph = Graph::new();
        let first = frame("first", frame_bounds());
        graph.add_group(first.clone()).unwrap();
        graph.groups.get_mut(&first.id).unwrap().locked = true;
        assert_eq!(containing_text_frame(&graph, text_bounds()), None);
        graph.add_group(frame("second", frame_bounds())).unwrap();
        assert_eq!(containing_text_frame(&graph, text_bounds()), None);
        graph.groups.get_mut(&first.id).unwrap().locked = false;
        assert_eq!(containing_text_frame(&graph, text_bounds()), None);
        // Even nested containment is ambiguous; no innermost-frame heuristic.
        graph.groups.get_mut(&first.id).unwrap().bounds = text_bounds();
        assert_eq!(containing_text_frame(&graph, text_bounds()), None);
    }

    #[test]
    fn hidden_children_do_not_hide_frames_and_hosts_can_filter_hidden_frames() {
        let mut graph = Graph::new();
        graph
            .add_node(Node::new("child", "Hidden", Point::ZERO))
            .unwrap();
        let first = frame("visible", frame_bounds()).with_nodes(["child".into()]);
        graph.add_group(first.clone()).unwrap();
        set_node_visible(&mut graph, &"child".into(), false).unwrap();
        assert_eq!(
            containing_text_frame(&graph, text_bounds()),
            Some(first.id.clone())
        );
        graph.add_group(frame("hidden", frame_bounds())).unwrap();
        assert_eq!(
            containing_text_frame_in(
                graph.groups.values().filter(|frame| frame.id == first.id),
                text_bounds(),
            ),
            Some(first.id)
        );
        assert_eq!(
            containing_text_frame_in(std::iter::empty::<&Group>(), text_bounds()),
            None
        );
    }

    #[test]
    fn malformed_frame_geometry_cannot_claim_text() {
        let valid = frame("valid", frame_bounds());
        let invalid = frame(
            "invalid",
            Rect::new(Point::ZERO, Size::new(f32::INFINITY, 100.0)),
        );
        assert_eq!(
            containing_text_frame_in([&invalid, &valid], text_bounds()),
            Some(valid.id.clone())
        );
        assert_eq!(containing_text_frame_in([&invalid], text_bounds()), None);
    }
}
