//! Creative Frames use core groups without changing node geometry or topology.
//!
//! Each mutation validates a candidate before committing. The caller owns editor
//! transactions and undo history. Resizing changes only the frame rectangle;
//! moving a frame together with its members remains the core editor's operation.

use gpui_flow_core::{
    Graph, GraphError, Group, GroupId, NodeId, Point, Rect, Size, ValidationIssue, hidden_node_ids,
};
use std::collections::BTreeSet;

pub const MIN_FRAME_SIZE: f32 = 32.0;
pub const MAX_FRAME_SIZE: f32 = 16_384.0;
pub const DEFAULT_FRAME_SIZE: Size = Size::new(1024.0, 1024.0);

/// Initial membership policy. Merely intersecting a node never collects it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FrameContents {
    #[default]
    Empty,
    /// Collect fully enclosed, visible, unlocked nodes with no existing group.
    EncloseVisible,
}

/// A click-created frame starts at `origin` with a 1024 by 1024 world-unit size.
pub fn default_frame_bounds(origin: Point) -> Result<Rect, GraphError> {
    normalize_frame_bounds(Rect::new(origin, DEFAULT_FRAME_SIZE))
}

/// A drag in either direction, rejecting non-finite points and undersized drags.
pub fn frame_bounds_from_points(start: Point, end: Point) -> Result<Rect, GraphError> {
    // Check before Rect::from_points: f32::min/max can otherwise discard a NaN.
    if !start.is_finite() || !end.is_finite() {
        return Err(invalid_bounds());
    }
    normalize_frame_bounds(Rect::from_points(start, end))
}

/// Normalize reverse-drag dimensions, then enforce finite bounds and the size
/// range. Invalid input is rejected rather than silently clamped.
pub fn normalize_frame_bounds(bounds: Rect) -> Result<Rect, GraphError> {
    if !bounds.origin.is_finite() || !bounds.size.is_finite() {
        return Err(invalid_bounds());
    }
    let end = bounds.origin + Point::new(bounds.size.width, bounds.size.height);
    if !end.is_finite() {
        return Err(invalid_bounds());
    }
    let normalized = Rect::new(
        Point::new(bounds.origin.x.min(end.x), bounds.origin.y.min(end.y)),
        Size::new(bounds.size.width.abs(), bounds.size.height.abs()),
    );
    if !normalized.is_valid()
        || !(MIN_FRAME_SIZE..=MAX_FRAME_SIZE).contains(&normalized.size.width)
        || !(MIN_FRAME_SIZE..=MAX_FRAME_SIZE).contains(&normalized.size.height)
    {
        return Err(invalid_bounds());
    }
    Ok(normalized)
}

/// Create a frame atomically. Existing groups are never reparented or changed.
/// The marker opts new frames into Creative's presentation; title and bounds
/// retain the existing core fields and old groups need no metadata migration.
pub fn create_frame(
    graph: &mut Graph,
    id: impl Into<GroupId>,
    title: impl Into<String>,
    bounds: Rect,
    contents: FrameContents,
) -> Result<GroupId, GraphError> {
    let bounds = normalize_frame_bounds(bounds)?;
    let mut frame = Group::new(id, title, bounds);
    frame.data = serde_json::json!({ "flow_creative_frame": 1 });
    if contents == FrameContents::EncloseVisible {
        frame.node_ids = enclosed_eligible_nodes(graph, bounds);
    }
    let id = frame.id.clone();
    graph.add_group(frame)?;
    Ok(id)
}

/// Change the title without touching application metadata or any members.
pub fn set_frame_title(
    graph: &mut Graph,
    id: &GroupId,
    title: impl Into<String>,
) -> Result<bool, GraphError> {
    let mut frame = editable_frame(graph, id)?.clone();
    frame.title = title.into();
    commit_frame(graph, frame)
}

/// Set only the frame rectangle. Members keep their world positions, sizes and
/// membership, even when they now fall outside the frame or are locked/hidden.
pub fn set_frame_bounds(graph: &mut Graph, id: &GroupId, bounds: Rect) -> Result<bool, GraphError> {
    let mut frame = editable_frame(graph, id)?.clone();
    frame.bounds = normalize_frame_bounds(bounds)?;
    commit_frame(graph, frame)
}

/// Query the same complete-containment policy used by `EncloseVisible`.
/// Already grouped nodes, including this frame's own members, are omitted.
/// This is read-only; applying the result still checks the target frame's lock.
pub fn enclosed_frame_members(graph: &Graph, id: &GroupId) -> Result<BTreeSet<NodeId>, GraphError> {
    let frame = graph
        .groups
        .get(id)
        .ok_or_else(|| GraphError::MissingGroup(id.clone()))?;
    let bounds = normalize_frame_bounds(frame.bounds)?;
    Ok(enclosed_eligible_nodes(graph, bounds))
}

/// Explicitly collect nodes without moving them or fitting the frame. Hidden
/// and out-of-bounds nodes are allowed because the caller selected them by ID.
/// Missing/locked nodes and membership in another group reject the whole batch;
/// existing membership in this frame is an idempotent no-op.
pub fn add_frame_members(
    graph: &mut Graph,
    id: &GroupId,
    nodes: &BTreeSet<NodeId>,
) -> Result<bool, GraphError> {
    let mut frame = editable_frame(graph, id)?.clone();
    for node in nodes {
        ensure_unlocked_node(graph, node)?;
        if let Some(owner) = graph
            .groups
            .values()
            .find(|group| &group.id != id && group.node_ids.contains(node))
        {
            return Err(GraphError::Validation(vec![ValidationIssue {
                path: format!("groups.{id}.node_ids"),
                message: format!("node {node} already belongs to group {}", owner.id),
            }]));
        }
        frame.node_ids.insert(node.clone());
    }
    commit_frame(graph, frame)
}

/// Remove only membership, keeping nodes, assets, links and positions intact.
/// Missing IDs reject the whole batch. Existing nodes outside this frame are
/// ignored; a locked member cannot be released until explicitly unlocked.
pub fn remove_frame_members(
    graph: &mut Graph,
    id: &GroupId,
    nodes: &BTreeSet<NodeId>,
) -> Result<bool, GraphError> {
    let mut frame = editable_frame(graph, id)?.clone();
    for node in nodes {
        if !graph.nodes.contains_key(node) {
            return Err(GraphError::MissingNode(node.clone()));
        }
        if frame.node_ids.contains(node) {
            ensure_unlocked_node(graph, node)?;
            frame.node_ids.remove(node);
        }
    }
    commit_frame(graph, frame)
}

/// Delete only the frame. Removing membership is an edit to a locked member,
/// so both a locked frame and any effectively locked member reject deletion.
pub fn remove_frame(graph: &mut Graph, id: &GroupId) -> Result<Group, GraphError> {
    let frame = editable_frame(graph, id)?;
    for node in &frame.node_ids {
        ensure_unlocked_node(graph, node)?;
    }
    let mut candidate = graph.clone();
    let removed = candidate.remove_group(id)?;
    candidate.validate()?;
    *graph = candidate;
    Ok(removed)
}

fn invalid_bounds() -> GraphError {
    GraphError::InvalidGeometry(format!(
        "Frame bounds must be finite, with width and height in {MIN_FRAME_SIZE}..={MAX_FRAME_SIZE}"
    ))
}

fn editable_frame<'a>(graph: &'a Graph, id: &GroupId) -> Result<&'a Group, GraphError> {
    let frame = graph
        .groups
        .get(id)
        .ok_or_else(|| GraphError::MissingGroup(id.clone()))?;
    if frame.locked {
        return Err(GraphError::Locked(id.to_string()));
    }
    Ok(frame)
}

fn ensure_unlocked_node(graph: &Graph, id: &NodeId) -> Result<(), GraphError> {
    let node = graph
        .node(id)
        .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
    if node.locked
        || graph
            .groups
            .values()
            .any(|group| group.locked && group.node_ids.contains(id))
    {
        return Err(GraphError::Locked(id.to_string()));
    }
    Ok(())
}

fn enclosed_eligible_nodes(graph: &Graph, bounds: Rect) -> BTreeSet<NodeId> {
    let hidden = hidden_node_ids(graph);
    let grouped: BTreeSet<_> = graph
        .groups
        .values()
        .flat_map(|group| group.node_ids.iter())
        .collect();
    graph
        .nodes
        .values()
        .filter(|node| {
            !node.locked
                && !hidden.contains(&node.id)
                && !grouped.contains(&node.id)
                && node.bounds().is_valid()
                && bounds.contains_rect(node.bounds())
        })
        .map(|node| node.id.clone())
        .collect()
}

fn commit_frame(graph: &mut Graph, frame: Group) -> Result<bool, GraphError> {
    if graph.groups.get(&frame.id) == Some(&frame) {
        return Ok(false);
    }
    let mut candidate = graph.clone();
    candidate.groups.insert(frame.id.clone(), frame);
    candidate.validate()?;
    *graph = candidate;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{DataType, Node, Port, PortRef, set_node_visible};
    use serde_json::json;

    fn bounds() -> Rect {
        Rect::new(Point::ZERO, Size::new(300.0, 200.0))
    }

    fn ids(values: &[&str]) -> BTreeSet<NodeId> {
        values.iter().map(|id| NodeId::from(*id)).collect()
    }

    fn add_node(graph: &mut Graph, id: &str, x: f32, y: f32) {
        graph
            .add_node(Node::new(id, id, Point::new(x, y)).with_size(Size::new(40.0, 40.0)))
            .unwrap();
    }

    fn empty_frame(graph: &mut Graph, id: &str) -> GroupId {
        create_frame(graph, id, "Frame", bounds(), FrameContents::Empty).unwrap()
    }

    #[test]
    fn reverse_drags_normalize_and_click_frames_start_empty_at_1024() {
        let reversed = frame_bounds_from_points(Point::new(300.0, 200.0), Point::ZERO).unwrap();
        assert_eq!(reversed, bounds());
        let signed = Rect::new(Point::new(300.0, 200.0), Size::new(-300.0, -200.0));
        let mut graph = Graph::new();
        add_node(&mut graph, "inside", 10.0, 10.0);
        let frame =
            create_frame(&mut graph, "drag", "Board", signed, FrameContents::Empty).unwrap();
        assert_eq!(graph.groups[&frame].bounds, bounds());
        assert!(graph.groups[&frame].node_ids.is_empty());
        assert_eq!(graph.groups[&frame].data, json!({"flow_creative_frame": 1}));
        assert_eq!(
            default_frame_bounds(Point::new(-50.0, 20.0)).unwrap(),
            Rect::new(Point::new(-50.0, 20.0), Size::new(1024.0, 1024.0))
        );
    }

    #[test]
    fn automatic_collection_requires_full_containment_and_skips_protected_nodes() {
        let mut graph = Graph::new();
        for (id, x, y) in [
            ("inside", 10.0, 10.0),
            ("boundary", 260.0, 160.0),
            ("partial", 290.0, 20.0),
            ("outside", 350.0, 10.0),
            ("hidden", 20.0, 20.0),
            ("locked", 30.0, 30.0),
            ("grouped", 40.0, 40.0),
            ("group_locked", 50.0, 50.0),
        ] {
            add_node(&mut graph, id, x, y);
        }
        set_node_visible(&mut graph, &"hidden".into(), false).unwrap();
        graph.nodes.get_mut(&"locked".into()).unwrap().locked = true;
        let existing = empty_frame(&mut graph, "existing");
        add_frame_members(&mut graph, &existing, &ids(&["grouped"])).unwrap();
        let locked = empty_frame(&mut graph, "locked_frame");
        add_frame_members(&mut graph, &locked, &ids(&["group_locked"])).unwrap();
        graph.groups.get_mut(&locked).unwrap().locked = true;
        let before = graph.clone();
        let frame = create_frame(
            &mut graph,
            "collected",
            "Collected",
            bounds(),
            FrameContents::EncloseVisible,
        )
        .unwrap();
        assert_eq!(graph.groups[&frame].node_ids, ids(&["inside", "boundary"]));
        assert_eq!(graph.nodes, before.nodes);
        assert_eq!(graph.groups[&existing], before.groups[&existing]);
        assert_eq!(graph.groups[&locked], before.groups[&locked]);
        assert_eq!(graph.data, before.data);
        assert!(enclosed_frame_members(&graph, &frame).unwrap().is_empty());
        add_node(&mut graph, "later", 100.0, 100.0);
        assert_eq!(
            enclosed_frame_members(&graph, &frame).unwrap(),
            ids(&["later"])
        );
    }

    #[test]
    fn bounds_and_title_edits_preserve_locked_members_and_legacy_metadata() {
        let mut graph = Graph::new();
        add_node(&mut graph, "image", 60.0, 80.0);
        let mut legacy = Group::new("legacy", "Old", bounds()).with_nodes(ids(&["image"]));
        legacy.data = json!({"host": {"shape": "custom", "color": "red"}});
        graph.add_group(legacy).unwrap();
        let id = GroupId::from("legacy");
        graph.nodes.get_mut(&"image".into()).unwrap().locked = true;
        set_node_visible(&mut graph, &"image".into(), false).unwrap();
        let before = graph.clone();
        let next = Rect::new(Point::new(500.0, -100.0), Size::new(32.0, 16_384.0));
        assert!(set_frame_bounds(&mut graph, &id, next).unwrap());
        assert!(set_frame_title(&mut graph, &id, "Renamed").unwrap());
        assert_eq!(graph.groups[&id].bounds, next);
        assert_eq!(graph.groups[&id].title, "Renamed");
        assert_eq!(graph.nodes, before.nodes);
        assert_eq!(graph.groups[&id].node_ids, before.groups[&id].node_ids);
        assert_eq!(graph.groups[&id].data, before.groups[&id].data);
        assert_eq!(graph.data, before.data);
        assert!(!set_frame_bounds(&mut graph, &id, next).unwrap());
        assert!(!set_frame_title(&mut graph, &id, "Renamed").unwrap());
    }

    #[test]
    fn explicit_membership_allows_hidden_and_outside_nodes_without_moving_them() {
        let mut graph = Graph::new();
        add_node(&mut graph, "hidden", 10.0, 10.0);
        add_node(&mut graph, "outside", 600.0, 400.0);
        set_node_visible(&mut graph, &"hidden".into(), false).unwrap();
        let frame = empty_frame(&mut graph, "frame");
        let before = graph.clone();
        let selected = ids(&["hidden", "outside"]);
        assert!(add_frame_members(&mut graph, &frame, &selected).unwrap());
        assert!(!add_frame_members(&mut graph, &frame, &selected).unwrap());
        assert_eq!(graph.groups[&frame].node_ids, selected);
        assert_eq!(graph.groups[&frame].bounds, before.groups[&frame].bounds);
        assert_eq!(graph.nodes, before.nodes);
        assert_eq!(graph.data, before.data);
        assert!(remove_frame_members(&mut graph, &frame, &selected).unwrap());
        assert_eq!(graph, before);
        assert!(!remove_frame_members(&mut graph, &frame, &selected).unwrap());
    }

    #[test]
    fn membership_batches_reject_missing_locked_and_owned_nodes_atomically() {
        let mut graph = Graph::new();
        for id in ["a_free", "z_owned", "z_locked", "z_parent_locked"] {
            add_node(&mut graph, id, 20.0, 20.0);
        }
        let target = empty_frame(&mut graph, "target");
        let other = empty_frame(&mut graph, "other");
        add_frame_members(&mut graph, &other, &ids(&["z_owned"])).unwrap();
        let locked = empty_frame(&mut graph, "locked");
        add_frame_members(&mut graph, &locked, &ids(&["z_parent_locked"])).unwrap();
        graph.groups.get_mut(&locked).unwrap().locked = true;
        graph.nodes.get_mut(&"z_locked".into()).unwrap().locked = true;
        let before = graph.clone();
        for bad in ["z_owned", "z_locked", "z_parent_locked", "z_missing"] {
            assert!(add_frame_members(&mut graph, &target, &ids(&["a_free", bad])).is_err());
            assert_eq!(graph, before);
        }
        add_frame_members(&mut graph, &target, &ids(&["a_free"])).unwrap();
        let before = graph.clone();
        assert!(remove_frame_members(&mut graph, &target, &ids(&["a_free", "z_missing"])).is_err());
        assert_eq!(graph, before);
        graph.nodes.get_mut(&"a_free".into()).unwrap().locked = true;
        let before = graph.clone();
        assert!(remove_frame_members(&mut graph, &target, &ids(&["a_free"])).is_err());
        assert!(remove_frame(&mut graph, &target).is_err());
        assert_eq!(graph, before);
    }

    #[test]
    fn locked_frame_rejects_all_mutations() {
        let mut graph = Graph::new();
        add_node(&mut graph, "member", 20.0, 20.0);
        add_node(&mut graph, "free", 80.0, 80.0);
        let frame = empty_frame(&mut graph, "frame");
        add_frame_members(&mut graph, &frame, &ids(&["member"])).unwrap();
        graph.groups.get_mut(&frame).unwrap().locked = true;
        let before = graph.clone();
        assert!(set_frame_title(&mut graph, &frame, "Changed").is_err());
        assert!(
            set_frame_bounds(
                &mut graph,
                &frame,
                default_frame_bounds(Point::ZERO).unwrap()
            )
            .is_err()
        );
        assert!(add_frame_members(&mut graph, &frame, &ids(&["free"])).is_err());
        assert!(remove_frame_members(&mut graph, &frame, &ids(&["member"])).is_err());
        assert!(remove_frame(&mut graph, &frame).is_err());
        assert_eq!(graph, before);
    }

    #[test]
    fn removing_frame_preserves_assets_nodes_edges_and_presentation() {
        let mut graph = Graph::new();
        graph
            .add_node(
                Node::new("image", "Image", Point::ZERO)
                    .with_data(json!({"asset": "assets/original.png"}))
                    .with_port(Port::output(
                        "image",
                        "Image",
                        DataType::Named("image".into()),
                    )),
            )
            .unwrap();
        graph
            .add_node(
                Node::new("consumer", "Consumer", Point::new(400.0, 0.0)).with_port(Port::input(
                    "input",
                    "Image",
                    DataType::Named("image".into()),
                )),
            )
            .unwrap();
        graph
            .connect(
                "edge",
                PortRef::new("image", "image"),
                PortRef::new("consumer", "input"),
            )
            .unwrap();
        graph.data = json!({"assets": ["assets/original.png"], "host": 42});
        set_node_visible(&mut graph, &"image".into(), false).unwrap();
        let frame = empty_frame(&mut graph, "frame");
        add_frame_members(&mut graph, &frame, &ids(&["image"])).unwrap();
        let before = graph.clone();
        assert_eq!(
            remove_frame(&mut graph, &frame).unwrap(),
            before.groups[&frame]
        );
        assert!(graph.groups.is_empty());
        assert_eq!(graph.nodes, before.nodes);
        assert_eq!(graph.edges, before.edges);
        assert_eq!(graph.data, before.data);
        graph.validate().unwrap();
    }

    #[test]
    fn invalid_bounds_and_duplicate_ids_leave_graph_unchanged() {
        let mut graph = Graph::new();
        let frame = empty_frame(&mut graph, "frame");
        let before = graph.clone();
        for rect in [
            Rect::new(Point::ZERO, Size::new(31.0, 32.0)),
            Rect::new(Point::ZERO, Size::new(32.0, 16_385.0)),
            Rect::new(Point::ZERO, Size::new(0.0, 100.0)),
            Rect::new(Point::ZERO, Size::new(f32::INFINITY, 100.0)),
            Rect::new(Point::ZERO, Size::new(100.0, f32::NAN)),
            Rect::new(Point::new(f32::NAN, 0.0), Size::new(100.0, 100.0)),
            Rect::new(Point::new(0.0, f32::INFINITY), Size::new(100.0, 100.0)),
        ] {
            assert!(create_frame(&mut graph, "new", "New", rect, FrameContents::Empty).is_err());
            assert_eq!(graph, before);
            assert!(set_frame_bounds(&mut graph, &frame, rect).is_err());
            assert_eq!(graph, before);
        }
        assert!(
            frame_bounds_from_points(Point::new(f32::NAN, 0.0), Point::new(100.0, 100.0)).is_err()
        );
        assert!(
            frame_bounds_from_points(Point::new(-f32::MAX, 0.0), Point::new(f32::MAX, 100.0))
                .is_err()
        );
        assert!(
            create_frame(
                &mut graph,
                "frame",
                "Duplicate",
                bounds(),
                FrameContents::Empty
            )
            .is_err()
        );
        assert!(create_frame(&mut graph, " ", "Invalid", bounds(), FrameContents::Empty).is_err());
        assert_eq!(graph, before);
    }
    #[test]
    fn frame_history_and_document_roundtrip_preserve_members_and_metadata() {
        let mut graph = Graph::new();
        add_node(&mut graph, "image", 40., 50.);
        let mut editor = gpui_flow_core::Editor::new(graph.clone()).unwrap();
        editor
            .transaction("Draw frame", |g| {
                create_frame(g, "board", "Panel", bounds(), FrameContents::EncloseVisible)?;
                Ok(())
            })
            .unwrap();
        let drawn = editor.graph().clone();
        assert_eq!(drawn.groups[&"board".into()].node_ids, ids(&["image"]));
        editor
            .transaction("Resize frame", |g| {
                set_frame_bounds(
                    g,
                    &"board".into(),
                    Rect::new(Point::ZERO, Size::new(500., 600.)),
                )?;
                Ok(())
            })
            .unwrap();
        assert_eq!(editor.graph().nodes, graph.nodes);
        assert!(editor.undo());
        assert_eq!(editor.graph(), &drawn);
        assert!(editor.undo());
        assert_eq!(editor.graph(), &graph);
        assert!(editor.redo());
        assert!(editor.redo());
        let document = editor.document();
        let encoded = serde_json::to_string(&document).unwrap();
        let decoded: gpui_flow_core::FlowDocument = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, document);
    }
}
