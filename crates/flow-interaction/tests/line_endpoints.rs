use gpui_flow_core::{Point, Size};
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ElementHandle, EngineError, LayoutProvider, SceneEngine};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{
    Affine, DisplayId, DisplayObject, EventKind, ListenerOptions, Scene, Shape, display as d,
};

struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
}
impl Lab {
    fn new(scene: Scene, selected: &[&str]) -> Self {
        let mut editor = SceneEditor::new(scene).unwrap();
        editor.selection.ids = selected.iter().map(|id| (*id).into()).collect();
        Self {
            editor,
            host: ScenePluginHost::with_defaults(),
            router: SceneEventRouter::new(),
        }
    }
    fn line() -> Self {
        let mut scene = Scene::new();
        add(
            &mut scene,
            "line",
            d::Line::new(Point::ZERO, Point::new(100., 0.)),
            Affine::translation(40., 30.),
            None,
        );
        Self::new(scene, &["line"])
    }
    fn geometry(&self) -> SceneLineGeometry {
        SceneLineGeometry::for_selection(&self.editor).unwrap()
    }
    fn shape(&self) -> d::Line {
        let Shape::Line(line) = &self
            .editor
            .scene()
            .get(&"line".into())
            .unwrap()
            .object
            .shape
        else {
            panic!("expected the original built-in line");
        };
        line.clone()
    }
    fn transform(&self) -> Affine {
        self.editor
            .scene()
            .get(&"line".into())
            .unwrap()
            .object
            .transform
    }
    fn event(&mut self, event: CanvasEvent) -> RoutedReport {
        self.router
            .dispatch(&mut self.editor, &mut self.host, event)
    }
    fn dispatch(&mut self, event: CanvasEvent) {
        let report = self.event(event);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(
            report.default.errors.is_empty(),
            "{:?}",
            report.default.errors
        );
    }
    fn press(&mut self, endpoint: LineEndpoint, position: Point, shift: bool, space: bool) {
        let mut pointer = PointerInput::left(
            position,
            HitTarget::LineEndpoint {
                id: "line".into(),
                endpoint,
            },
        );
        pointer.modifiers.shift = shift;
        pointer.modifiers.space = space;
        self.dispatch(CanvasEvent::PointerDown(pointer));
    }
    fn down(&mut self, endpoint: LineEndpoint) -> Point {
        let start = self
            .geometry()
            .screen_handles(&self.editor)
            .unwrap()
            .iter()
            .find(|handle| handle.endpoint == endpoint)
            .unwrap()
            .position;
        self.press(endpoint, start, false, false);
        start
    }
    fn movement(&mut self, position: Point, shift: bool) {
        let mut pointer = PointerInput::left(position, HitTarget::Background);
        pointer.modifiers.shift = shift;
        self.dispatch(CanvasEvent::PointerMove(pointer));
    }
    fn up(&mut self, position: Point) {
        self.dispatch(CanvasEvent::PointerUp(PointerInput::left(
            position,
            HitTarget::Background,
        )));
    }
    fn delta_pointer(&self, start: Point, world: Affine, local_delta: Point) -> Point {
        start + world.transform_vector(local_delta) * self.editor.viewport.zoom
    }
}
fn add(
    scene: &mut Scene,
    id: &str,
    shape: impl Into<Shape>,
    transform: Affine,
    parent: Option<&str>,
) {
    scene
        .insert(
            DisplayObject::new(id, shape).with_transform(transform),
            parent.map(DisplayId::from),
            None,
        )
        .unwrap();
}
fn point_close(actual: Point, expected: Point) {
    assert!(
        actual.distance(expected) < 0.008,
        "{actual:?} != {expected:?}"
    );
}
fn endpoint(line: &d::Line, which: LineEndpoint) -> Point {
    match which {
        LineEndpoint::Start => line.start,
        LineEndpoint::End => line.end,
    }
}
fn other(which: LineEndpoint) -> LineEndpoint {
    match which {
        LineEndpoint::Start => LineEndpoint::End,
        LineEndpoint::End => LineEndpoint::Start,
    }
}

#[test]
fn handles_are_true_endpoints_through_nested_reflections_shear_and_zoom() {
    let mut scene = Scene::new();
    add(
        &mut scene,
        "parent",
        d::Group::new(),
        Affine::translation(210., -70.) * Affine::rotation(31.) * Affine::scale(-2., 0.5),
        None,
    );
    let original = d::Line::new(Point::new(-30., 50.), Point::new(80., -20.));
    add(
        &mut scene,
        "line",
        original.clone(),
        Affine::translation(-15., 22.) * Affine::skew(18., -7.),
        Some("parent"),
    );
    let mut lab = Lab::new(scene, &["line"]);
    assert!(lab.host.plugin_ids().contains(&"line-endpoints"));
    let geometry = lab.geometry();
    assert_eq!(geometry.line, original);
    assert_eq!(
        geometry.target,
        lab.editor.engine.handle(&"line".into()).unwrap()
    );
    assert_eq!(
        geometry.world,
        lab.editor.engine.world_transform(&"line".into()).unwrap()
    );
    for point in [original.start, original.end, Point::new(17., -11.)] {
        point_close(
            geometry
                .inverse
                .transform_point(geometry.world.transform_point(point)),
            point,
        );
    }
    for zoom in [0.25, 1., 3.] {
        lab.editor.viewport.zoom = zoom;
        lab.editor.viewport.pan = Point::new(79., -31.);
        let handles = geometry.screen_handles(&lab.editor).unwrap();
        assert_eq!(handles.len(), 2);
        for which in [LineEndpoint::Start, LineEndpoint::End] {
            let handle = handles
                .iter()
                .find(|handle| handle.endpoint == which)
                .unwrap();
            point_close(
                handle.position,
                lab.editor
                    .viewport
                    .world_to_screen(geometry.world.transform_point(endpoint(&original, which))),
            );
            assert_eq!(
                hit_line_handle(&handles, handle.position).unwrap().endpoint,
                which
            );
            assert_eq!(
                hit_line_handle(&handles, handle.position + Point::new(3., 0.))
                    .unwrap()
                    .endpoint,
                which
            );
        }
        assert!(hit_line_handle(&handles, Point::new(100_000., 100_000.)).is_none());
        assert!(hit_line_handle(&handles, Point::new(f32::NAN, 0.)).is_none());
    }
}

#[test]
fn horizontal_and_vertical_lines_expose_both_endpoints_despite_zero_area_bounds() {
    for end in [Point::new(100., 0.), Point::new(0., 100.)] {
        let mut scene = Scene::new();
        add(
            &mut scene,
            "line",
            d::Line::new(Point::ZERO, end),
            Affine::IDENTITY,
            None,
        );
        let mut lab = Lab::new(scene, &["line"]);
        for which in [LineEndpoint::Start, LineEndpoint::End] {
            let before = lab.editor.scene().clone();
            let start = lab.down(which);
            assert_eq!(lab.host.capture_owner(), Some("line-endpoints"));
            lab.movement(start + Point::new(11., 17.), false);
            assert_ne!(*lab.editor.scene(), before);
            lab.dispatch(CanvasEvent::PointerCancel);
            assert_eq!(*lab.editor.scene(), before);
        }
    }
}

#[test]
fn nearest_endpoint_wins_when_short_line_hit_targets_overlap() {
    let mut scene = Scene::new();
    add(
        &mut scene,
        "line",
        d::Line::new(Point::ZERO, Point::new(0.25, 0.)),
        Affine::IDENTITY,
        None,
    );
    let lab = Lab::new(scene, &["line"]);
    let handles = lab.geometry().screen_handles(&lab.editor).unwrap();
    for handle in &handles {
        assert_eq!(
            hit_line_handle(&handles, handle.position).unwrap().endpoint,
            handle.endpoint
        );
    }
    let midpoint = (handles[0].position + handles[1].position) * 0.5;
    assert_eq!(
        hit_line_handle(&handles, midpoint).unwrap().endpoint,
        LineEndpoint::Start
    );
    assert_eq!(
        hit_line_handle(&[handles[1], handles[0]], midpoint)
            .unwrap()
            .endpoint,
        LineEndpoint::Start
    );
    assert!(hit_line_handle(&[], Point::ZERO).is_none());
}

#[test]
fn both_endpoints_use_frozen_inverse_preserve_opposite_and_transform_and_make_one_undo() {
    for which in [LineEndpoint::Start, LineEndpoint::End] {
        for zoom in [0.25, 2.5] {
            let mut scene = Scene::new();
            add(
                &mut scene,
                "parent",
                d::Group::new(),
                Affine::translation(150., 60.) * Affine::rotation(31.) * Affine::scale(-2., 0.5),
                None,
            );
            let line = d::Line::new(Point::new(-40., 30.), Point::new(90., -20.));
            let local = Affine::translation(12., -16.) * Affine::skew(11., -6.);
            add(&mut scene, "line", line.clone(), local, Some("parent"));
            let mut lab = Lab::new(scene, &["line"]);
            lab.editor.viewport.zoom = zoom;
            lab.editor.viewport.pan = Point::new(71., -43.);
            let world = lab.geometry().world;
            let before = lab.editor.scene().clone();
            let selection = lab.editor.selection.clone();
            let opposite = world.transform_point(endpoint(&line, other(which)));
            let start = lab.down(which);
            let revision = lab.editor.engine.revision();
            for delta in [
                Point::new(17., -9.),
                Point::new(28., 16.),
                Point::new(-12., 25.),
            ] {
                let pointer = lab.delta_pointer(start, world, delta);
                lab.movement(pointer, false);
                let changed = lab.shape();
                point_close(endpoint(&changed, which), endpoint(&line, which) + delta);
                assert_eq!(
                    endpoint(&changed, other(which)),
                    endpoint(&line, other(which))
                );
                assert_eq!(lab.transform(), local);
                point_close(
                    lab.geometry()
                        .world
                        .transform_point(endpoint(&changed, other(which))),
                    opposite,
                );
                assert_eq!(lab.editor.selection, selection);
            }
            assert_eq!(lab.editor.engine.revision(), revision + 3);
            let after = lab.editor.scene().clone();
            lab.up(start);
            assert!(lab.editor.undo());
            assert_eq!(*lab.editor.scene(), before);
            assert!(!lab.editor.undo());
            assert!(lab.editor.redo());
            assert_eq!(*lab.editor.scene(), after);
        }
    }
}

#[test]
fn shift_uses_dominant_local_drag_axis_ties_favor_y_and_releasing_shift_restores_free_delta() {
    for which in [LineEndpoint::Start, LineEndpoint::End] {
        let mut lab = Lab::line();
        lab.editor
            .transaction(|scene| {
                scene.update_object(&"line".into(), |object| {
                    object.transform = Affine::translation(40., 30.) * Affine::scale(-2., 0.5);
                })
            })
            .unwrap();
        lab.editor.viewport.zoom = 2.;
        let geometry = lab.geometry();
        let original = endpoint(&geometry.line, which);
        let start = lab.down(which);
        for (delta, constrained) in [
            (Point::new(20., 7.), Point::new(20., 0.)),
            (Point::new(5., -19.), Point::new(0., -19.)),
            (Point::new(12., 12.), Point::new(0., 12.)),
            (Point::new(-12., -12.), Point::new(0., -12.)),
        ] {
            let pointer = lab.delta_pointer(start, geometry.world, delta);
            lab.movement(pointer, true);
            point_close(endpoint(&lab.shape(), which), original + constrained);
            lab.movement(pointer, false);
            point_close(endpoint(&lab.shape(), which), original + delta);
        }
        lab.dispatch(CanvasEvent::PointerCancel);
        assert_eq!(lab.shape(), geometry.line);
    }
}

#[test]
fn off_center_press_no_motion_and_return_to_origin_preserve_exact_scene_and_redo() {
    for which in [LineEndpoint::Start, LineEndpoint::End] {
        let mut lab = Lab::line();
        let start = lab.down(which);
        lab.movement(start + Point::new(20., 10.), false);
        lab.up(start);
        let redo = lab.editor.scene().clone();
        assert!(lab.editor.undo());
        let before = lab.editor.scene().clone();
        let original = lab.shape();
        let handle = lab
            .geometry()
            .screen_handles(&lab.editor)
            .unwrap()
            .iter()
            .find(|handle| handle.endpoint == which)
            .unwrap()
            .position;
        let start = handle + Point::new(3., -2.);
        lab.press(which, start, false, false);
        lab.movement(start, false);
        assert_eq!(*lab.editor.scene(), before);
        lab.movement(start + Point::new(15., 21.), false);
        point_close(
            endpoint(&lab.shape(), which),
            endpoint(&original, which) + Point::new(15., 21.),
        );
        assert_eq!(
            endpoint(&lab.shape(), other(which)),
            endpoint(&original, other(which))
        );
        lab.movement(start, false);
        lab.up(start);
        assert_eq!(*lab.editor.scene(), before);
        assert!(lab.editor.redo());
        assert_eq!(*lab.editor.scene(), redo);
    }
}

#[test]
fn minimum_is_frozen_world_length_capped_at_one_and_rejected_samples_keep_last_scene() {
    for initial_local_length in [0.25, 10.] {
        let mut scene = Scene::new();
        add(
            &mut scene,
            "line",
            d::Line::new(Point::ZERO, Point::new(initial_local_length, 0.)),
            Affine::translation(40., 30.) * Affine::scale(2., 0.5),
            None,
        );
        let mut lab = Lab::new(scene, &["line"]);
        let geometry = lab.geometry();
        let minimum = (initial_local_length as f64 * 2.).min(1.);
        assert!((geometry.minimum_length - minimum).abs() < 1e-9);
        let start = lab.down(LineEndpoint::End);
        let extended = Point::new(20., 0.);
        lab.movement(
            lab.delta_pointer(start, geometry.world, extended - geometry.line.end),
            false,
        );
        let accepted = lab.editor.scene().clone();
        let revision = lab.editor.engine.revision();
        let too_short = Point::new((minimum / 4.) as f32, 0.);
        lab.movement(
            lab.delta_pointer(start, geometry.world, too_short - geometry.line.end),
            false,
        );
        assert_eq!(*lab.editor.scene(), accepted);
        assert_eq!(lab.editor.engine.revision(), revision);
        assert!(lab.host.has_capture());
        let at_floor = Point::new((minimum / 2.) as f32, 0.);
        lab.movement(
            lab.delta_pointer(start, geometry.world, at_floor - geometry.line.end),
            false,
        );
        assert_eq!(lab.shape().end, at_floor);
        lab.up(start);
    }
}

#[test]
fn zero_length_path_non_line_multiselection_singular_and_inherited_lock_are_excluded() {
    for mode in 0..9 {
        let mut scene = Scene::new();
        let shape: Shape = match mode {
            0 => d::Line::new(Point::ZERO, Point::ZERO).into(),
            1 => d::Path::new(vec![
                d::PathCommand::MoveTo { to: Point::ZERO },
                d::PathCommand::LineTo {
                    to: Point::new(100., 0.),
                },
            ])
            .into(),
            2 => d::Rect::new(100., 40.).into(),
            3 => d::Text::new("native", Size::new(100., 40.)).into(),
            _ => d::Line::new(Point::ZERO, Point::new(100., 0.)).into(),
        };
        let mut object = DisplayObject::new("line", shape);
        if mode == 4 {
            object.transform = Affine::scale(0., 1.);
        }
        if mode == 5 {
            object.locked = true;
        }
        scene.insert(object, None, None).unwrap();
        if mode == 6 {
            let mut parent = DisplayObject::new("parent", d::Group::new());
            parent.locked = true;
            scene.insert(parent, None, None).unwrap();
            scene
                .reparent(&"line".into(), Some("parent".into()), None, false)
                .unwrap();
        }
        if mode == 7 {
            add(
                &mut scene,
                "second",
                d::Line::new(Point::ZERO, Point::new(100., 0.)),
                Affine::IDENTITY,
                None,
            );
        }
        let mut lab = Lab::new(
            scene,
            if mode == 7 {
                &["line", "second"]
            } else if mode == 8 {
                &[]
            } else {
                &["line"]
            },
        );
        let before = lab.editor.scene().clone();
        assert!(
            SceneLineGeometry::for_selection(&lab.editor).is_none(),
            "mode {mode}"
        );
        lab.press(LineEndpoint::End, Point::new(100., 0.), false, false);
        assert!(!lab.host.has_capture(), "mode {mode}");
        assert_eq!(*lab.editor.scene(), before);
    }
}

#[test]
fn direct_command_rejects_stale_handles_detached_instances_wrong_shapes_and_readonly() {
    for mode in 0..5 {
        let mut lab = Lab::line();
        let target = lab.geometry().target;
        match mode {
            0 => {
                lab.editor
                    .transaction(|scene| scene.remove(&"line".into()).map(|_| ()))
                    .unwrap();
                lab.editor
                    .transaction(|scene| {
                        scene.insert(
                            DisplayObject::new(
                                "line",
                                d::Line::new(Point::ZERO, Point::new(70., 0.)),
                            ),
                            None,
                            None,
                        )
                    })
                    .unwrap();
            }
            1 => {
                lab.editor.engine.detach(target).unwrap();
            }
            2 => {
                lab.editor
                    .transaction(|scene| {
                        scene.update_object(&"line".into(), |object| {
                            object.shape = d::Path::new(vec![
                                d::PathCommand::MoveTo { to: Point::ZERO },
                                d::PathCommand::LineTo {
                                    to: Point::new(100., 0.),
                                },
                            ])
                            .into();
                        })
                    })
                    .unwrap();
            }
            3 => {
                lab.editor
                    .transaction(|scene| {
                        scene.update_object(&"line".into(), |object| object.locked = true)
                    })
                    .unwrap();
            }
            _ => {
                lab.host.set_editable(false, &mut lab.editor);
            }
        }
        let before = lab.editor.scene().clone();
        assert!(
            lab.host
                .execute(
                    &mut lab.editor,
                    SceneCommand::SetLineEndpoint {
                        target,
                        endpoint: LineEndpoint::End,
                        position: Point::new(140., 20.),
                    }
                )
                .is_err(),
            "mode {mode}"
        );
        assert_eq!(*lab.editor.scene(), before);
    }
}

#[test]
fn finite_screen_coordinate_that_overflows_world_space_does_not_begin_capture() {
    let mut lab = Lab::line();
    lab.editor.viewport.zoom = 0.1;
    let before = lab.editor.scene().clone();
    // Exercise the plugin host directly: the tree router additionally rejects
    // an event with nonfinite world coordinates before emitting tree listeners.
    let _ = lab.host.dispatch(
        &mut lab.editor,
        CanvasEvent::PointerDown(PointerInput::left(
            Point::new(f32::MAX, 0.),
            HitTarget::LineEndpoint {
                id: "line".into(),
                endpoint: LineEndpoint::End,
            },
        )),
    );
    assert!(!lab.host.has_capture());
    assert!(!lab.editor.editing());
    assert_eq!(*lab.editor.scene(), before);
}

#[test]
fn invalid_pointer_cancels_an_owned_change_without_leaving_history() {
    for invalid in [Point::new(f32::NAN, 0.), Point::new(f32::INFINITY, 0.)] {
        let mut lab = Lab::line();
        let before = lab.editor.scene().clone();
        let start = lab.down(LineEndpoint::End);
        lab.movement(start + Point::new(20., 10.), false);
        let _ = lab.event(CanvasEvent::PointerMove(PointerInput::left(
            invalid,
            HitTarget::Background,
        )));
        assert_eq!(*lab.editor.scene(), before);
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.editing());
        assert!(!lab.editor.undo());
    }
}

#[test]
fn stale_replacement_remount_document_engine_selection_and_external_mutation_never_replay() {
    for mode in 0..10 {
        let mut lab = Lab::line();
        let start = lab.down(LineEndpoint::End);
        lab.movement(start + Point::new(20., 10.), false);
        match mode {
            0 => {
                lab.editor
                    .transaction(|scene| scene.remove(&"line".into()).map(|_| ()))
                    .unwrap();
                lab.editor
                    .transaction(|scene| {
                        scene.insert(
                            DisplayObject::new(
                                "line",
                                d::Line::new(Point::new(7., 9.), Point::new(55., 12.)),
                            )
                            .with_transform(Affine::translation(420., 320.)),
                            None,
                            None,
                        )
                    })
                    .unwrap();
            }
            1 => {
                let target = lab.editor.engine.handle(&"line".into()).unwrap();
                let (detached, _) = lab.editor.engine.detach(target).unwrap();
                lab.editor.engine.attach(&detached, None, None).unwrap();
            }
            2 => {
                lab.editor
                    .engine
                    .reset_scene(lab.editor.scene().clone())
                    .unwrap();
            }
            3 => {
                lab.editor.engine = SceneEngine::new(lab.editor.scene().clone()).unwrap();
            }
            4 => {
                lab.editor.selection.ids.clear();
            }
            5 => {
                lab.editor
                    .transaction(|scene| {
                        scene.update_object(&"line".into(), |object| {
                            object.shape =
                                d::Line::new(Point::new(-9., 5.), Point::new(140., 70.)).into();
                        })
                    })
                    .unwrap();
            }
            6 => {
                lab.editor
                    .transaction(|scene| {
                        scene.update_object(&"line".into(), |object| object.locked = true)
                    })
                    .unwrap();
            }
            7 => {
                lab.editor
                    .transaction(|scene| {
                        scene.insert(DisplayObject::new("parent", d::Group::new()), None, None)?;
                        scene.reparent(&"line".into(), Some("parent".into()), None, false)
                    })
                    .unwrap();
            }
            8 => {
                let target = lab.editor.engine.handle(&"line".into()).unwrap();
                lab.editor.engine.detach(target).unwrap();
            }
            _ => {
                lab.editor
                    .transaction(|scene| {
                        scene.insert(
                            DisplayObject::new("unrelated", d::Rect::new(10., 10.)),
                            None,
                            None,
                        )
                    })
                    .unwrap();
            }
        }
        let external = lab.editor.scene().clone();
        let mut selection = lab.editor.selection.clone();
        // Editor reconciliation prunes IDs removed by a direct-engine detach.
        // It must not resurrect the captured selection or the detached shape.
        selection.retain_existing(&external);
        lab.movement(start, false);
        assert_eq!(*lab.editor.scene(), external, "mode {mode}");
        assert_eq!(lab.editor.selection, selection, "mode {mode}");
        assert!(!lab.host.has_capture(), "mode {mode}");
        assert!(!lab.editor.editing(), "mode {mode}");
    }
}

#[test]
fn changed_ancestor_cannot_be_overwritten_by_policy_cancellation() {
    let mut scene = Scene::new();
    add(
        &mut scene,
        "parent",
        d::Group::new(),
        Affine::IDENTITY,
        None,
    );
    add(
        &mut scene,
        "line",
        d::Line::new(Point::ZERO, Point::new(100., 0.)),
        Affine::IDENTITY,
        Some("parent"),
    );
    let mut lab = Lab::new(scene, &["line"]);
    let start = lab.down(LineEndpoint::End);
    lab.movement(start + Point::new(20., 10.), false);
    lab.editor
        .transaction(|scene| {
            scene.update_object(&"parent".into(), |object| {
                object.transform = Affine::rotation(17.);
            })
        })
        .unwrap();
    let external = lab.editor.scene().clone();
    lab.host.set_editable(false, &mut lab.editor);
    assert_eq!(*lab.editor.scene(), external);
    assert!(!lab.host.has_capture());
    assert!(!lab.editor.editing());
}

#[test]
fn escape_focus_loss_pointer_cancel_disable_and_outer_switch_restore_baseline() {
    for mode in 0..9 {
        let mut lab = Lab::line();
        let before = lab.editor.scene().clone();
        let start = lab.down(LineEndpoint::End);
        lab.movement(start + Point::new(20., 10.), false);
        match mode {
            0 => lab.dispatch(CanvasEvent::KeyDown {
                key: KeyCode::Escape,
                modifiers: Modifiers::default(),
            }),
            1 => lab.dispatch(CanvasEvent::FocusLost),
            2 => lab.dispatch(CanvasEvent::PointerCancel),
            3 => {
                let mut pointer = PointerInput::left(start, HitTarget::Background);
                pointer.button = None;
                lab.dispatch(CanvasEvent::PointerMove(pointer));
            }
            4 => {
                lab.host.set_editable(false, &mut lab.editor);
                lab.host.set_editable(true, &mut lab.editor);
            }
            5 => {
                lab.host
                    .set_enabled("line-endpoints", false, &mut lab.editor)
                    .unwrap();
                lab.host
                    .set_enabled("line-endpoints", true, &mut lab.editor)
                    .unwrap();
            }
            6 => {
                let mut tools = SceneToolController::new();
                tools.register_outer("inspect").unwrap();
                tools
                    .select_outer(&mut lab.editor, &mut lab.host, "inspect")
                    .unwrap();
            }
            7 => {
                let mut tools = SceneToolController::new();
                tools
                    .set_enabled(&mut lab.editor, &mut lab.host, false)
                    .unwrap();
                tools
                    .set_enabled(&mut lab.editor, &mut lab.host, true)
                    .unwrap();
            }
            _ => {
                lab.host
                    .uninstall("line-endpoints", &mut lab.editor)
                    .unwrap();
                lab.host = ScenePluginHost::with_defaults();
            }
        }
        assert_eq!(*lab.editor.scene(), before, "mode {mode}");
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.editing());
        assert!(!lab.editor.undo());
        let restart = lab.down(LineEndpoint::Start);
        lab.movement(restart + Point::new(7., 11.), false);
        lab.up(restart);
        assert!(lab.editor.undo());
        assert_eq!(*lab.editor.scene(), before);
    }
}

#[test]
fn foreign_pointer_and_pointer_leave_do_not_end_or_move_owned_endpoint() {
    let mut lab = Lab::line();
    let before = lab.editor.scene().clone();
    let start = lab.down(LineEndpoint::End);
    let end = start + Point::new(20., 10.);
    let mut foreign = PointerInput::left(end, HitTarget::Background);
    foreign.pointer_id = 99;
    lab.dispatch(CanvasEvent::PointerMove(foreign.clone()));
    lab.dispatch(CanvasEvent::PointerUp(foreign));
    assert_eq!(*lab.editor.scene(), before);
    assert_eq!(lab.host.capture_owner(), Some("line-endpoints"));
    lab.dispatch(CanvasEvent::PointerLeave);
    assert!(lab.host.has_capture());
    lab.movement(end, false);
    lab.up(end);
    assert_eq!(lab.shape().end, Point::new(120., 10.));
    assert!(!lab.host.has_capture());
}

#[test]
fn readonly_space_pan_and_text_session_cannot_start_endpoint_editing() {
    let mut lab = Lab::line();
    let before = lab.editor.scene().clone();
    let selection = lab.editor.selection.clone();
    let start = lab.geometry().screen_handles(&lab.editor).unwrap()[1].position;
    lab.host.set_editable(false, &mut lab.editor);
    lab.press(LineEndpoint::End, start, false, false);
    assert!(!lab.host.has_capture());
    lab.host.set_editable(true, &mut lab.editor);
    lab.press(LineEndpoint::End, start, false, true);
    assert_eq!(lab.host.capture_owner(), Some("pan"));
    lab.movement(start + Point::new(30., 20.), false);
    lab.up(start);
    assert_eq!(*lab.editor.scene(), before);
    assert_eq!(lab.editor.selection, selection);
    assert_eq!(lab.editor.viewport.pan, Point::new(30., 20.));
    lab.editor
        .transaction(|scene| {
            scene.insert(
                DisplayObject::new("text", d::Text::new("content", Size::new(60., 30.))),
                None,
                None,
            )
        })
        .unwrap();
    let mut tools = SceneToolController::with_text();
    let text = lab.editor.engine.handle(&"text".into()).unwrap();
    let token = tools
        .open_text(&mut lab.editor, &mut lab.host, text)
        .unwrap();
    // Keep the line selected so the active text session, rather than selection,
    // is what excludes a new endpoint gesture.
    lab.editor.selection.select("line".into(), false);
    lab.press(LineEndpoint::End, start, false, false);
    assert!(!lab.host.has_capture());
    assert_eq!(tools.active_token(), Some(token));
    tools.cancel(&mut lab.editor, &mut lab.host, token).unwrap();
}

struct ChangeEndpoint {
    mode: u8,
    replacement: ElementHandle,
}
impl CanvasPlugin<SceneEditor, SceneCommand> for ChangeEndpoint {
    fn id(&self) -> &str {
        "change-endpoint"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetLineEndpoint {
            target,
            endpoint,
            position,
        } = command
        {
            match self.mode {
                0 => *target = self.replacement,
                1 => *endpoint = other(*endpoint),
                2 => *command = SceneCommand::ClearSelection,
                3 => *position = Point::new(f32::NAN, 0.),
                4 => *position = Point::new(0.25, 0.),
                _ => *position = Point::new(position.x.round(), position.y.round()),
            }
        }
        Ok(())
    }
}

#[test]
fn middleware_cannot_retarget_target_endpoint_kind_or_make_invalid_or_short_geometry() {
    for mode in 0..5 {
        let mut lab = Lab::line();
        lab.editor
            .transaction(|scene| {
                scene.insert(
                    DisplayObject::new("other", d::Line::new(Point::ZERO, Point::new(80., 0.))),
                    None,
                    None,
                )
            })
            .unwrap();
        let replacement = lab.editor.engine.handle(&"other".into()).unwrap();
        lab.host
            .install(ChangeEndpoint { mode, replacement })
            .unwrap();
        let before = lab.editor.scene().clone();
        let selection = lab.editor.selection.clone();
        let start = lab.down(LineEndpoint::End);
        let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
            start + Point::new(20., 10.),
            HitTarget::Background,
        )));
        assert!(!report.default.errors.is_empty(), "mode {mode}");
        assert_eq!(*lab.editor.scene(), before, "mode {mode}");
        assert_eq!(lab.editor.selection, selection);
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.editing());
    }
}

#[test]
fn middleware_may_normalize_position_without_extra_revision_or_history() {
    let mut lab = Lab::line();
    let replacement = lab.geometry().target;
    lab.host
        .install(ChangeEndpoint {
            mode: 5,
            replacement,
        })
        .unwrap();
    let before = lab.editor.scene().clone();
    let start = lab.down(LineEndpoint::End);
    let revision = lab.editor.engine.revision();
    lab.movement(start + Point::new(20.25, 10.75), false);
    assert_eq!(lab.shape().end, Point::new(120., 11.));
    assert_eq!(lab.editor.engine.revision(), revision + 1);
    lab.up(start);
    assert!(lab.editor.undo());
    assert_eq!(*lab.editor.scene(), before);
    assert!(!lab.editor.undo());
}

struct RejectSecond(usize);
impl CanvasPlugin<SceneEditor, SceneCommand> for RejectSecond {
    fn id(&self) -> &str {
        "reject-second-endpoint"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetLineEndpoint { position, .. } = command {
            self.0 += 1;
            if self.0 == 2 {
                *position = Point::new(0.25, 0.);
            }
        }
        Ok(())
    }
}

#[test]
fn later_invalid_normalization_rolls_back_whole_gesture_and_preserves_existing_redo() {
    let mut lab = Lab::line();
    let start = lab.down(LineEndpoint::End);
    lab.movement(start + Point::new(20., 10.), false);
    lab.up(start);
    let redo = lab.editor.scene().clone();
    assert!(lab.editor.undo());
    let before = lab.editor.scene().clone();
    lab.host.install(RejectSecond(0)).unwrap();
    let start = lab.down(LineEndpoint::End);
    lab.movement(start + Point::new(30., 15.), false);
    let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
        start + Point::new(40., 20.),
        HitTarget::Background,
    )));
    assert!(!report.default.errors.is_empty());
    assert_eq!(*lab.editor.scene(), before);
    assert!(!lab.host.has_capture());
    assert!(!lab.editor.editing());
    assert!(lab.editor.redo());
    assert_eq!(*lab.editor.scene(), redo);
}

struct InvalidDerived(u8);
impl LayoutProvider for InvalidDerived {
    fn derive(&self, _: &SceneEngine, scene: &Scene) -> Result<Scene, EngineError> {
        let mut next = scene.clone();
        let changed = matches!(&scene.get(&"line".into()).unwrap().object.shape,
            Shape::Line(line) if line.end != Point::new(100., 0.));
        if changed {
            if self.0 == 0 {
                return Err(EngineError::Layout("endpoint rejected by layout".into()));
            }
            next.update_object(&"line".into(), |object| match self.0 {
                1 => {
                    object.shape = d::Path::new(vec![
                        d::PathCommand::MoveTo { to: Point::ZERO },
                        d::PathCommand::LineTo {
                            to: Point::new(100., 0.),
                        },
                    ])
                    .into()
                }
                2 => object.locked = true,
                3 => object.transform = Affine::scale(0., 1.),
                4 => object.transform = Affine::translation(900., 900.),
                5 => {
                    if let Shape::Line(line) = &mut object.shape {
                        line.start = Point::new(5., 7.);
                    }
                }
                _ => {
                    if let Shape::Line(line) = &mut object.shape {
                        line.end = line.start;
                    }
                }
            })?;
        }
        Ok(next)
    }
}

#[test]
fn layout_failure_or_invalid_derived_line_never_commits_partial_endpoint_change() {
    for mode in 0..7 {
        let mut lab = Lab::line();
        lab.editor
            .engine
            .install_layout_provider(InvalidDerived(mode))
            .unwrap();
        let before = lab.editor.scene().clone();
        let start = lab.down(LineEndpoint::End);
        let revision = lab.editor.engine.revision();
        let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
            start + Point::new(20., 10.),
            HitTarget::Background,
        )));
        assert!(!report.default.errors.is_empty(), "mode {mode}");
        assert_eq!(*lab.editor.scene(), before, "mode {mode}");
        assert_eq!(lab.editor.engine.revision(), revision, "mode {mode}");
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.editing());
        assert!(!lab.editor.undo());
    }
}

#[test]
fn typed_endpoint_tree_events_can_prevent_down_move_or_up_without_stranding_gesture() {
    for kind in [
        EventKind::PointerDown,
        EventKind::PointerMove,
        EventKind::PointerUp,
    ] {
        let mut lab = Lab::line();
        let target = lab.geometry().target;
        lab.editor
            .engine
            .on(target, kind.clone(), ListenerOptions::default(), |event| {
                event.prevent_default();
            })
            .unwrap();
        let before = lab.editor.scene().clone();
        let start = lab.down(LineEndpoint::End);
        if kind != EventKind::PointerDown {
            assert!(lab.host.has_capture());
            lab.movement(start + Point::new(20., 10.), false);
            if kind == EventKind::PointerMove {
                assert_eq!(*lab.editor.scene(), before);
            } else {
                assert_ne!(*lab.editor.scene(), before);
            }
        }
        lab.up(start);
        assert_eq!(*lab.editor.scene(), before, "{kind:?}");
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.editing());
        assert!(!lab.editor.undo());
    }
}
