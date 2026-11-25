use gpui_flow_interaction::{core::*, plugins::*, *};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
fn editor() -> Editor {
    let mut graph = Graph::new();
    for (id, x) in [("a", 0.0), ("b", 350.0), ("c", 700.0)] {
        graph
            .add_node(
                Node::new(id, id, Point::new(x, 0.0))
                    .with_port(Port::input("in", "Input", DataType::Number))
                    .with_port(Port::output("out", "Output", DataType::Number)),
            )
            .unwrap();
    }
    Editor::new(graph).unwrap()
}
fn input(x: f32, y: f32, hit: HitTarget) -> PointerInput {
    PointerInput::left(Point::new(x, y), hit)
}
fn down(
    host: &mut PluginHost,
    editor: &mut Editor,
    x: f32,
    y: f32,
    hit: HitTarget,
) -> DispatchReport {
    host.dispatch(editor, CanvasEvent::PointerDown(input(x, y, hit)))
}
fn movement(host: &mut PluginHost, editor: &mut Editor, x: f32, y: f32) -> DispatchReport {
    host.dispatch(
        editor,
        CanvasEvent::PointerMove(input(x, y, HitTarget::Background)),
    )
}
fn up(
    host: &mut PluginHost,
    editor: &mut Editor,
    x: f32,
    y: f32,
    hit: HitTarget,
) -> DispatchReport {
    host.dispatch(editor, CanvasEvent::PointerUp(input(x, y, hit)))
}
fn start_drag(host: &mut PluginHost, editor: &mut Editor) {
    down(host, editor, 20.0, 20.0, HitTarget::Node("a".into()));
    movement(host, editor, 57.0, 55.0);
}
#[test]
fn host_is_empty_until_behaviors_are_installed() {
    let mut host = PluginHost::default();
    let mut editor = editor();
    down(
        &mut host,
        &mut editor,
        10.0,
        10.0,
        HitTarget::Node("a".into()),
    );
    assert!(editor.selection.is_empty());
    assert!(!host.has_capture());
    host.install(SelectionPlugin::default()).unwrap();
    down(
        &mut host,
        &mut editor,
        10.0,
        10.0,
        HitTarget::Node("a".into()),
    );
    assert!(editor.selection.nodes.contains(&"a".into()));
}
#[test]
fn selection_before_drag_is_real_ordered_behavior_and_one_undo() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    start_drag(&mut host, &mut editor);
    assert_eq!(host.capture_owner(), Some("drag"));
    assert!(editor.transaction_active());
    assert!(editor.graph().nodes[&"a".into()].position.x > 0.0);
    movement(&mut host, &mut editor, 90.0, 70.0);
    up(&mut host, &mut editor, 90.0, 70.0, HitTarget::Background);
    assert!(!editor.transaction_active());
    assert!(!host.has_capture());
    assert_eq!(editor.history_len(), 1);
    editor.undo();
    assert_eq!(editor.graph().nodes[&"a".into()].position, Point::ZERO);
}
#[test]
fn disabled_selection_does_not_drag_an_unrelated_existing_selection() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    editor.selection.select_node("b", false);
    host.set_enabled("selection", false, &mut editor).unwrap();
    down(
        &mut host,
        &mut editor,
        10.0,
        10.0,
        HitTarget::Node("a".into()),
    );
    movement(&mut host, &mut editor, 100.0, 100.0);
    up(&mut host, &mut editor, 100.0, 100.0, HitTarget::Background);
    assert!(!host.has_capture());
    assert_eq!(
        editor.graph().nodes[&"b".into()].position,
        Point::new(350.0, 0.0)
    );
    assert_eq!(editor.history_len(), 0);
}
#[test]
fn disabled_drag_still_allows_selection() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    host.set_enabled("drag", false, &mut editor).unwrap();
    start_drag(&mut host, &mut editor);
    assert!(editor.selection.nodes.contains(&"a".into()));
    assert_eq!(editor.graph().nodes[&"a".into()].position, Point::ZERO);
    assert!(!host.has_capture());
}
#[test]
fn drag_threshold_is_screen_space_at_any_zoom() {
    for zoom in [0.25, 1.0, 3.0] {
        let mut host = PluginHost::with_defaults();
        let mut editor = editor();
        editor.viewport.zoom = zoom;
        down(
            &mut host,
            &mut editor,
            10.0,
            10.0,
            HitTarget::Node("a".into()),
        );
        movement(&mut host, &mut editor, 12.0, 10.0);
        assert!(!editor.transaction_active());
        movement(&mut host, &mut editor, 14.0, 10.0);
        assert!(editor.transaction_active());
        host.cancel(&mut editor);
    }
}
#[test]
fn uninstalling_active_drag_rolls_back_graph_and_capture() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    let before = editor.graph().clone();
    start_drag(&mut host, &mut editor);
    let report = host.uninstall("drag", &mut editor).unwrap();
    assert!(report.document_changed);
    assert_eq!(editor.graph(), &before);
    assert!(!editor.transaction_active());
    assert!(!host.has_capture());
    assert_eq!(host.cursor(), CanvasCursor::Default);
    assert_eq!(editor.history_len(), 0);
}
#[test]
fn disabling_active_resize_rolls_back() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    let original = editor.graph().nodes[&"a".into()].size;
    down(
        &mut host,
        &mut editor,
        224.0,
        144.0,
        HitTarget::ResizeHandle("a".into()),
    );
    movement(&mut host, &mut editor, 300.0, 210.0);
    assert_ne!(editor.graph().nodes[&"a".into()].size, original);
    host.set_enabled("resize", false, &mut editor).unwrap();
    assert_eq!(editor.graph().nodes[&"a".into()].size, original);
    assert!(!editor.transaction_active());
}
#[test]
fn lost_button_focus_escape_and_multibutton_interrupt_cancel_not_commit() {
    for cancel in [
        CanvasEvent::PointerMove(PointerInput {
            position: Point::new(70.0, 60.0),
            button: None,
            ..Default::default()
        }),
        CanvasEvent::FocusLost,
        CanvasEvent::KeyDown {
            key: KeyCode::Escape,
            modifiers: Modifiers::default(),
        },
        CanvasEvent::PointerDown(PointerInput {
            position: Point::ZERO,
            button: Some(PointerButton::Right),
            ..Default::default()
        }),
    ] {
        let mut host = PluginHost::with_defaults();
        let mut editor = editor();
        start_drag(&mut host, &mut editor);
        host.dispatch(&mut editor, cancel);
        assert_eq!(editor.graph().nodes[&"a".into()].position, Point::ZERO);
        assert_eq!(editor.history_len(), 0);
        assert!(!editor.transaction_active());
    }
}
#[test]
fn switching_readonly_midgesture_rolls_back_before_applying_policy() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    start_drag(&mut host, &mut editor);
    host.set_editable(false, &mut editor);
    assert_eq!(editor.graph().nodes[&"a".into()].position, Point::ZERO);
    assert!(!editor.transaction_active());
    assert!(!host.has_capture());
    assert!(!host.policy().editable);
}
#[test]
fn readonly_blocks_every_edit_command_including_history_paste_and_input() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    editor.selection.select_node("a", false);
    let clipboard = editor.copy_selection();
    let document = editor.document();
    host.set_editable(false, &mut editor);
    let before = editor.graph().clone();
    for command in [
        CanvasCommand::AddNode(Node::new("extra", "Extra", Point::ZERO)),
        CanvasCommand::RemoveNode { node: "a".into() },
        CanvasCommand::DeleteSelection,
        CanvasCommand::GroupSelection {
            title: "group".into(),
            padding: 10.0,
        },
        CanvasCommand::UngroupSelection,
        CanvasCommand::Paste {
            clipboard,
            offset: Point::ZERO,
        },
        CanvasCommand::Duplicate {
            offset: Point::ZERO,
        },
        CanvasCommand::Undo,
        CanvasCommand::Redo,
        CanvasCommand::SetNodeData {
            node: "a".into(),
            data: json!({"text":"blocked"}),
        },
        CanvasCommand::LoadDocument(document.clone()),
        CanvasCommand::ImportDocument(document),
        CanvasCommand::ImportGraph(Graph::new()),
        CanvasCommand::BeginGesture {
            label: "blocked".into(),
        },
    ] {
        assert!(matches!(
            host.execute(&mut editor, command),
            Err(InteractionError::CapabilityDenied(Capability::Edit))
        ));
    }
    assert_eq!(editor.graph(), &before);
    assert_eq!(editor.history_len(), 0);
}
#[test]
fn remove_node_keeps_other_selection_and_restores_incident_edges_on_undo() {
    let mut host = PluginHost::with_defaults();
    let mut graph = editor().graph().clone();
    graph
        .connect("wire", PortRef::new("a", "out"), PortRef::new("b", "in"))
        .unwrap();
    let mut editor = Editor::new(graph).unwrap();
    editor.selection.select_node("b", false);
    let selected = editor.selection.clone();
    let report = host
        .execute(&mut editor, CanvasCommand::RemoveNode { node: "a".into() })
        .unwrap();
    assert!(report.document_changed);
    assert!(!report.selection_changed);
    assert_eq!(editor.selection, selected);
    assert!(editor.graph().node(&"a".into()).is_none());
    assert!(editor.graph().edges.is_empty());
    assert_eq!(editor.history_len(), 1);
    assert!(editor.undo());
    assert!(editor.graph().node(&"a".into()).is_some());
    assert!(editor.graph().edges.contains_key(&"wire".into()));
}
#[test]
fn readonly_navigation_selection_are_independently_configurable() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    host.set_policy(
        AccessPolicy {
            editable: false,
            navigation: true,
            selection: false,
            clipboard_read: true,
        },
        &mut editor,
    );
    down(
        &mut host,
        &mut editor,
        10.0,
        10.0,
        HitTarget::Node("a".into()),
    );
    assert!(editor.selection.is_empty());
    host.dispatch(
        &mut editor,
        CanvasEvent::Wheel {
            position: Point::ZERO,
            delta: Point::new(10.0, 20.0),
            modifiers: Modifiers::default(),
        },
    );
    assert_eq!(editor.viewport.pan, Point::new(10.0, 20.0));
    host.set_policy(
        AccessPolicy {
            navigation: false,
            selection: true,
            ..AccessPolicy::read_only()
        },
        &mut editor,
    );
    assert!(
        host.execute(
            &mut editor,
            CanvasCommand::Zoom {
                position: Point::ZERO,
                zoom: 2.0
            }
        )
        .is_err()
    );
    down(
        &mut host,
        &mut editor,
        10.0,
        10.0,
        HitTarget::Node("a".into()),
    );
    assert!(editor.selection.nodes.contains(&"a".into()));
}
#[test]
fn zoom_keeps_pointer_world_anchor_and_disabling_zoom_removes_behavior() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    let position = Point::new(400.0, 200.0);
    let before = editor.viewport.screen_to_world(position);
    let event = CanvasEvent::Wheel {
        position,
        delta: Point::new(0.0, -200.0),
        modifiers: Modifiers {
            control: true,
            ..Default::default()
        },
    };
    host.dispatch(&mut editor, event.clone());
    assert!(editor.viewport.zoom > 1.0);
    assert!(editor.viewport.screen_to_world(position).distance(before) < 0.001);
    host.set_enabled("zoom", false, &mut editor).unwrap();
    let old = editor.viewport;
    host.dispatch(&mut editor, event);
    assert_eq!(editor.viewport, old);
}
#[test]
fn pan_can_be_removed_without_removing_zoom() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    host.uninstall("pan", &mut editor).unwrap();
    host.dispatch(
        &mut editor,
        CanvasEvent::Wheel {
            position: Point::ZERO,
            delta: Point::new(30.0, 40.0),
            modifiers: Modifiers::default(),
        },
    );
    assert_eq!(editor.viewport.pan, Point::ZERO);
    host.dispatch(
        &mut editor,
        CanvasEvent::Wheel {
            position: Point::ZERO,
            delta: Point::new(0.0, -100.0),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
        },
    );
    assert!(editor.viewport.zoom > 1.0);
}
#[test]
fn exact_connection_release_commits_one_transaction_without_snapping() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    host.uninstall("snapping", &mut editor).unwrap();
    let from = PortRef::new("a", "out");
    let to = PortRef::new("b", "in");
    let start = editor.graph().port_position(&from).unwrap();
    let end = editor.graph().port_position(&to).unwrap();
    down(
        &mut host,
        &mut editor,
        start.x,
        start.y,
        HitTarget::Port(from),
    );
    up(&mut host, &mut editor, end.x, end.y, HitTarget::Port(to));
    assert_eq!(editor.graph().edges.len(), 1);
    assert_eq!(editor.history_len(), 1);
    assert!(!host.connection().active());
    assert!(!host.has_capture());
    editor.undo();
    assert!(editor.graph().edges.is_empty());
}
#[test]
fn port_proximity_is_owned_by_snapping_and_zoom_independent() {
    for zoom in [0.25, 1.0, 3.0] {
        let mut host = PluginHost::with_defaults();
        let mut editor = editor();
        editor.viewport.zoom = zoom;
        let from = PortRef::new("a", "out");
        let to = PortRef::new("b", "in");
        let start = editor
            .viewport
            .world_to_screen(editor.graph().port_position(&from).unwrap());
        let end = editor
            .viewport
            .world_to_screen(editor.graph().port_position(&to).unwrap())
            + Point::new(12.0, 0.0);
        down(
            &mut host,
            &mut editor,
            start.x,
            start.y,
            HitTarget::Port(from.clone()),
        );
        up(&mut host, &mut editor, end.x, end.y, HitTarget::Background);
        assert_eq!(editor.graph().edges.len(), 1);
        host.execute(&mut editor, CanvasCommand::Undo).unwrap();
        host.uninstall("snapping", &mut editor).unwrap();
        down(
            &mut host,
            &mut editor,
            start.x,
            start.y,
            HitTarget::Port(from),
        );
        up(&mut host, &mut editor, end.x, end.y, HitTarget::Background);
        assert!(editor.graph().edges.is_empty());
    }
}
#[test]
fn invalid_connection_is_rejected_and_preview_clears() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    let from = PortRef::new("a", "out");
    down(&mut host, &mut editor, 224.0, 100.0, HitTarget::Port(from));
    let result = up(
        &mut host,
        &mut editor,
        574.0,
        100.0,
        HitTarget::Port(PortRef::new("b", "out")),
    );
    assert!(!result.errors.is_empty());
    assert!(editor.graph().edges.is_empty());
    assert!(!host.connection().active());
    assert!(!host.has_capture());
    assert_eq!(host.cursor(), CanvasCursor::Default);
}
#[test]
fn removing_connection_during_drag_cancels_preview_without_edge() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    down(
        &mut host,
        &mut editor,
        224.0,
        100.0,
        HitTarget::Port(PortRef::new("a", "out")),
    );
    assert!(host.connection().active());
    host.uninstall("connection", &mut editor).unwrap();
    assert!(!host.connection().active());
    assert!(!host.has_capture());
    assert!(editor.graph().edges.is_empty());
}
#[test]
fn replacing_snap_plugin_changes_actual_grid_behavior() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    host.replace(
        SnappingPlugin::new(SnapConfig {
            grid_size: Some(20.0),
            node_alignment: false,
            ..Default::default()
        }),
        &mut editor,
    )
    .unwrap();
    down(
        &mut host,
        &mut editor,
        10.0,
        10.0,
        HitTarget::Node("a".into()),
    );
    movement(&mut host, &mut editor, 28.0, 28.0);
    assert_eq!(
        editor.graph().nodes[&"a".into()].position,
        Point::new(20.0, 20.0)
    );
    host.set_enabled("snapping", false, &mut editor).unwrap();
    assert_eq!(editor.graph().nodes[&"a".into()].position, Point::ZERO);
    assert!(!host.has_capture());
}
struct BlockDefaults;
impl CanvasPlugin for BlockDefaults {
    fn id(&self) -> &str {
        "guard"
    }
    fn priority(&self) -> i32 {
        -100
    }
    fn on_event(&mut self, _: &CanvasEvent, _: &PluginContext<'_>, out: &mut PluginOutput) {
        out.prevent_default();
    }
}
#[test]
fn custom_prevent_default_blocks_builtin_behavior_without_mutating_editor() {
    let mut host = PluginHost::with_defaults();
    host.install(BlockDefaults).unwrap();
    let mut editor = editor();
    let report = down(
        &mut host,
        &mut editor,
        10.0,
        10.0,
        HitTarget::Node("a".into()),
    );
    assert!(report.default_prevented);
    assert!(editor.selection.is_empty());
    assert!(!host.has_capture());
}
struct ClampResize;
impl CanvasPlugin for ClampResize {
    fn id(&self) -> &str {
        "clamp"
    }
    fn filter_command(
        &mut self,
        command: &mut CanvasCommand,
        _: &PluginContext<'_>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let CanvasCommand::ResizeNode { size, .. } = command {
            size.width = size.width.max(300.0);
        }
        Ok(())
    }
}
#[test]
fn custom_command_middleware_replaces_geometry_policy() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    host.install(ClampResize).unwrap();
    host.execute(
        &mut editor,
        CanvasCommand::ResizeNode {
            node: "a".into(),
            size: Size::new(100.0, 100.0),
        },
    )
    .unwrap();
    assert_eq!(editor.graph().nodes[&"a".into()].size.width, 300.0);
    host.uninstall("clamp", &mut editor).unwrap();
    host.execute(
        &mut editor,
        CanvasCommand::ResizeNode {
            node: "a".into(),
            size: Size::new(100.0, 100.0),
        },
    )
    .unwrap();
    assert_eq!(editor.graph().nodes[&"a".into()].size.width, 100.0);
}
struct Escalate;
impl CanvasPlugin for Escalate {
    fn id(&self) -> &str {
        "escalate"
    }
    fn filter_command(
        &mut self,
        command: &mut CanvasCommand,
        _: &PluginContext<'_>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if matches!(command, CanvasCommand::SelectAll) {
            *command = CanvasCommand::DeleteSelection;
        }
        Ok(())
    }
}
#[test]
fn checked_execution_inspects_rewrites_before_they_mutate_the_editor() {
    let mut host = PluginHost::default();
    host.install(Escalate).unwrap();
    let mut editor = editor();
    editor.selection.select_node("a", false);
    let before = editor.document();
    let result = host.execute_checked(&mut editor, CanvasCommand::SelectAll, |filtered| {
        assert_eq!(filtered, &CanvasCommand::DeleteSelection);
        Err::<(), _>(InteractionError::InvalidInput("changed operation".into()))
    });
    assert!(matches!(result, Err(InteractionError::InvalidInput(_))));
    assert_eq!(editor.document(), before);
    assert!(editor.selection.nodes.contains(&"a".into()));
    assert!(!editor.can_undo());
}
#[test]
fn middleware_cannot_bypass_readonly_by_rewriting_allowed_commands() {
    let mut host = PluginHost::default();
    host.install(Escalate).unwrap();
    let mut editor = editor();
    editor.selection.select_node("a", false);
    host.set_editable(false, &mut editor);
    assert!(matches!(
        host.execute(&mut editor, CanvasCommand::SelectAll),
        Err(InteractionError::CapabilityDenied(Capability::Edit))
    ));
    assert_eq!(editor.graph().nodes.len(), 3);
}
struct Recorder {
    log: Rc<RefCell<Vec<PluginLifecycle>>>,
}
impl CanvasPlugin for Recorder {
    fn id(&self) -> &str {
        "recorder"
    }
    fn on_lifecycle(&mut self, event: PluginLifecycle) {
        self.log.borrow_mut().push(event);
    }
}
#[test]
fn lifecycle_install_disable_enable_uninstall_are_observable() {
    let log = Rc::new(RefCell::new(vec![]));
    let mut host = PluginHost::default();
    let mut editor = editor();
    host.install(Recorder { log: log.clone() }).unwrap();
    host.set_enabled("recorder", false, &mut editor).unwrap();
    host.set_enabled("recorder", true, &mut editor).unwrap();
    host.uninstall("recorder", &mut editor).unwrap();
    assert_eq!(
        *log.borrow(),
        vec![
            PluginLifecycle::Installed,
            PluginLifecycle::Disabled,
            PluginLifecycle::Enabled,
            PluginLifecycle::Uninstalled
        ]
    );
}
#[test]
fn live_install_is_rejected_and_replace_cancels_active_gesture_first() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    start_drag(&mut host, &mut editor);
    assert!(matches!(
        host.install(ClampResize),
        Err(InteractionError::GestureOwned(_))
    ));
    host.replace(ClampResize, &mut editor).unwrap();
    assert_eq!(editor.graph().nodes[&"a".into()].position, Point::ZERO);
    assert!(!editor.transaction_active());
    assert!(!host.has_capture());
    assert!(host.is_enabled("clamp"));
}
#[test]
fn locked_drag_failure_cannot_leave_grabbing_cursor_or_transaction() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    editor
        .transaction("lock", |g| {
            g.nodes.get_mut(&"a".into()).unwrap().locked = true;
            Ok(())
        })
        .unwrap();
    start_drag(&mut host, &mut editor);
    assert_eq!(host.cursor(), CanvasCursor::Default);
    assert!(!editor.transaction_active());
    assert!(!host.has_capture());
}
#[test]
fn marquee_lives_in_selection_plugin_and_cancellation_restores_prior_selection() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    editor.selection.select_node("b", false);
    down(&mut host, &mut editor, -10.0, -10.0, HitTarget::Background);
    movement(&mut host, &mut editor, 240.0, 160.0);
    assert!(host.marquee().is_some());
    host.dispatch(&mut editor, CanvasEvent::FocusLost);
    assert_eq!(editor.selection.nodes.len(), 1);
    assert!(editor.selection.nodes.contains(&"b".into()));
    assert!(host.marquee().is_none());
}
#[test]
fn invalid_input_and_external_edits_cannot_merge_into_active_gestures() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    start_drag(&mut host, &mut editor);
    assert!(matches!(
        host.execute(&mut editor, CanvasCommand::DeleteSelection),
        Err(InteractionError::GestureOwned(_))
    ));
    let report = movement(&mut host, &mut editor, f32::NAN, 20.0);
    assert!(!report.errors.is_empty());
    assert_eq!(editor.graph().nodes[&"a".into()].position, Point::ZERO);
    assert!(!host.has_capture());
}
#[test]
fn import_is_undoable_but_load_resets_history_and_both_are_gated() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    host.execute(&mut editor, CanvasCommand::ImportGraph(Graph::new()))
        .unwrap();
    assert!(editor.graph().nodes.is_empty());
    host.execute(&mut editor, CanvasCommand::Undo).unwrap();
    assert_eq!(editor.graph().nodes.len(), 3);
    host.execute(
        &mut editor,
        CanvasCommand::LoadDocument(FlowDocument::default()),
    )
    .unwrap();
    assert!(!editor.can_undo());
}
#[test]
fn readonly_keyboard_never_requests_paste_or_history_but_copy_is_allowed() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    host.set_editable(false, &mut editor);
    for key in [
        KeyCode::Delete,
        KeyCode::Character('v'),
        KeyCode::Character('z'),
    ] {
        let report = host.dispatch(
            &mut editor,
            CanvasEvent::KeyDown {
                key,
                modifiers: Modifiers {
                    control: true,
                    ..Default::default()
                },
            },
        );
        assert!(!report.document_changed);
        assert!(
            !report
                .effects
                .iter()
                .any(|effect| matches!(effect, CanvasEffect::RequestPaste))
        );
    }
    let report = host.dispatch(
        &mut editor,
        CanvasEvent::KeyDown {
            key: KeyCode::Character('c'),
            modifiers: Modifiers {
                control: true,
                ..Default::default()
            },
        },
    );
    assert!(
        report
            .effects
            .iter()
            .any(|effect| matches!(effect, CanvasEffect::Clipboard(_)))
    );
}
#[test]
fn save_and_explicit_copy_document_shortcuts_are_distinct() {
    let mut host = PluginHost::with_defaults();
    let mut editor = editor();
    for shift in [false, true] {
        let report = host.dispatch(
            &mut editor,
            CanvasEvent::KeyDown {
                key: KeyCode::Character('s'),
                modifiers: Modifiers {
                    control: true,
                    shift,
                    ..Default::default()
                },
            },
        );
        assert!(report.effects.iter().any(|effect| if shift {
            matches!(effect, CanvasEffect::RequestCopyDocument)
        } else {
            matches!(effect, CanvasEffect::RequestSave)
        }));
    }
}
