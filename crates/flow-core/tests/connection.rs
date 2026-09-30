use gpui_flow_core::*;
fn graph() -> Graph {
    let mut graph = Graph::new();
    for (id, x) in [("a", 0.), ("b", 400.), ("c", 800.)] {
        graph
            .add_node(
                Node::new(id, id, Point::new(x, 30.))
                    .with_port(Port::input("in", "in", DataType::Number))
                    .with_port(Port::output("out", "out", DataType::Number)),
            )
            .unwrap();
    }
    graph
}
fn at(graph: &Graph, viewport: Viewport, id: &str, port: &str) -> Point {
    viewport.world_to_screen(graph.port_position(&PortRef::new(id, port)).unwrap())
}
fn begin(drag: &mut ConnectionDrag, graph: &Graph, viewport: Viewport) {
    drag.begin(
        graph,
        PortRef::new("a", "out"),
        at(graph, viewport, "a", "out"),
    )
    .unwrap();
}
#[test]
fn release_empty_cancels_and_later_motion_never_revives_preview() {
    let graph = graph();
    let viewport = Viewport::default();
    let mut drag = ConnectionDrag::default();
    begin(&mut drag, &graph, viewport);
    drag.move_to(&graph, viewport, Point::new(300., 500.), true);
    assert!(drag.preview(&graph, viewport).is_some());
    assert_eq!(
        drag.release(&graph, viewport, Point::new(300., 500.))
            .unwrap(),
        None
    );
    assert!(!drag.active());
    drag.move_to(&graph, viewport, at(&graph, viewport, "b", "in"), false);
    assert!(drag.preview(&graph, viewport).is_none());
    assert!(drag.target().is_none());
}
#[test]
fn snap_radius_is_screen_space_at_every_zoom_and_release_without_move_connects() {
    let graph = graph();
    for zoom in [0.25, 1., 3.] {
        let viewport = Viewport {
            zoom,
            pan: Point::new(83., -21.),
            ..Default::default()
        };
        let mut drag = ConnectionDrag::default();
        begin(&mut drag, &graph, viewport);
        let near = at(&graph, viewport, "b", "in") + Point::new(18., 0.);
        drag.move_to(&graph, viewport, near, true);
        assert_eq!(drag.target(), Some(&PortRef::new("b", "in")));
        assert_eq!(
            drag.preview(&graph, viewport).unwrap().1,
            graph.port_position(&PortRef::new("b", "in")).unwrap()
        );
        drag.move_to(
            &graph,
            viewport,
            at(&graph, viewport, "b", "in") + Point::new(21., 0.),
            true,
        );
        assert!(drag.target().is_none());
        assert_eq!(
            drag.release(&graph, viewport, near)
                .unwrap()
                .unwrap()
                .target,
            PortRef::new("b", "in")
        );
        assert!(!drag.active());
    }
}
#[test]
fn reverse_drag_normalizes_direction_and_invalid_release_clears() {
    let graph = graph();
    let viewport = Viewport::default();
    let mut drag = ConnectionDrag::default();
    drag.begin(
        &graph,
        PortRef::new("b", "in"),
        at(&graph, viewport, "b", "in"),
    )
    .unwrap();
    let pair = drag
        .release(&graph, viewport, at(&graph, viewport, "a", "out"))
        .unwrap()
        .unwrap();
    assert_eq!(pair.source, PortRef::new("a", "out"));
    assert_eq!(pair.target, PortRef::new("b", "in"));
    begin(&mut drag, &graph, viewport);
    assert!(drag
        .release(&graph, viewport, at(&graph, viewport, "b", "out"))
        .is_err());
    assert!(!drag.active());
}
#[test]
fn escape_middle_focus_loss_and_missing_button_share_idempotent_cancel() {
    let graph = graph();
    let viewport = Viewport::default();
    let mut drag = ConnectionDrag::default();
    for _ in 0..3 {
        begin(&mut drag, &graph, viewport);
        drag.cancel();
        drag.cancel();
        assert!(!drag.active());
        assert!(drag.preview(&graph, viewport).is_none());
    }
    begin(&mut drag, &graph, viewport);
    drag.move_to(&graph, viewport, at(&graph, viewport, "b", "in"), false);
    assert!(!drag.active());
}
#[test]
fn incompatible_full_duplicate_self_cycle_and_removed_ports_do_not_snap() {
    let mut graph = graph();
    let viewport = Viewport::default();
    let mut drag = ConnectionDrag::default();
    graph.nodes.get_mut(&"b".into()).unwrap().ports[0].data_type = DataType::String;
    begin(&mut drag, &graph, viewport);
    drag.move_to(&graph, viewport, at(&graph, viewport, "b", "in"), true);
    assert!(drag.target().is_none());
    assert_eq!(drag.hovered(), Some(&PortRef::new("b", "in")));
    assert!(drag
        .release(&graph, viewport, at(&graph, viewport, "a", "in"))
        .is_err());
    graph.nodes.get_mut(&"b".into()).unwrap().ports[0].data_type = DataType::Number;
    graph
        .connect("edge", PortRef::new("a", "out"), PortRef::new("b", "in"))
        .unwrap();
    begin(&mut drag, &graph, viewport);
    assert!(drag
        .release(&graph, viewport, at(&graph, viewport, "b", "in"))
        .is_err());
    assert!(drag
        .begin(
            &graph,
            PortRef::new("b", "in"),
            at(&graph, viewport, "b", "in")
        )
        .is_err());
    assert!(!drag.active());
    graph.options.allow_cycles = false;
    drag.begin(
        &graph,
        PortRef::new("b", "out"),
        at(&graph, viewport, "b", "out"),
    )
    .unwrap();
    assert!(drag
        .release(&graph, viewport, at(&graph, viewport, "a", "in"))
        .is_err());
    begin(&mut drag, &graph, viewport);
    graph.remove_node(&"a".into()).unwrap();
    drag.move_to(&graph, viewport, Point::ZERO, true);
    assert!(!drag.active());
}
#[test]
fn successful_drag_is_one_atomic_undo_and_cancel_does_not_mutate() {
    let mut editor = Editor::new(graph()).unwrap();
    let viewport = Viewport::default();
    let mut drag = ConnectionDrag::default();
    begin(&mut drag, editor.graph(), viewport);
    let pair = drag
        .release(
            editor.graph(),
            viewport,
            at(editor.graph(), viewport, "b", "in"),
        )
        .unwrap()
        .unwrap();
    editor
        .transaction("Connect", |g| g.connect("edge", pair.source, pair.target))
        .unwrap();
    assert_eq!(editor.graph().edges.len(), 1);
    assert!(editor.undo());
    assert!(editor.graph().edges.is_empty());
    assert!(editor.redo());
    let before = editor.graph().clone();
    begin(&mut drag, editor.graph(), viewport);
    drag.cancel();
    assert_eq!(&before, editor.graph());
}

#[test]
fn exact_hit_api_does_not_implicitly_enable_magnet_snapping() {
    let graph = graph();
    let viewport = Viewport::default();
    let mut drag = ConnectionDrag::default();
    begin(&mut drag, &graph, viewport);
    drag.move_to_exact(
        &graph,
        at(&graph, viewport, "b", "in") + Point::new(1., 0.),
        None,
        true,
    );
    assert!(drag.target().is_none());
    assert!(drag.release_exact(&graph).unwrap().is_none());
    begin(&mut drag, &graph, viewport);
    drag.move_to_exact(
        &graph,
        at(&graph, viewport, "b", "in"),
        Some(PortRef::new("b", "in")),
        true,
    );
    assert!(drag.release_exact(&graph).unwrap().is_some());
    assert!(!drag.active());
}
