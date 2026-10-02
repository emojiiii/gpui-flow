use gpui_flow_core::{Point, Size};
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{EngineError, LayoutProvider, SceneEngine};
use gpui_flow_interaction::scene::plugins;
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{Affine, DisplayId, DisplayObject, Scene, Shape, display as d};
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
    fn pair() -> Self {
        let mut scene = Scene::new();
        add(
            &mut scene,
            "a",
            d::Rect::new(80., 40.),
            Affine::translation(20., 30.),
            None,
        );
        add(
            &mut scene,
            "b",
            d::Rect::new(60., 30.),
            Affine::translation(160., 100.),
            None,
        );
        Self::new(scene, &["a", "b"])
    }
    fn geometry(&self) -> SceneRotationGeometry {
        SceneRotationGeometry::for_selection(&self.editor).unwrap()
    }
    fn event(&mut self, event: CanvasEvent) -> RoutedReport {
        self.router
            .dispatch(&mut self.editor, &mut self.host, event)
    }
    fn dispatch(&mut self, event: CanvasEvent) {
        let result = self.event(event);
        assert!(result.errors.is_empty(), "{:?}", result.errors);
        assert!(
            result.default.errors.is_empty(),
            "{:?}",
            result.default.errors
        );
    }
    fn press(&mut self, position: Point, shift: bool, space: bool) {
        let mut input = PointerInput::left(position, HitTarget::SelectionRotateHandle);
        input.modifiers.shift = shift;
        input.modifiers.space = space;
        self.dispatch(CanvasEvent::PointerDown(input));
    }
    fn down(&mut self) -> Point {
        let start = self
            .geometry()
            .screen_handle(&self.editor)
            .unwrap()
            .position;
        self.press(start, false, false);
        start
    }
    fn movement(&mut self, position: Point, shift: bool) {
        let mut input = PointerInput::left(position, HitTarget::Background);
        input.modifiers.shift = shift;
        self.dispatch(CanvasEvent::PointerMove(input));
    }
    fn up(&mut self, position: Point) {
        self.dispatch(CanvasEvent::PointerUp(PointerInput::left(
            position,
            HitTarget::Background,
        )));
    }
    fn world(&self, id: &str) -> Affine {
        self.editor.engine.world_transform(&id.into()).unwrap()
    }
    fn rotated_pointer(&self, start: Point, pivot: Point, degrees: f32) -> Point {
        self.editor.viewport.world_to_screen(
            Affine::rotation(degrees)
                .around(pivot)
                .transform_point(self.editor.viewport.screen_to_world(start)),
        )
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
fn matrix_close(a: Affine, b: Affine) {
    for p in [Point::ZERO, Point::new(37., 0.), Point::new(0., 21.)] {
        let (a, b) = (a.transform_point(p), b.transform_point(p));
        assert!(a.distance(b) < 0.008, "{a:?} != {b:?}");
    }
}
#[test]
fn single_and_shared_rotate_about_frozen_center_with_one_undo() {
    for single in [false, true] {
        let mut lab = Lab::pair();
        if single {
            lab.editor.selection.select("a".into(), false);
        }
        let before = lab.editor.scene().clone();
        let old = [lab.world("a"), lab.world("b")];
        let pivot = lab.geometry().pivot;
        let start = lab.down();
        assert_eq!(lab.host.capture_owner(), Some("rotate"));
        let revision = lab.editor.engine.revision();
        for degrees in [20., 45., 90.] {
            lab.movement(lab.rotated_pointer(start, pivot, degrees), false);
            let delta = Affine::rotation(degrees).around(pivot);
            matrix_close(lab.world("a"), delta * old[0]);
            matrix_close(lab.world("b"), if single { old[1] } else { delta * old[1] });
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
#[test]
fn transformed_parents_reflections_negative_origins_and_zoom_use_world_rotation() {
    for single in [false, true] {
        let mut scene = Scene::new();
        add(
            &mut scene,
            "pa",
            d::Group::new(),
            Affine::translation(100., 50.) * Affine::rotation(31.) * Affine::scale(-2., 0.5),
            None,
        );
        add(
            &mut scene,
            "pb",
            d::Group::new(),
            Affine::translation(350., 130.) * Affine::skew(20., -8.) * Affine::scale(1., -1.),
            None,
        );
        add(
            &mut scene,
            "a",
            d::Polygon::new(vec![
                Point::new(-80., -40.),
                Point::new(20., -40.),
                Point::new(20., 10.),
                Point::new(-80., 10.),
            ]),
            Affine::rotation(18.),
            Some("pa"),
        );
        add(
            &mut scene,
            "b",
            d::Rect::new(60., 30.),
            Affine::translation(12., 16.),
            Some("pb"),
        );
        let mut lab = Lab::new(scene, if single { &["a"] } else { &["a", "b"] });
        lab.editor.viewport.zoom = 2.5;
        lab.editor.viewport.pan = Point::new(71., -43.);
        let pivot = lab.geometry().pivot;
        if single {
            assert!(pivot.distance(lab.world("a").transform_point(Point::new(-30., -15.))) < 0.001);
        }
        let old = [lab.world("a"), lab.world("b")];
        let start = lab.down();
        lab.movement(lab.rotated_pointer(start, pivot, 57.), false);
        let delta = Affine::rotation(57.).around(pivot);
        matrix_close(lab.world("a"), delta * old[0]);
        matrix_close(lab.world("b"), if single { old[1] } else { delta * old[1] });
        assert!(lab.world("a").determinant() < 0.);
    }
}
#[test]
fn selected_parent_child_deduplicates_even_for_one_independent_root() {
    for pair in [false, true] {
        let mut scene = Scene::new();
        add(
            &mut scene,
            "a",
            d::Group::new(),
            Affine::translation(20., 30.),
            None,
        );
        add(
            &mut scene,
            "child",
            d::Rect::new(80., 40.),
            Affine::translation(10., 5.),
            Some("a"),
        );
        add(
            &mut scene,
            "b",
            d::Rect::new(60., 30.),
            Affine::translation(160., 100.),
            None,
        );
        let mut lab = Lab::new(
            scene,
            if pair {
                &["a", "child", "b"]
            } else {
                &["a", "child"]
            },
        );
        assert_eq!(
            lab.geometry().selection.roots.len(),
            if pair { 2 } else { 1 }
        );
        let local = lab
            .editor
            .scene()
            .get(&"child".into())
            .unwrap()
            .object
            .transform;
        let old = lab.world("child");
        let pivot = lab.geometry().pivot;
        let start = lab.down();
        lab.movement(lab.rotated_pointer(start, pivot, 70.), false);
        assert_eq!(
            lab.editor
                .scene()
                .get(&"child".into())
                .unwrap()
                .object
                .transform,
            local
        );
        matrix_close(
            lab.world("child"),
            Affine::rotation(70.).around(pivot) * old,
        );
    }
}
#[test]
fn angular_unwrap_crosses_pi_both_ways_and_keeps_multiple_turn_snap_phase() {
    for sign in [-1., 1.] {
        let mut lab = Lab::pair();
        lab.host.uninstall("rotate", &mut lab.editor).unwrap();
        lab.host
            .install(plugins::SceneRotatePlugin::with_snap_gap(100.).unwrap())
            .unwrap();
        let old = lab.world("a");
        let pivot = lab.geometry().pivot;
        let start = lab
            .editor
            .viewport
            .world_to_screen(pivot + Point::new(-100., 1.));
        lab.press(start, false, false);
        for degrees in [
            10., 90., 170., 190., 270., 350., 370., 450., 530., 550., 630., 710., 730.,
        ] {
            lab.movement(lab.rotated_pointer(start, pivot, degrees * sign), false);
            matrix_close(
                lab.world("a"),
                Affine::rotation(degrees * sign).around(pivot) * old,
            );
        }
        lab.movement(lab.rotated_pointer(start, pivot, 730. * sign), true);
        matrix_close(
            lab.world("a"),
            Affine::rotation(700. * sign).around(pivot) * old,
        );
    }
}
#[test]
fn default_45_snap_and_configurable_gap_keep_unsnapped_accumulator() {
    assert!(plugins::SceneRotatePlugin::with_snap_gap(0.).is_none());
    assert!(plugins::SceneRotatePlugin::with_snap_gap(f32::NAN).is_none());
    assert!(plugins::SceneRotatePlugin::with_snap_gap(361.).is_none());
    for gap in [45., 15.] {
        let mut lab = Lab::pair();
        if gap != 45. {
            lab.host.uninstall("rotate", &mut lab.editor).unwrap();
            lab.host
                .install(plugins::SceneRotatePlugin::with_snap_gap(gap).unwrap())
                .unwrap();
        }
        let old = lab.world("a");
        let pivot = lab.geometry().pivot;
        let selection = lab.editor.selection.clone();
        let start = lab.down();
        let end = lab.rotated_pointer(start, pivot, 38.);
        lab.movement(end, true);
        matrix_close(lab.world("a"), Affine::rotation(45.).around(pivot) * old);
        lab.movement(end, false);
        matrix_close(lab.world("a"), Affine::rotation(38.).around(pivot) * old);
        assert_eq!(lab.editor.selection, selection);
    }
}
#[test]
fn off_center_no_motion_return_and_full_turn_preserve_exact_matrices_and_redo() {
    for single in [false, true] {
        let mut lab = Lab::pair();
        if single {
            lab.editor.selection.select("a".into(), false);
        }
        let pivot = lab.geometry().pivot;
        let start = lab.down();
        lab.movement(lab.rotated_pointer(start, pivot, 60.), false);
        lab.up(start);
        let redo = lab.editor.scene().clone();
        assert!(lab.editor.undo());
        let before = lab.editor.scene().clone();
        let start =
            lab.geometry().screen_handle(&lab.editor).unwrap().position + Point::new(3., -2.);
        lab.press(start, false, false);
        lab.movement(start, false);
        assert_eq!(*lab.editor.scene(), before);
        lab.movement(lab.rotated_pointer(start, pivot, 60.), false);
        lab.movement(start, false);
        assert_eq!(*lab.editor.scene(), before);
        for degrees in [90., 180., 270.] {
            lab.movement(lab.rotated_pointer(start, pivot, degrees), false);
        }
        lab.movement(start, false);
        lab.up(start);
        assert_eq!(*lab.editor.scene(), before);
        assert!(lab.editor.redo());
        assert_eq!(*lab.editor.scene(), redo);
    }
}
#[test]
fn center_sample_is_ignored_and_nonfinite_input_cancels_without_jump() {
    let mut lab = Lab::pair();
    let pivot = lab.geometry().pivot;
    lab.press(pivot, false, false);
    assert!(!lab.host.has_capture());
    let before = lab.editor.scene().clone();
    let start = lab.down();
    lab.movement(lab.rotated_pointer(start, pivot, 30.), false);
    let changed = lab.editor.scene().clone();
    lab.movement(pivot, false);
    assert_eq!(*lab.editor.scene(), changed);
    lab.movement(lab.rotated_pointer(start, pivot, 60.), false);
    assert_ne!(*lab.editor.scene(), changed);
    let _ = lab.event(CanvasEvent::PointerMove(PointerInput::left(
        Point::new(f32::NAN, 0.),
        HitTarget::Background,
    )));
    assert_eq!(*lab.editor.scene(), before);
    assert!(!lab.host.has_capture());
}
#[test]
fn handle_offset_and_hit_radius_are_screen_constant_and_ignore_effect_bounds() {
    let mut lab = Lab::pair();
    let pivot = lab.geometry().pivot;
    lab.editor
        .transaction(|s| {
            s.update_object(&"a".into(), |o| {
                o.style.shadow = Some(gpui_flow_scene::Shadow::new(
                    100.,
                    70.,
                    30.,
                    gpui_flow_scene::Color::BLACK,
                ))
            })
        })
        .unwrap();
    assert_eq!(lab.geometry().pivot, pivot);
    for zoom in [0.25, 1., 3.] {
        lab.editor.viewport.zoom = zoom;
        lab.editor.viewport.pan = Point::new(70., 40.);
        let h = lab.geometry().screen_handle(&lab.editor).unwrap();
        assert!((h.position.distance(h.stem) - 28.).abs() < 0.001);
        assert!(hit_rotation_handle(h, h.position + Point::new(10., 0.)));
        assert!(!hit_rotation_handle(h, h.position + Point::new(10.1, 0.)));
    }
}
#[test]
fn native_custom_locked_singular_and_zero_area_members_suppress_entire_rotation() {
    for single in [false, true] {
        for mode in 0..7 {
            let mut lab = Lab::pair();
            lab.editor
                .transaction(|scene| {
                    scene.update_object(&"b".into(), |o| match mode {
                        0 => o.shape = d::Text::new("native", Size::new(60., 30.)).into(),
                        1 => {
                            o.shape = d::Text::new("hidden native", Size::new(60., 30.)).into();
                            o.visible = false;
                        }
                        2 => o.locked = true,
                        3 => o.transform = Affine::scale(0., 1.),
                        4 => {
                            o.shape = d::Custom::new(
                                "custom",
                                gpui_flow_core::Rect::new(Point::ZERO, Size::new(60., 30.)),
                                vec![],
                            )
                            .into()
                        }
                        5 => o.shape = d::Line::new(Point::ZERO, Point::new(60., 0.)).into(),
                        _ => o.shape = d::Group::new().into(),
                    })?;
                    if mode == 6 {
                        scene.insert(
                            DisplayObject::new("child", d::Text::new("child", Size::new(60., 30.))),
                            Some("b".into()),
                            None,
                        )?;
                    }
                    Ok(())
                })
                .unwrap();
            if single {
                lab.editor.selection.select("b".into(), false);
            }
            let before = lab.editor.scene().clone();
            assert!(SceneRotationGeometry::for_selection(&lab.editor).is_none());
            lab.press(Point::ZERO, false, false);
            assert!(!lab.host.has_capture());
            assert_eq!(*lab.editor.scene(), before);
            let transforms = lab
                .editor
                .selection
                .roots(lab.editor.scene())
                .iter()
                .map(|id| (lab.editor.engine.handle(id).unwrap(), Affine::rotation(30.)))
                .collect();
            assert!(
                lab.host
                    .execute(
                        &mut lab.editor,
                        SceneCommand::SetRotationTransforms { transforms }
                    )
                    .is_err()
            );
            assert_eq!(*lab.editor.scene(), before);
        }
    }
}
struct ChangeBatch(u8);
impl CanvasPlugin<SceneEditor, SceneCommand> for ChangeBatch {
    fn id(&self) -> &str {
        "change-batch"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetRotationTransforms { transforms } = command {
            match self.0 {
                0 => {
                    transforms.pop();
                }
                1 => transforms.push(transforms[0]),
                2 => transforms.swap(0, 1),
                3 => transforms[1].0 = transforms[0].0,
                4 => transforms[1].1 = Affine::scale(0., 1.),
                5 => {
                    *command = SceneCommand::SetResizeTransforms {
                        transforms: transforms.clone(),
                    }
                }
                6 => *command = SceneCommand::ClearSelection,
                _ => {
                    for (_, m) in transforms {
                        m.tx = m.tx.round();
                        m.ty = m.ty.round();
                    }
                }
            }
        }
        Ok(())
    }
}
#[test]
fn middleware_cannot_change_batch_kind_membership_or_invalidate_second_member() {
    for mode in 0..7 {
        let mut lab = Lab::pair();
        lab.host.install(ChangeBatch(mode)).unwrap();
        let before = lab.editor.scene().clone();
        let pivot = lab.geometry().pivot;
        let start = lab.down();
        let end = lab.rotated_pointer(start, pivot, 30.);
        let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
            end,
            HitTarget::Background,
        )));
        assert!(!report.default.errors.is_empty(), "mode {mode}");
        assert_eq!(*lab.editor.scene(), before);
        assert!(!lab.host.has_capture());
    }
}
#[test]
fn allowed_matrix_normalization_keeps_atomic_revision_and_one_history() {
    let mut lab = Lab::pair();
    lab.host.install(ChangeBatch(7)).unwrap();
    let pivot = lab.geometry().pivot;
    let before = lab.editor.scene().clone();
    let start = lab.down();
    let revision = lab.editor.engine.revision();
    lab.movement(lab.rotated_pointer(start, pivot, 33.), false);
    assert_eq!(lab.editor.engine.revision(), revision + 1);
    lab.up(start);
    assert!(lab.editor.undo());
    assert_eq!(*lab.editor.scene(), before);
}
struct InvalidDerived(u8);
impl LayoutProvider for InvalidDerived {
    fn derive(&self, _: &SceneEngine, scene: &Scene) -> Result<Scene, EngineError> {
        let mut next = scene.clone();
        if scene.get(&"a".into()).unwrap().object.transform.b != 0. {
            if self.0 == 0 {
                return Err(EngineError::Layout("second member failed".into()));
            }
            next.update_object(&"b".into(), |o| {
                if self.0 == 1 {
                    o.shape = d::Text::new("native", Size::new(60., 30.)).into();
                    o.transform = Affine::IDENTITY;
                } else if self.0 == 2 {
                    o.locked = true;
                } else {
                    o.shape = d::Line::new(Point::ZERO, Point::new(60., 0.)).into();
                }
            })?;
        }
        Ok(next)
    }
}
#[test]
fn layout_failure_or_derived_native_locked_or_zero_area_never_commits_partial_batch() {
    for mode in 0..4 {
        let mut lab = Lab::pair();
        lab.editor
            .engine
            .install_layout_provider(InvalidDerived(mode))
            .unwrap();
        let before = lab.editor.scene().clone();
        let pivot = lab.geometry().pivot;
        let start = lab.down();
        let revision = lab.editor.engine.revision();
        let end = lab.rotated_pointer(start, pivot, 30.);
        let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
            end,
            HitTarget::Background,
        )));
        assert!(!report.default.errors.is_empty());
        assert_eq!(*lab.editor.scene(), before);
        assert_eq!(lab.editor.engine.revision(), revision);
        assert!(!lab.host.has_capture());
    }
}
#[test]
fn stale_replacement_reparent_detach_document_selection_and_ancestor_do_not_replay() {
    for single in [false, true] {
        for mode in 0..8 {
            let mut lab = Lab::pair();
            if single {
                lab.editor.selection.select("b".into(), false);
            }
            let pivot = lab.geometry().pivot;
            let start = lab.down();
            lab.movement(lab.rotated_pointer(start, pivot, 30.), false);
            match mode {
                0 => {
                    lab.editor
                        .transaction(|s| s.remove(&"b".into()).map(|_| ()))
                        .unwrap();
                    lab.editor
                        .transaction(|s| {
                            s.insert(
                                DisplayObject::new("b", d::Rect::new(31., 27.))
                                    .with_transform(Affine::translation(420., 320.)),
                                None,
                                None,
                            )
                        })
                        .unwrap();
                }
                1 => {
                    lab.editor
                        .transaction(|s| {
                            s.insert(DisplayObject::new("parent", d::Group::new()), None, None)?;
                            s.reparent(&"b".into(), Some("parent".into()), None, false)
                        })
                        .unwrap();
                }
                2 => {
                    let t = lab.editor.engine.handle(&"b".into()).unwrap();
                    let (d, _) = lab.editor.engine.detach(t).unwrap();
                    lab.editor.engine.attach(&d, None, None).unwrap();
                }
                3 => {
                    lab.editor
                        .engine
                        .reset_scene(lab.editor.scene().clone())
                        .unwrap();
                }
                4 => {
                    lab.editor.engine = SceneEngine::new(lab.editor.scene().clone()).unwrap();
                }
                5 => {
                    lab.editor.selection.select("a".into(), false);
                }
                6 => {
                    lab.editor
                        .transaction(|s| {
                            s.update_object(&"b".into(), |o| {
                                o.shape = d::Text::new("external", Size::new(60., 30.)).into()
                            })
                        })
                        .unwrap();
                }
                _ => {
                    lab.editor
                        .transaction(|s| s.update_object(&"b".into(), |o| o.locked = true))
                        .unwrap();
                }
            }
            let external = lab.editor.scene().clone();
            let selection = lab.editor.selection.clone();
            lab.movement(start, false);
            assert_eq!(*lab.editor.scene(), external, "mode {mode}");
            assert_eq!(lab.editor.selection, selection);
            assert!(!lab.host.has_capture());
            assert!(!lab.editor.editing());
        }
    }
}
#[test]
fn changed_ancestor_of_single_rotation_invalidates_before_policy_cancellation() {
    let mut scene = Scene::new();
    add(&mut scene, "p", d::Group::new(), Affine::IDENTITY, None);
    add(
        &mut scene,
        "a",
        d::Rect::new(80., 40.),
        Affine::IDENTITY,
        Some("p"),
    );
    let mut lab = Lab::new(scene, &["a"]);
    let pivot = lab.geometry().pivot;
    let start = lab.down();
    lab.movement(lab.rotated_pointer(start, pivot, 30.), false);
    lab.editor
        .transaction(|s| s.update_object(&"p".into(), |o| o.transform = Affine::rotation(11.)))
        .unwrap();
    let external = lab.editor.scene().clone();
    lab.host.set_editable(false, &mut lab.editor);
    assert_eq!(*lab.editor.scene(), external);
    assert!(!lab.host.has_capture());
}
#[test]
fn input_and_tool_cancellation_restore_owned_batch_and_allow_restart() {
    for mode in 0..8 {
        let mut lab = Lab::pair();
        let before = lab.editor.scene().clone();
        let pivot = lab.geometry().pivot;
        let start = lab.down();
        lab.movement(lab.rotated_pointer(start, pivot, 30.), false);
        match mode {
            0 => lab.dispatch(CanvasEvent::KeyDown {
                key: KeyCode::Escape,
                modifiers: Modifiers::default(),
            }),
            1 => lab.dispatch(CanvasEvent::FocusLost),
            2 => lab.dispatch(CanvasEvent::PointerCancel),
            3 => {
                let mut p = PointerInput::left(start, HitTarget::Background);
                p.button = None;
                lab.dispatch(CanvasEvent::PointerMove(p));
            }
            4 => {
                lab.host.set_editable(false, &mut lab.editor);
                lab.host.set_editable(true, &mut lab.editor);
            }
            5 => {
                lab.host
                    .set_enabled("rotate", false, &mut lab.editor)
                    .unwrap();
                lab.host
                    .set_enabled("rotate", true, &mut lab.editor)
                    .unwrap();
            }
            6 => {
                let mut t = SceneToolController::new();
                t.register_outer("inspect").unwrap();
                t.select_outer(&mut lab.editor, &mut lab.host, "inspect")
                    .unwrap();
            }
            _ => {
                lab.host.uninstall("rotate", &mut lab.editor).unwrap();
                lab.host
                    .install(plugins::SceneRotatePlugin::default())
                    .unwrap();
            }
        }
        assert_eq!(*lab.editor.scene(), before, "mode {mode}");
        assert!(!lab.host.has_capture());
        let start = lab.down();
        lab.movement(lab.rotated_pointer(start, pivot, 40.), false);
        lab.up(start);
        assert!(lab.editor.undo());
        assert_eq!(*lab.editor.scene(), before);
    }
}
#[test]
fn foreign_pointer_and_leave_do_not_end_rotation() {
    let mut lab = Lab::pair();
    let before = lab.editor.scene().clone();
    let pivot = lab.geometry().pivot;
    let start = lab.down();
    let end = lab.rotated_pointer(start, pivot, 30.);
    let mut p = PointerInput::left(end, HitTarget::Background);
    p.pointer_id = 99;
    lab.dispatch(CanvasEvent::PointerMove(p.clone()));
    lab.dispatch(CanvasEvent::PointerUp(p));
    assert_eq!(*lab.editor.scene(), before);
    assert!(lab.host.has_capture());
    lab.dispatch(CanvasEvent::PointerLeave);
    lab.movement(end, false);
    lab.up(end);
    assert!(!lab.host.has_capture());
    assert_ne!(*lab.editor.scene(), before);
}
#[test]
fn readonly_space_pan_and_text_inner_owner_keep_rotation_out() {
    let mut lab = Lab::pair();
    let before = lab.editor.scene().clone();
    let selection = lab.editor.selection.clone();
    let start = lab.geometry().screen_handle(&lab.editor).unwrap().position;
    lab.host.set_editable(false, &mut lab.editor);
    lab.press(start, false, false);
    assert!(!lab.host.has_capture());
    lab.host.set_editable(true, &mut lab.editor);
    lab.press(start, false, true);
    assert_eq!(lab.host.capture_owner(), Some("pan"));
    lab.movement(start + Point::new(30., 20.), false);
    lab.up(start);
    assert_eq!(*lab.editor.scene(), before);
    assert_eq!(lab.editor.selection, selection);
    assert_eq!(lab.editor.viewport.pan, Point::new(30., 20.));
    lab.editor
        .transaction(|s| {
            s.insert(
                DisplayObject::new("text", d::Text::new("content", Size::new(60., 30.))),
                None,
                None,
            )
        })
        .unwrap();
    let mut tools = SceneToolController::with_text();
    let target = lab.editor.engine.handle(&"text".into()).unwrap();
    let token = tools
        .open_text(&mut lab.editor, &mut lab.host, target)
        .unwrap();
    lab.press(start, false, false);
    assert!(!lab.host.has_capture());
    assert_eq!(tools.active_token(), Some(token));
    tools.cancel(&mut lab.editor, &mut lab.host, token).unwrap();
}
#[test]
fn pointer_rotation_hover_uses_crosshair_and_background_resets() {
    let mut lab = Lab::pair();
    lab.host
        .set_enabled("resize", false, &mut lab.editor)
        .unwrap();
    let start = lab.geometry().screen_handle(&lab.editor).unwrap().position;
    lab.dispatch(CanvasEvent::PointerMove(PointerInput::left(
        start,
        HitTarget::SelectionRotateHandle,
    )));
    assert_eq!(lab.host.cursor(), CanvasCursor::Crosshair);
    lab.movement(Point::ZERO, false);
    assert_eq!(lab.host.cursor(), CanvasCursor::Default);
}
struct RejectSecond(usize);
impl CanvasPlugin<SceneEditor, SceneCommand> for RejectSecond {
    fn id(&self) -> &str {
        "reject-second"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetRotationTransforms { transforms } = command {
            self.0 += 1;
            if self.0 == 2 {
                transforms.last_mut().unwrap().1 = Affine::scale(0., 1.);
            }
        }
        Ok(())
    }
}
#[test]
fn rejected_later_move_rolls_back_all_roots_and_preserves_existing_redo() {
    let mut lab = Lab::pair();
    let pivot = lab.geometry().pivot;
    let start = lab.down();
    lab.movement(lab.rotated_pointer(start, pivot, 30.), false);
    lab.up(start);
    let redo = lab.editor.scene().clone();
    assert!(lab.editor.undo());
    let before = lab.editor.scene().clone();
    lab.host.install(RejectSecond(0)).unwrap();
    let start = lab.down();
    lab.movement(lab.rotated_pointer(start, pivot, 40.), false);
    let end = lab.rotated_pointer(start, pivot, 60.);
    let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
        end,
        HitTarget::Background,
    )));
    assert!(!report.default.errors.is_empty());
    assert_eq!(*lab.editor.scene(), before);
    assert!(!lab.host.has_capture());
    assert!(lab.editor.redo());
    assert_eq!(*lab.editor.scene(), redo);
}

#[test]
fn image_rotation_under_affine_parent_preserves_geometry_identity_and_one_undo() {
    let mut s = Scene::new();
    add(
        &mut s,
        "p",
        d::Group::new(),
        Affine::translation(180., 130.) * Affine::rotation(20.) * Affine::scale(-1.4, 0.8),
        None,
    );
    add(
        &mut s,
        "i",
        d::Image::new("fixture", Size::new(80., 40.)).with_corners([10., 0., 5., 0.]),
        Affine::translation(20., 10.),
        Some("p"),
    );
    let mut lab = Lab::new(s, &["i"]);
    lab.editor.viewport.zoom = 1.5;
    let before = lab.editor.scene().clone();
    let h = lab.editor.engine.handle(&"i".into()).unwrap();
    let shape = lab.editor.engine.object(h).unwrap().shape.clone();
    let old = lab.world("i");
    let pivot = lab.geometry().pivot;
    let start = lab.down();
    lab.movement(lab.rotated_pointer(start, pivot, 37.), false);
    let after = lab.editor.scene().clone();
    lab.up(start);
    matrix_close(lab.world("i"), Affine::rotation(37.).around(pivot) * old);
    assert_eq!(lab.editor.engine.object(h).unwrap().shape, shape);
    assert!(lab.editor.undo());
    assert_eq!(lab.editor.scene(), &before);
    assert!(lab.editor.redo());
    assert_eq!(lab.editor.scene(), &after);
    assert_eq!(lab.editor.engine.handle(&"i".into()), Some(h));
}
#[test]
fn image_and_vector_shared_rotation_deduplicates_selected_image_ancestor() {
    let mut s = Scene::new();
    add(
        &mut s,
        "p",
        d::Group::new(),
        Affine::translation(30., 50.),
        None,
    );
    add(
        &mut s,
        "i",
        d::Image::new("fixture", Size::new(80., 40.)),
        Affine::translation(10., 5.),
        Some("p"),
    );
    add(
        &mut s,
        "v",
        d::Rect::new(40., 40.),
        Affine::translation(180., 100.),
        None,
    );
    let mut lab = Lab::new(s, &["p", "i", "v"]);
    let child = lab
        .editor
        .scene()
        .get(&"i".into())
        .unwrap()
        .object
        .transform;
    let roots = [lab.world("p"), lab.world("v")];
    let pivot = lab.geometry().pivot;
    let start = lab.down();
    lab.movement(lab.rotated_pointer(start, pivot, 45.), false);
    lab.up(start);
    let delta = Affine::rotation(45.).around(pivot);
    matrix_close(lab.world("p"), delta * roots[0]);
    matrix_close(lab.world("v"), delta * roots[1]);
    assert_eq!(
        lab.editor
            .scene()
            .get(&"i".into())
            .unwrap()
            .object
            .transform,
        child
    );
}
#[test]
fn image_rotation_cancellation_and_retyped_native_descendant_keep_capture_safe() {
    for external in [false, true] {
        let mut lab = Lab::pair();
        lab.editor
            .transaction(|s| {
                s.update_object(&"b".into(), |o| {
                    o.shape = d::Image::new("fixture", Size::new(60., 30.)).into()
                })
            })
            .unwrap();
        let before = lab.editor.scene().clone();
        let pivot = lab.geometry().pivot;
        let start = lab.down();
        lab.movement(lab.rotated_pointer(start, pivot, 30.), false);
        if external {
            lab.editor
                .transaction(|s| {
                    s.update_object(&"b".into(), |o| {
                        o.shape = d::Text::new("native", Size::new(60., 30.)).into()
                    })
                })
                .unwrap();
            let changed = lab.editor.scene().clone();
            lab.movement(start, false);
            assert_eq!(lab.editor.scene(), &changed);
        } else {
            lab.host.cancel(&mut lab.editor);
            assert_eq!(lab.editor.scene(), &before);
        }
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.editing());
    }
}
