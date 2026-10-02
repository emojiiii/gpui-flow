use gpui_flow_core::{Point, Rect, Size, Viewport};
use gpui_flow_editor::SceneEditor;
use gpui_flow_interaction::{
    scene::{plugins::*, *},
    *,
};
use gpui_flow_scene::{Affine, DisplayObject, Scene, display as d};
use std::time::Duration;
fn rect(x: f32, y: f32, w: f32, h: f32) -> Rect {
    Rect::new(Point::new(x, y), Size::new(w, h))
}
fn editor() -> SceneEditor {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("a", d::Rect::new(20., 10.))
                .with_transform(Affine::translation(10., 10.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("b", d::Rect::new(20., 10.))
                .with_transform(Affine::translation(70., 40.)),
            None,
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    editor.selection.select("a".into(), false);
    editor
}
fn dispatch(host: &mut ScenePluginHost, editor: &mut SceneEditor, event: CanvasEvent) {
    let result = host.dispatch(editor, event);
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}
fn down(host: &mut ScenePluginHost, editor: &mut SceneEditor, p: Point, hit: HitTarget) {
    dispatch(
        host,
        editor,
        CanvasEvent::PointerDown(PointerInput::left(p, hit)),
    );
}
fn movement(host: &mut ScenePluginHost, editor: &mut SceneEditor, p: Point) {
    dispatch(
        host,
        editor,
        CanvasEvent::PointerMove(PointerInput::left(p, HitTarget::Background)),
    );
}
fn up(host: &mut ScenePluginHost, editor: &mut SceneEditor, p: Point) {
    dispatch(
        host,
        editor,
        CanvasEvent::PointerUp(PointerInput::left(p, HitTarget::Background)),
    );
}
fn tick(host: &mut ScenePluginHost, editor: &mut SceneEditor, ms: u64) {
    let result = host.advance_navigation(editor, Duration::from_millis(ms));
    assert!(result.errors.is_empty(), "{:?}", result.errors);
}
fn constrain_drag(host: &mut ScenePluginHost, bounds: DragBounds) {
    host.install(SceneDragConstraintsPlugin {
        constraints: SceneDragConstraints {
            bounds: Some(bounds),
            axis: DragAxis::Both,
        },
    })
    .unwrap();
}
fn move_to(host: &mut ScenePluginHost, editor: &mut SceneEditor, x: f32, y: f32) {
    host.execute(
        editor,
        SceneCommand::MoveSelectionTo {
            anchor: Point::new(x, y),
        },
    )
    .unwrap();
}
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}

#[test]
fn world_bounds_keep_multiselection_together_with_one_undo() {
    let mut editor = editor();
    editor.selection.select("b".into(), true);
    let before = editor.scene().clone();
    let mut host = ScenePluginHost::with_defaults();
    constrain_drag(&mut host, DragBounds::World(rect(0., 0., 100., 80.)));
    down(
        &mut host,
        &mut editor,
        Point::new(15., 15.),
        HitTarget::Object("a".into()),
    );
    movement(&mut host, &mut editor, Point::new(150., 150.));
    assert_eq!(
        selection_bounds(&editor).unwrap(),
        Some(rect(20., 40., 80., 40.))
    );
    movement(&mut host, &mut editor, Point::new(-100., -100.));
    assert_eq!(
        selection_bounds(&editor).unwrap(),
        Some(rect(0., 0., 80., 40.))
    );
    up(&mut host, &mut editor, Point::new(-100., -100.));
    assert!(editor.undo());
    assert_eq!(*editor.scene(), before);
    assert!(!editor.undo());
    assert!(editor.redo());
}
#[test]
fn rotated_reflected_skewed_parent_uses_parent_local_bounds() {
    for transform in [
        Affine::translation(100., 80.) * Affine::rotation(37.),
        Affine::translation(100., 80.) * Affine::scale(-2., 0.5),
        Affine::translation(100., 80.) * Affine::skew(20., -15.),
    ] {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("parent", d::Group::new()).with_transform(transform),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new("a", d::Rect::new(20., 10.))
                    .with_transform(Affine::translation(10., 10.)),
                Some("parent".into()),
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        editor.selection.select("a".into(), false);
        let mut host = ScenePluginHost::with_defaults();
        constrain_drag(&mut host, DragBounds::ParentLocal(rect(0., 0., 100., 80.)));
        let anchor = selection_bounds(&editor).unwrap().unwrap().origin;
        let delta = transform.transform_vector(Point::new(500., 500.));
        move_to(
            &mut host,
            &mut editor,
            anchor.x + delta.x,
            anchor.y + delta.y,
        );
        let actual = editor.scene().get(&"a".into()).unwrap().object.transform;
        near(actual.tx, 80.);
        near(actual.ty, 70.);
    }
}
#[test]
fn parent_local_rejects_mixed_parents_atomically() {
    let mut editor = editor();
    editor
        .transaction(|scene| {
            scene.insert(DisplayObject::new("p", d::Group::new()), None, None)?;
            scene.reparent(&"b".into(), Some("p".into()), None, false)
        })
        .unwrap();
    editor.selection.select("b".into(), true);
    let before = editor.scene().clone();
    let mut host = ScenePluginHost::with_defaults();
    constrain_drag(&mut host, DragBounds::ParentLocal(rect(0., 0., 100., 100.)));
    assert!(
        host.execute(
            &mut editor,
            SceneCommand::MoveSelectionTo {
                anchor: Point::ZERO
            }
        )
        .is_err()
    );
    assert_eq!(*editor.scene(), before);
}
#[test]
fn oversize_selection_centers_in_impossible_axis_without_jitter() {
    let mut editor = editor();
    let mut host = ScenePluginHost::with_defaults();
    constrain_drag(&mut host, DragBounds::World(rect(0., 0., 10., 40.)));
    move_to(&mut host, &mut editor, 100., 100.);
    assert_eq!(
        selection_bounds(&editor).unwrap(),
        Some(rect(-5., 30., 20., 10.))
    );
    move_to(&mut host, &mut editor, -100., -100.);
    assert_eq!(
        selection_bounds(&editor).unwrap(),
        Some(rect(-5., 0., 20., 10.))
    );
}
#[test]
fn axis_lock_and_grid_and_bounds_keep_truthful_guides() {
    let mut editor = editor();
    let mut host = ScenePluginHost::with_defaults();
    host.install(SceneSnappingPlugin::default()).unwrap();
    host.install(SceneDragConstraintsPlugin {
        constraints: SceneDragConstraints {
            bounds: Some(DragBounds::World(rect(0., 0., 55., 55.))),
            axis: DragAxis::X,
        },
    })
    .unwrap();
    move_to(&mut host, &mut editor, 38., 37.);
    assert_eq!(
        selection_bounds(&editor).unwrap(),
        Some(rect(35., 10., 20., 10.))
    );
    assert!(host.guides().is_empty());
}
#[test]
fn grid_and_edge_snapping_use_logical_screen_tolerance_at_every_zoom() {
    for zoom in [0.5, 1., 2.] {
        let mut editor = editor();
        editor.viewport.zoom = zoom;
        let mut host = ScenePluginHost::with_defaults();
        host.install(SceneEdgeSnappingPlugin {
            threshold_pixels: 6.,
            include_centers: false,
        })
        .unwrap();
        move_to(&mut host, &mut editor, 50. - 5. / zoom, 15.);
        near(selection_bounds(&editor).unwrap().unwrap().max_x(), 70.);
        assert!(
            host.guides()
                .iter()
                .any(|guide| guide.axis == Axis::X && guide.position == 70.)
        );
        host.set_enabled("edge-snapping", false, &mut editor)
            .unwrap();
        move_to(&mut host, &mut editor, 50. - 5. / zoom, 15.);
        near(
            selection_bounds(&editor).unwrap().unwrap().origin.x,
            50. - 5. / zoom,
        );
        host.install(SceneSnappingPlugin::default()).unwrap();
        move_to(&mut host, &mut editor, 40. - 5. / zoom, 15.);
        near(selection_bounds(&editor).unwrap().unwrap().origin.x, 40.);
    }
}
#[test]
fn cancel_disable_readonly_and_button_loss_roll_back_constrained_drag() {
    for mode in 0..5 {
        let mut editor = editor();
        let before = editor.scene().clone();
        let mut host = ScenePluginHost::with_defaults();
        constrain_drag(&mut host, DragBounds::World(rect(0., 0., 100., 80.)));
        down(
            &mut host,
            &mut editor,
            Point::new(15., 15.),
            HitTarget::Object("a".into()),
        );
        movement(&mut host, &mut editor, Point::new(50., 50.));
        match mode {
            0 => {
                host.dispatch(&mut editor, CanvasEvent::FocusLost);
            }
            1 => {
                host.set_editable(false, &mut editor);
            }
            2 => {
                host.set_enabled("drag-constraints", false, &mut editor)
                    .unwrap();
            }
            3 => {
                host.dispatch(&mut editor, CanvasEvent::PointerCancel);
            }
            _ => {
                host.dispatch(
                    &mut editor,
                    CanvasEvent::PointerMove(PointerInput {
                        position: Point::new(70., 70.),
                        ..Default::default()
                    }),
                );
            }
        }
        assert_eq!(*editor.scene(), before, "{mode}");
        assert!(!host.has_capture());
        assert!(!editor.undo());
    }
}
#[test]
fn unrelated_pointer_cannot_move_or_release_drag() {
    let mut editor = editor();
    let mut host = ScenePluginHost::with_defaults();
    down(
        &mut host,
        &mut editor,
        Point::new(15., 15.),
        HitTarget::Object("a".into()),
    );
    let before = editor.scene().clone();
    let input = PointerInput {
        pointer_id: 7,
        ..PointerInput::left(Point::new(80., 80.), HitTarget::Background)
    };
    dispatch(
        &mut host,
        &mut editor,
        CanvasEvent::PointerMove(input.clone()),
    );
    dispatch(&mut host, &mut editor, CanvasEvent::PointerUp(input));
    assert_eq!(*editor.scene(), before);
    assert!(host.has_capture());
}
#[test]
fn stale_selection_attachment_and_document_do_not_restore_old_drag_snapshot() {
    for mode in 0..4 {
        let mut editor = editor();
        let mut host = ScenePluginHost::with_defaults();
        down(
            &mut host,
            &mut editor,
            Point::new(15., 15.),
            HitTarget::Object("a".into()),
        );
        movement(&mut host, &mut editor, Point::new(30., 30.));
        match mode {
            0 => {
                let target = editor.engine.handle(&"a".into()).unwrap();
                let (subtree, _) = editor.engine.detach(target).unwrap();
                editor.engine.attach(&subtree, None, None).unwrap();
            }
            1 => {
                let scene = editor.scene().clone();
                editor.engine.reset_scene(scene).unwrap();
            }
            2 => {
                editor.selection.select("b".into(), false);
            }
            _ => {
                editor
                    .transaction(|scene| {
                        scene.update_object(&"b".into(), |object| object.transform.tx = 500.)
                    })
                    .unwrap();
            }
        }
        let external = editor.scene().clone();
        movement(&mut host, &mut editor, Point::new(90., 90.));
        assert_eq!(*editor.scene(), external, "{mode}");
        assert!(!host.has_capture());
        assert!(!editor.gesture_editing());
    }
}
#[test]
fn viewport_clamps_pan_zoom_fit_and_centers_small_content() {
    let mut editor = editor();
    let mut host = ScenePluginHost::with_defaults();
    host.install(SceneViewportConstraintsPlugin {
        constraints: SceneViewportConstraints {
            min_zoom: Some(0.5),
            max_zoom: Some(2.),
            bounds: Some(ViewportBounds {
                world: rect(0., 0., 1000., 800.),
                canvas: Size::new(500., 400.),
            }),
        },
    })
    .unwrap();
    host.execute(
        &mut editor,
        SceneCommand::Pan {
            delta: Point::new(-9999., 9999.),
        },
    )
    .unwrap();
    assert_eq!(editor.viewport.pan, Point::new(-500., 0.));
    host.execute(
        &mut editor,
        SceneCommand::Zoom {
            position: Point::new(250., 200.),
            zoom: 3.,
        },
    )
    .unwrap();
    assert_eq!(editor.viewport.zoom, 2.);
    host.execute(
        &mut editor,
        SceneCommand::Fit {
            canvas: Size::new(500., 400.),
            padding: 20.,
        },
    )
    .unwrap();
    assert_eq!(editor.viewport.zoom, 2.);
    host.execute(
        &mut editor,
        SceneCommand::Zoom {
            position: Point::ZERO,
            zoom: 0.1,
        },
    )
    .unwrap();
    assert_eq!(editor.viewport.zoom, 0.5);
    assert_eq!(editor.viewport.pan, Point::ZERO);
    let constrained = SceneViewportConstraints {
        bounds: Some(ViewportBounds {
            world: rect(10., 20., 100., 80.),
            canvas: Size::new(500., 400.),
        }),
        ..Default::default()
    }
    .constrain(Viewport::default())
    .unwrap();
    assert_eq!(constrained.pan, Point::new(190., 140.));
}
#[test]
fn constrained_zoom_preserves_cursor_anchor_away_from_bounds() {
    let mut editor = editor();
    let mut host = ScenePluginHost::with_defaults();
    host.install(SceneViewportConstraintsPlugin {
        constraints: SceneViewportConstraints {
            min_zoom: Some(0.5),
            max_zoom: Some(2.),
            bounds: None,
        },
    })
    .unwrap();
    let point = Point::new(120., 80.);
    let world = editor.viewport.screen_to_world(point);
    host.execute(
        &mut editor,
        SceneCommand::Zoom {
            position: point,
            zoom: 3.,
        },
    )
    .unwrap();
    assert_eq!(editor.viewport.screen_to_world(point), world);
}
#[test]
fn invalid_constraints_and_viewport_are_atomic_errors() {
    let mut editor = editor();
    let original = editor.viewport;
    let mut host = ScenePluginHost::with_defaults();
    host.install(SceneViewportConstraintsPlugin {
        constraints: SceneViewportConstraints {
            min_zoom: Some(4.),
            ..Default::default()
        },
    })
    .unwrap();
    assert!(
        host.execute(
            &mut editor,
            SceneCommand::Pan {
                delta: Point::new(1., 1.)
            }
        )
        .is_err()
    );
    assert_eq!(editor.viewport, original);
    host.uninstall("viewport-constraints", &mut editor).unwrap();
    assert!(
        host.execute(
            &mut editor,
            SceneCommand::SetViewport {
                viewport: Viewport {
                    zoom: f32::NAN,
                    ..original
                }
            }
        )
        .is_err()
    );
    assert_eq!(editor.viewport, original);
}
#[test]
fn wheel_policy_does_not_guess_devices_or_double_apply() {
    let mut editor = editor();
    let mut host = ScenePluginHost::with_defaults();
    let wheel = SceneWheelPolicy::ZoomOnly;
    host.replace(
        ScenePanPlugin::default().with_policy(ScenePanPolicy {
            wheel,
            ..Default::default()
        }),
        &mut editor,
    )
    .unwrap();
    host.replace(
        SceneZoomPlugin::default().with_wheel_policy(wheel),
        &mut editor,
    )
    .unwrap();
    dispatch(
        &mut host,
        &mut editor,
        CanvasEvent::Wheel {
            position: Point::ZERO,
            delta: Point::new(20., -20.),
            modifiers: Modifiers::default(),
        },
    );
    assert!(editor.viewport.zoom > 1.);
    assert_eq!(editor.viewport.pan, Point::ZERO);
    host.replace(
        ScenePanPlugin::default().with_policy(ScenePanPolicy {
            wheel: SceneWheelPolicy::PanOnly,
            shift_wheel_horizontal: true,
            ..Default::default()
        }),
        &mut editor,
    )
    .unwrap();
    host.replace(
        SceneZoomPlugin::default().with_wheel_policy(SceneWheelPolicy::PanOnly),
        &mut editor,
    )
    .unwrap();
    let zoom = editor.viewport.zoom;
    dispatch(
        &mut host,
        &mut editor,
        CanvasEvent::Wheel {
            position: Point::ZERO,
            delta: Point::new(0., 20.),
            modifiers: Modifiers {
                shift: true,
                control: true,
                ..Default::default()
            },
        },
    );
    assert_eq!(editor.viewport.zoom, zoom);
    assert_eq!(editor.viewport.pan, Point::new(20., 0.));
}

fn flick() -> (ScenePluginHost, SceneEditor) {
    let mut editor = editor();
    let mut host = ScenePluginHost::with_defaults();
    host.replace(
        ScenePanPlugin::default().with_inertia(PanInertiaOptions::default()),
        &mut editor,
    )
    .unwrap();
    tick(&mut host, &mut editor, 0);
    down(
        &mut host,
        &mut editor,
        Point::new(200., 200.),
        HitTarget::Background,
    );
    tick(&mut host, &mut editor, 20);
    movement(&mut host, &mut editor, Point::new(220., 200.));
    tick(&mut host, &mut editor, 21);
    up(&mut host, &mut editor, Point::new(220., 200.));
    assert!(host.needs_navigation_frame());
    (host, editor)
}
#[test]
fn fake_clock_inertia_moves_decays_and_stops_without_document_history() {
    let (mut host, mut editor) = flick();
    let before = editor.scene().clone();
    let start = editor.viewport.pan.x;
    tick(&mut host, &mut editor, 41);
    let first = editor.viewport.pan.x - start;
    assert!(first > 0.);
    tick(&mut host, &mut editor, 61);
    let second = editor.viewport.pan.x - start - first;
    assert!(second > 0. && second < first);
    tick(&mut host, &mut editor, 4000);
    assert!(!host.needs_navigation_frame());
    let final_pan = editor.viewport.pan;
    tick(&mut host, &mut editor, 5000);
    assert_eq!(editor.viewport.pan, final_pan);
    assert_eq!(*editor.scene(), before);
    assert!(!editor.undo());
}
#[test]
fn inertia_is_frame_partition_independent_and_caps_extreme_velocity() {
    let options = PanInertiaOptions::default();
    let mut a = PanInertia::new(Point::new(1000., 400.), Duration::ZERO, options).unwrap();
    let mut b = a.clone();
    let full = a.advance(Duration::from_millis(200));
    let mut sum = Point::ZERO;
    for ms in (10..=200).step_by(10) {
        sum += b.advance(Duration::from_millis(ms));
    }
    near(full.x, sum.x);
    near(full.y, sum.y);
    let extreme = PanInertia::new(Point::new(f32::MAX, f32::MAX), Duration::ZERO, options).unwrap();
    near(extreme.velocity().distance(Point::ZERO), options.max_speed);
    assert!(
        PanInertia::new(
            Point::new(100., 100.),
            Duration::ZERO,
            PanInertiaOptions {
                decay_per_second: 0.,
                ..options
            }
        )
        .is_none()
    );
}
#[test]
fn backwards_clock_fails_and_cancels_inertia() {
    let (mut host, mut editor) = flick();
    let before = editor.viewport;
    let report = host.advance_navigation(&mut editor, Duration::from_millis(19));
    assert!(!report.errors.is_empty());
    assert!(!host.needs_navigation_frame());
    assert_eq!(editor.viewport, before);
}
#[test]
fn inertia_cancels_on_interrupt_policy_plugin_or_external_viewport_changes() {
    for mode in 0..8 {
        let (mut host, mut editor) = flick();
        match mode {
            0 => {
                dispatch(&mut host, &mut editor, CanvasEvent::FocusLost);
            }
            1 => {
                host.set_enabled("pan", false, &mut editor).unwrap();
            }
            2 => {
                host.set_policy(AccessPolicy::locked(), &mut editor);
            }
            3 => {
                host.replace(ScenePanPlugin::default(), &mut editor)
                    .unwrap();
            }
            4 => {
                dispatch(
                    &mut host,
                    &mut editor,
                    CanvasEvent::KeyDown {
                        key: KeyCode::Escape,
                        modifiers: Modifiers::default(),
                    },
                );
            }
            5 => {
                dispatch(
                    &mut host,
                    &mut editor,
                    CanvasEvent::Wheel {
                        position: Point::ZERO,
                        delta: Point::new(0., 4.),
                        modifiers: Modifiers::default(),
                    },
                );
            }
            6 => {
                down(
                    &mut host,
                    &mut editor,
                    Point::new(15., 15.),
                    HitTarget::Control,
                );
            }
            _ => {
                editor.viewport.pan = Point::new(999., 999.);
            }
        }
        let before = editor.viewport;
        tick(&mut host, &mut editor, 100);
        assert!(!host.needs_navigation_frame(), "{mode}");
        assert_eq!(editor.viewport, before, "{mode}");
    }
}
#[test]
fn bounds_stop_inertia_without_bounce_or_busy_frames() {
    let (mut host, mut editor) = flick();
    host.install(SceneViewportConstraintsPlugin {
        constraints: SceneViewportConstraints {
            bounds: Some(ViewportBounds {
                world: rect(-20., 0., 500., 400.),
                canvas: Size::new(500., 400.),
            }),
            ..Default::default()
        },
    })
    .unwrap();
    tick(&mut host, &mut editor, 41);
    assert_eq!(editor.viewport.pan.x, 20.);
    tick(&mut host, &mut editor, 61);
    assert!(!host.needs_navigation_frame());
    assert_eq!(editor.viewport.pan.x, 20.);
}
#[test]
fn legacy_default_has_no_inertia_and_readonly_still_allows_navigation() {
    let mut editor = editor();
    let mut host = ScenePluginHost::with_defaults();
    host.set_policy(AccessPolicy::read_only(), &mut editor);
    tick(&mut host, &mut editor, 0);
    down(
        &mut host,
        &mut editor,
        Point::new(200., 200.),
        HitTarget::Background,
    );
    tick(&mut host, &mut editor, 20);
    movement(&mut host, &mut editor, Point::new(230., 200.));
    up(&mut host, &mut editor, Point::new(230., 200.));
    assert!(!host.needs_navigation_frame());
    assert_eq!(editor.viewport.pan.x, 30.);
}
#[test]
fn release_after_pause_does_not_fling() {
    let mut editor = editor();
    let mut host = ScenePluginHost::with_defaults();
    host.replace(
        ScenePanPlugin::default().with_inertia(PanInertiaOptions::default()),
        &mut editor,
    )
    .unwrap();
    down(
        &mut host,
        &mut editor,
        Point::new(200., 200.),
        HitTarget::Background,
    );
    tick(&mut host, &mut editor, 20);
    movement(&mut host, &mut editor, Point::new(230., 200.));
    tick(&mut host, &mut editor, 200);
    up(&mut host, &mut editor, Point::new(230., 200.));
    assert!(!host.needs_navigation_frame());
}
struct FrameEdit;
impl CanvasPlugin<SceneEditor, SceneCommand> for FrameEdit {
    fn id(&self) -> &str {
        "frame-edit"
    }
    fn on_frame(
        &mut self,
        _: Duration,
        _: &PluginContext<'_, SceneEditor>,
        out: &mut PluginOutput<SceneCommand>,
    ) {
        out.command(SceneCommand::DeleteSelection);
    }
}
#[test]
fn frame_hooks_cannot_bypass_edit_or_capture_boundaries() {
    let mut editor = editor();
    let before = editor.scene().clone();
    let mut host = ScenePluginHost::with_defaults();
    host.install(FrameEdit).unwrap();
    let report = host.advance_navigation(&mut editor, Duration::from_millis(10));
    assert!(!report.errors.is_empty());
    assert_eq!(*editor.scene(), before);
}

#[test]
fn synchronizing_input_clock_does_not_advance_an_unpainted_viewport() {
    let (mut host, mut editor) = flick();
    let before = editor.viewport;
    let report = host.synchronize_navigation_time(&mut editor, Duration::from_millis(200));
    assert!(report.errors.is_empty());
    assert!(!report.viewport_changed);
    assert_eq!(editor.viewport, before);
    assert!(host.needs_navigation_frame());
    assert_eq!(host.navigation_time(), Duration::from_millis(200));
    dispatch(
        &mut host,
        &mut editor,
        CanvasEvent::PointerDown(PointerInput::left(Point::ZERO, HitTarget::Control)),
    );
    tick(&mut host, &mut editor, 300);
    assert_eq!(editor.viewport, before);
    assert!(!host.needs_navigation_frame());
}
#[test]
fn tree_prevented_down_wheel_and_key_still_retire_navigation_tail() {
    use gpui_flow_scene::{EventKind, ListenerOptions};
    for mode in 0..3 {
        let (mut host, mut editor) = flick();
        let mut router = SceneEventRouter::new();
        let target = editor.engine.handle(&"a".into()).unwrap();
        router
            .set_focus(&mut editor, &mut host, Some(target))
            .unwrap();
        tick(&mut host, &mut editor, 40);
        down(
            &mut host,
            &mut editor,
            Point::new(200., 200.),
            HitTarget::Background,
        );
        tick(&mut host, &mut editor, 60);
        movement(&mut host, &mut editor, Point::new(220., 200.));
        tick(&mut host, &mut editor, 61);
        up(&mut host, &mut editor, Point::new(220., 200.));
        assert!(host.needs_navigation_frame());
        let kind = match mode {
            0 => EventKind::PointerDown,
            1 => EventKind::Wheel,
            _ => EventKind::KeyDown,
        };
        editor
            .engine
            .on(target, kind, ListenerOptions::default(), |event| {
                event.prevent_default()
            })
            .unwrap();
        let before = editor.viewport;
        let report = match mode {
            0 => router.dispatch(
                &mut editor,
                &mut host,
                CanvasEvent::PointerDown(PointerInput::left(
                    Point::new(15., 15.),
                    HitTarget::Object("a".into()),
                )),
            ),
            1 => router
                .dispatch_picked_wheel(
                    &mut editor,
                    &mut host,
                    Point::ZERO,
                    Point::new(0., 20.),
                    Modifiers::default(),
                    Some(target),
                )
                .unwrap(),
            _ => router.dispatch(
                &mut editor,
                &mut host,
                CanvasEvent::KeyDown {
                    key: KeyCode::Character('x'),
                    modifiers: Modifiers::default(),
                },
            ),
        };
        assert!(report.default.default_prevented, "{mode}");
        assert!(!host.needs_navigation_frame(), "{mode}");
        tick(&mut host, &mut editor, 200);
        assert_eq!(editor.viewport, before, "{mode}");
    }
}
#[test]
fn invalid_picked_wheel_target_does_not_cancel_tail_or_change_any_state() {
    let (mut host, mut editor) = flick();
    let mut router = SceneEventRouter::new();
    let foreign = crate::editor();
    let target = foreign.engine.handle(&"a".into()).unwrap();
    let before = editor.viewport;
    assert!(
        router
            .dispatch_picked_wheel(
                &mut editor,
                &mut host,
                Point::ZERO,
                Point::new(0., 20.),
                Modifiers::default(),
                Some(target)
            )
            .is_err()
    );
    assert!(host.needs_navigation_frame());
    assert_eq!(editor.viewport, before);
    tick(&mut host, &mut editor, 50);
    assert_ne!(editor.viewport, before);
}
struct RewriteFrame;
impl CanvasPlugin<SceneEditor, SceneCommand> for RewriteFrame {
    fn id(&self) -> &str {
        "rewrite-frame"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if matches!(command, SceneCommand::Pan { .. }) {
            *command = SceneCommand::DeleteSelection;
        }
        Ok(())
    }
}
#[test]
fn middleware_cannot_rewrite_a_navigation_frame_into_an_edit() {
    let (mut host, mut editor) = flick();
    let before = editor.scene().clone();
    host.install(RewriteFrame).unwrap();
    let report = host.advance_navigation(&mut editor, Duration::from_millis(50));
    assert!(!report.errors.is_empty());
    assert_eq!(*editor.scene(), before);
    assert!(!host.needs_navigation_frame());
}
#[test]
fn document_replacement_cancels_old_viewport_tail() {
    let (mut host, mut editor) = flick();
    let scene = editor.scene().clone();
    editor.engine.reset_scene(scene).unwrap();
    let before = editor.viewport;
    tick(&mut host, &mut editor, 50);
    assert_eq!(editor.viewport, before);
    assert!(!host.needs_navigation_frame());
}
#[test]
fn screen_drag_threshold_and_world_displacement_remain_zoom_consistent() {
    for zoom in [0.5, 1., 2.] {
        let mut editor = editor();
        editor.viewport.zoom = zoom;
        editor.viewport.pan = Point::new(50., 30.);
        let mut host = ScenePluginHost::with_defaults();
        let start = editor.viewport.world_to_screen(Point::new(15., 15.));
        down(&mut host, &mut editor, start, HitTarget::Object("a".into()));
        movement(&mut host, &mut editor, start + Point::new(2., 0.));
        assert!(!editor.gesture_editing());
        near(selection_bounds(&editor).unwrap().unwrap().origin.x, 10.);
        movement(&mut host, &mut editor, start + Point::new(20., 0.));
        near(
            selection_bounds(&editor).unwrap().unwrap().origin.x,
            10. + 20. / zoom,
        );
    }
}

#[test]
fn extreme_finite_inertia_options_cannot_emit_nonfinite_displacement() {
    let options = PanInertiaOptions {
        decay_per_second: 0.0001,
        max_speed: f32::MAX,
        ..Default::default()
    };
    let mut inertia = PanInertia::new(Point::new(f32::MAX, 0.), Duration::ZERO, options).unwrap();
    assert_eq!(inertia.advance(Duration::from_secs(10000)), Point::ZERO);
    assert!(!inertia.is_active());
}
