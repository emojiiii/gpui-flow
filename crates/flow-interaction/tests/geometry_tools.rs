use gpui_flow_editor::{PathControlGeometry, SceneEditor};
use gpui_flow_engine::{ElementHandle, EngineError, LayoutProvider, SceneEngine};
use gpui_flow_interaction::{scene::plugins::SceneGeometryToolsPlugin, scene::*, *};
use gpui_flow_scene::{display as d, *};

const CONTROL: PathPointId = PathPointId {
    command: 1,
    role: PathPointRole::Control1,
};
fn controls() -> [GeometryControl; 4] {
    [
        GeometryControl::PathPoint(CONTROL),
        GeometryControl::Skew(SkewAxis::X),
        GeometryControl::Skew(SkewAxis::Y),
        GeometryControl::Pivot,
    ]
}
struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
}
impl Lab {
    fn new(control: GeometryControl, pen: bool) -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("parent", d::Group::new()).with_transform(
                    Affine::translation(210., 170.)
                        * Affine::rotation(27.)
                        * Affine::scale(-1.5, 0.8),
                ),
                None,
                None,
            )
            .unwrap();
        let path = d::Path::from_svg("M0 0 Q30 80 80 0 C100 -20 140 -10 150 40 L0 60 Z").unwrap();
        let shape: Shape = if pen {
            d::Pen { path }.into()
        } else {
            path.into()
        };
        scene
            .insert(
                DisplayObject::new("target", shape)
                    .with_transform(Affine::translation(15., 25.) * Affine::skew(12., 0.)),
                Some("parent".into()),
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        editor.selection.select("target".into(), false);
        editor.viewport.zoom = 1.5;
        editor.viewport.pan = Point::new(30., 40.);
        let mut host = ScenePluginHost::with_defaults();
        host.install(SceneGeometryToolsPlugin::new(control.mode()))
            .unwrap();
        Self {
            editor,
            host,
            router: SceneEventRouter::new(),
        }
    }
    fn geometry(&self) -> SceneGeometryTools {
        SceneGeometryTools::for_selection(
            &self.editor,
            self.host.geometry_tool_mode().unwrap(),
            self.host.transform_capabilities(),
        )
        .unwrap()
    }
    fn object(&self) -> &DisplayObject {
        &self.editor.scene().get(&"target".into()).unwrap().object
    }
    fn event(&mut self, event: CanvasEvent) -> RoutedReport {
        self.router
            .dispatch(&mut self.editor, &mut self.host, event)
    }
    fn dispatch(&mut self, event: CanvasEvent) {
        let report = self.event(event);
        assert!(
            report.errors.is_empty() && report.default.errors.is_empty(),
            "{:?} {:?}",
            report.errors,
            report.default.errors
        );
    }
    fn down(&mut self, control: GeometryControl) -> Point {
        let start = self
            .geometry()
            .screen_handles(&self.editor)
            .unwrap()
            .into_iter()
            .find(|h| h.control == control)
            .unwrap()
            .position
            + Point::new(2., -3.);
        self.dispatch(CanvasEvent::PointerDown(PointerInput::left(
            start,
            HitTarget::GeometryControl {
                id: self.editor.selection.ids.iter().next().unwrap().to_string(),
                control,
            },
        )));
        start
    }
    fn moved(&mut self, position: Point) {
        self.dispatch(CanvasEvent::PointerMove(PointerInput::left(
            position,
            HitTarget::Background,
        )));
    }
    fn up(&mut self, position: Point) {
        self.dispatch(CanvasEvent::PointerUp(PointerInput::left(
            position,
            HitTarget::Background,
        )));
    }
    fn delta(&self, start: Point, local: Point) -> Point {
        start + self.geometry().world.transform_vector(local) * self.editor.viewport.zoom
    }
}
fn close(a: Point, b: Point) {
    assert!(a.distance(b) < 0.002, "{a:?} != {b:?}");
}

#[test]
fn path_and_leaf_pen_drag_one_real_control_and_one_undo_through_affine_parent() {
    for pen in [false, true] {
        let control = GeometryControl::PathPoint(CONTROL);
        let mut lab = Lab::new(control, pen);
        let before = lab.editor.scene().clone();
        let original = lab.object().clone();
        let start = lab.down(control);
        let end = lab.delta(start, Point::new(10., 20.));
        lab.moved(lab.delta(start, Point::new(5., 10.)));
        lab.moved(end);
        lab.up(end);
        let mut expected = original;
        // Only the Q control moves; endpoints, C handles, command count and kind survive.
        PathControlGeometry::set_point(&mut expected.shape, CONTROL, Point::new(40., 100.))
            .unwrap();
        let geometry = PathControlGeometry::new(&lab.object().shape, Affine::IDENTITY).unwrap();
        close(geometry.point(CONTROL).unwrap(), Point::new(40., 100.));
        PathControlGeometry::set_point(
            &mut expected.shape,
            CONTROL,
            geometry.point(CONTROL).unwrap(),
        )
        .unwrap();
        assert_eq!(lab.object(), &expected);
        let after = lab.editor.scene().clone();
        assert!(lab.editor.undo());
        assert_eq!(lab.editor.scene(), &before);
        assert!(!lab.editor.undo());
        assert!(lab.editor.redo());
        assert_eq!(lab.editor.scene(), &after);
    }
}

#[test]
fn skew_is_frozen_local_shear_keeps_pivot_and_has_one_history_entry() {
    for axis in [SkewAxis::X, SkewAxis::Y] {
        let control = GeometryControl::Skew(axis);
        let mut lab = Lab::new(control, false);
        lab.editor
            .engine
            .transaction(|s| {
                s.update_object(&"target".into(), |o| o.pivot = Some(Point::new(17., 23.)))
            })
            .unwrap();
        let before = lab.editor.scene().clone();
        let geometry = lab.geometry();
        let start = lab.down(control);
        let end = lab.delta(start, Point::new(25., 18.));
        lab.moved(lab.delta(start, Point::new(10., 8.)));
        lab.moved(end);
        lab.up(end);
        let after = lab.editor.scene().clone();
        let edit = geometry
            .drag(
                control,
                lab.editor.viewport.screen_to_world(start),
                lab.editor.viewport.screen_to_world(end),
                false,
            )
            .unwrap();
        let GeometryEdit::Skew { shear, .. } = edit else {
            panic!()
        };
        assert_eq!(
            lab.object().transform,
            gpui_flow_editor::apply_skew(geometry.original, geometry.pivot, shear, axis).unwrap()
        );
        close(
            after
                .world_transform(&"target".into())
                .unwrap()
                .transform_point(geometry.pivot),
            geometry.world.transform_point(geometry.pivot),
        );
        assert_eq!(
            lab.object().shape,
            before.get(&"target".into()).unwrap().object.shape
        );
        assert!(lab.editor.undo());
        assert_eq!(lab.editor.scene(), &before);
        assert!(!lab.editor.undo());
        assert!(lab.editor.redo());
        assert_eq!(lab.editor.scene(), &after);
    }
}

#[test]
fn pivot_drag_preserves_all_render_geometry_then_pointer_rotation_uses_it() {
    let mut lab = Lab::new(GeometryControl::Pivot, false);
    let before = lab.editor.scene().clone();
    let geometry = lab.geometry();
    let start = lab.down(GeometryControl::Pivot);
    let end = lab.delta(start, Point::new(-30., 18.));
    lab.moved(end);
    lab.up(end);
    let mut expected = before.get(&"target".into()).unwrap().object.clone();
    expected.pivot = lab.object().pivot;
    assert_eq!(lab.object(), &expected);
    assert!(
        lab.editor
            .engine
            .last_change()
            .properties
            .iter()
            .any(|change| change.property == "pivot" && change.old == serde_json::Value::Null)
    );
    close(
        lab.object().pivot.unwrap(),
        geometry.pivot + Point::new(-30., 18.),
    );
    assert_eq!(
        lab.editor.scene().world_bounds(&"target".into()).unwrap(),
        before.world_bounds(&"target".into()).unwrap()
    );
    let after_pivot = lab.editor.scene().clone();
    assert!(lab.editor.undo());
    assert_eq!(lab.editor.scene(), &before);
    assert!(!lab.editor.undo());
    assert!(lab.editor.redo());
    let rotation = SceneRotationGeometry::for_selection(&lab.editor).unwrap();
    close(
        rotation.pivot,
        geometry.world.transform_point(lab.object().pivot.unwrap()),
    );
    let start = lab
        .editor
        .viewport
        .world_to_screen(rotation.pivot + Point::new(100., 0.));
    let end = lab
        .editor
        .viewport
        .world_to_screen(rotation.pivot + Point::new(0., 100.));
    lab.dispatch(CanvasEvent::PointerDown(PointerInput::left(
        start,
        HitTarget::SelectionRotateHandle,
    )));
    lab.moved(end);
    lab.up(end);
    close(
        lab.editor
            .scene()
            .world_transform(&"target".into())
            .unwrap()
            .transform_point(lab.object().pivot.unwrap()),
        rotation.pivot,
    );
    assert_ne!(lab.object().transform, expected.transform);
    assert!(lab.editor.undo());
    assert_eq!(lab.editor.scene(), &after_pivot);
}

#[test]
fn every_control_no_motion_and_return_to_start_preserve_exact_scene_and_redo() {
    for control in controls() {
        let mut lab = Lab::new(control, false);
        let start = lab.down(control);
        lab.moved(lab.delta(start, Point::new(15., 10.)));
        lab.up(start);
        let redo = lab.editor.scene().clone();
        assert!(lab.editor.undo());
        let before = lab.editor.scene().clone();
        for move_away in [false, true] {
            let geometry = lab.geometry();
            let start = lab.down(control);
            if move_away {
                lab.moved(
                    start
                        + geometry.world.transform_vector(Point::new(10., 15.))
                            * lab.editor.viewport.zoom,
                );
            }
            lab.moved(start);
            lab.up(start);
            assert_eq!(lab.editor.scene(), &before, "{control:?}");
            assert!(!lab.editor.undo());
        }
        assert!(lab.editor.redo());
        assert_eq!(lab.editor.scene(), &redo);
    }
}

#[test]
fn all_modes_cancel_on_escape_blur_cancel_readonly_disable_replace_and_uninstall() {
    for control in controls() {
        for cancel in 0..7 {
            let mut lab = Lab::new(control, false);
            let before = lab.editor.scene().clone();
            let start = lab.down(control);
            lab.moved(lab.delta(start, Point::new(18., 9.)));
            assert_ne!(lab.editor.scene(), &before);
            match cancel {
                0 => lab.dispatch(CanvasEvent::KeyDown {
                    key: KeyCode::Escape,
                    modifiers: Modifiers::default(),
                }),
                1 => lab.dispatch(CanvasEvent::FocusLost),
                2 => lab.dispatch(CanvasEvent::PointerCancel),
                3 => {
                    lab.host.set_editable(false, &mut lab.editor);
                }
                4 => {
                    lab.host
                        .set_enabled(GEOMETRY_TOOLS_PLUGIN, false, &mut lab.editor)
                        .unwrap();
                }
                5 => {
                    lab.host
                        .replace(
                            SceneGeometryToolsPlugin::new(GeometryToolMode::Pivot),
                            &mut lab.editor,
                        )
                        .unwrap();
                }
                _ => {
                    lab.host
                        .uninstall(GEOMETRY_TOOLS_PLUGIN, &mut lab.editor)
                        .unwrap();
                }
            }
            assert_eq!(lab.editor.scene(), &before, "{control:?}/{cancel}");
            assert!(!lab.host.has_capture());
            assert!(!lab.editor.editing());
            assert!(!lab.editor.undo());
        }
    }
}

#[test]
fn stale_selection_remount_external_revision_and_parent_changes_do_not_replay() {
    for control in controls() {
        for stale in 0..4 {
            let mut lab = Lab::new(control, false);
            let start = lab.down(control);
            lab.moved(lab.delta(start, Point::new(18., 9.)));
            match stale {
                0 => lab.editor.selection.clear(),
                1 => {
                    let h = lab.geometry().target;
                    let (d, _) = lab.editor.engine.detach(h).unwrap();
                    lab.editor
                        .engine
                        .attach(
                            &d,
                            Some(lab.editor.engine.handle(&"parent".into()).unwrap()),
                            None,
                        )
                        .unwrap();
                }
                2 => {
                    lab.editor
                        .engine
                        .reset_scene(lab.editor.scene().clone())
                        .unwrap();
                }
                _ => {
                    lab.editor
                        .engine
                        .transaction(|s| {
                            s.update_object(&"parent".into(), |o| o.transform.tx += 30.)
                        })
                        .unwrap();
                }
            }
            let external = lab.editor.scene().clone();
            lab.moved(start);
            assert_eq!(lab.editor.scene(), &external, "{control:?}/{stale}");
            assert!(!lab.host.has_capture());
            assert!(!lab.editor.editing());
        }
    }
}

struct Rewrite {
    mode: u8,
    replacement: ElementHandle,
}
impl CanvasPlugin<SceneEditor, SceneCommand> for Rewrite {
    fn id(&self) -> &str {
        "rewrite-geometry"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetGeometry { target, edit } = command {
            match self.mode {
                0 => *target = self.replacement,
                1 => *command = SceneCommand::ClearSelection,
                2 => {
                    *edit = match *edit {
                        GeometryEdit::PathPoint { position, .. } => GeometryEdit::PathPoint {
                            point: PathPointId {
                                command: 2,
                                role: PathPointRole::Control1,
                            },
                            position,
                        },
                        GeometryEdit::Skew { axis, shear } => GeometryEdit::Skew {
                            axis: if axis == SkewAxis::X {
                                SkewAxis::Y
                            } else {
                                SkewAxis::X
                            },
                            shear,
                        },
                        GeometryEdit::Pivot { .. } => GeometryEdit::Skew {
                            axis: SkewAxis::X,
                            shear: 1.,
                        },
                    }
                }
                _ => {
                    *edit = match *edit {
                        GeometryEdit::PathPoint { point, .. } => GeometryEdit::PathPoint {
                            point,
                            position: Point::new(f32::NAN, 0.),
                        },
                        GeometryEdit::Skew { axis, .. } => GeometryEdit::Skew { axis, shear: 17. },
                        GeometryEdit::Pivot { .. } => GeometryEdit::Pivot {
                            position: Some(Point::new(f32::INFINITY, 0.)),
                        },
                    }
                }
            }
        }
        Ok(())
    }
}
#[test]
fn middleware_cannot_retarget_control_kind_axis_or_admit_nonfinite_or_overbudget_values() {
    for control in controls() {
        for mode in 0..4 {
            let mut lab = Lab::new(control, false);
            lab.host
                .install(Rewrite {
                    mode,
                    replacement: lab.editor.engine.handle(&"parent".into()).unwrap(),
                })
                .unwrap();
            let before = lab.editor.scene().clone();
            let start = lab.down(control);
            let end = lab.delta(start, Point::new(18., 9.));
            let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
                end,
                HitTarget::Background,
            )));
            assert!(!report.default.errors.is_empty(), "{control:?}/{mode}");
            assert_eq!(lab.editor.scene(), &before);
            assert!(!lab.host.has_capture());
            assert!(!lab.editor.undo());
        }
    }
}

struct Derived {
    baseline: DisplayObject,
    mode: u8,
}
impl LayoutProvider for Derived {
    fn derive(&self, _: &SceneEngine, scene: &Scene) -> Result<Scene, EngineError> {
        let mut scene = scene.clone();
        if scene.get(&"target".into()).unwrap().object != self.baseline {
            match self.mode {
                0 => scene
                    .update_object(&"target".into(), |o| {
                        if let Shape::Path(p) = &mut o.shape {
                            p.commands[0] = d::PathCommand::MoveTo {
                                to: Point::new(1., 1.),
                            };
                        }
                    })
                    .unwrap(),
                1 => scene
                    .update_object(&"target".into(), |o| o.transform.tx += 4.)
                    .unwrap(),
                2 => scene
                    .update_object(&"target".into(), |o| o.pivot = Some(Point::new(444., 555.)))
                    .unwrap(),
                _ => scene
                    .update_object(&"parent".into(), |o| o.transform.ty += 9.)
                    .unwrap(),
            }
        }
        Ok(scene)
    }
}
#[test]
fn post_layout_must_preserve_other_points_transform_pivot_and_parent_world() {
    for control in controls() {
        for mode in 0..4 {
            let mut lab = Lab::new(control, false);
            lab.editor
                .engine
                .install_layout_provider(Derived {
                    baseline: lab.object().clone(),
                    mode,
                })
                .unwrap();
            let before = lab.editor.scene().clone();
            let revision = lab.editor.engine.revision();
            let start = lab.down(control);
            let end = lab.delta(start, Point::new(18., 9.));
            let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
                end,
                HitTarget::Background,
            )));
            if control == GeometryControl::Pivot {
                // Pivot is render-inert metadata; it must not invoke layout.
                assert!(report.default.errors.is_empty());
                let mut expected = before.clone();
                expected
                    .update_object(&"target".into(), |o| o.pivot = lab.object().pivot)
                    .unwrap();
                assert_eq!(lab.editor.scene(), &expected);
                lab.dispatch(CanvasEvent::PointerCancel);
                assert_eq!(lab.editor.scene(), &before);
            } else {
                assert!(!report.default.errors.is_empty(), "{control:?}/{mode}");
                assert_eq!(lab.editor.scene(), &before);
                assert_eq!(lab.editor.engine.revision(), revision);
            }
            assert!(!lab.editor.undo());
        }
    }
}

#[test]
fn typed_controls_route_prevent_default_and_release_without_stranding_history() {
    for control in controls() {
        for kind in [
            EventKind::PointerDown,
            EventKind::PointerMove,
            EventKind::PointerUp,
        ] {
            let mut lab = Lab::new(control, false);
            lab.editor
                .engine
                .on(
                    lab.geometry().target,
                    kind.clone(),
                    ListenerOptions::default(),
                    |e| e.prevent_default(),
                )
                .unwrap();
            let before = lab.editor.scene().clone();
            let start = lab.down(control);
            if kind != EventKind::PointerDown {
                lab.moved(lab.delta(start, Point::new(18., 9.)));
            }
            lab.up(start);
            assert_eq!(lab.editor.scene(), &before);
            assert!(!lab.host.has_capture());
            assert!(!lab.editor.editing());
        }
    }
}

#[test]
fn invalid_samples_wrong_pointer_and_readonly_are_bounded() {
    for control in controls() {
        let mut lab = Lab::new(control, false);
        let before = lab.editor.scene().clone();
        let start = lab.down(control);
        let mut foreign = PointerInput::left(start + Point::new(30., 40.), HitTarget::Background);
        foreign.pointer_id = 77;
        lab.dispatch(CanvasEvent::PointerMove(foreign.clone()));
        lab.dispatch(CanvasEvent::PointerUp(foreign));
        assert_eq!(lab.editor.scene(), &before);
        assert!(lab.host.has_capture());
        lab.moved(lab.delta(start, Point::new(18., 9.)));
        lab.event(CanvasEvent::PointerMove(PointerInput::left(
            Point::new(f32::NAN, 0.),
            HitTarget::Background,
        )));
        assert_eq!(lab.editor.scene(), &before);
        assert!(!lab.host.has_capture());
        lab.host.set_editable(false, &mut lab.editor);
        lab.down(control);
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.undo());
    }
}

#[test]
fn guards_exclude_multiselection_locked_singular_group_paths_and_unsupported_skew_text() {
    for control in controls() {
        for invalid in 0..3 {
            let mut lab = Lab::new(control, false);
            match invalid {
                0 => lab.editor.selection.select("parent".into(), true),
                1 => {
                    lab.editor
                        .engine
                        .transaction(|s| s.update_object(&"parent".into(), |o| o.locked = true))
                        .unwrap();
                }
                _ => {
                    lab.editor
                        .engine
                        .transaction(|s| {
                            s.update_object(&"target".into(), |o| {
                                o.transform = Affine::scale(0., 1.)
                            })
                        })
                        .unwrap();
                }
            }
            assert!(
                SceneGeometryTools::for_selection(
                    &lab.editor,
                    control.mode(),
                    TransformCapabilities::default()
                )
                .is_none()
            );
        }
    }
    let mut lab = Lab::new(GeometryControl::Skew(SkewAxis::X), false);
    lab.editor.selection.select("parent".into(), false);
    assert!(
        SceneGeometryTools::for_selection(
            &lab.editor,
            GeometryToolMode::Path,
            TransformCapabilities::default()
        )
        .is_none()
    );
    lab.editor.selection.select("target".into(), false);
    lab.editor
        .engine
        .transaction(|s| {
            s.update_object(&"target".into(), |o| {
                o.shape = d::Text::new("native", Size::new(80., 30.)).into()
            })
        })
        .unwrap();
    assert!(
        SceneGeometryTools::for_selection(
            &lab.editor,
            GeometryToolMode::Skew,
            TransformCapabilities::default()
        )
        .is_none()
    );
    assert!(
        !ScenePluginHost::with_defaults()
            .plugin_ids()
            .contains(&GEOMETRY_TOOLS_PLUGIN)
    );
}

#[test]
fn pivot_on_top_stem_keeps_rotation_handle_finite_and_default_stem_direction() {
    let mut lab = Lab::new(GeometryControl::Pivot, false);
    let bounds = lab
        .editor
        .engine
        .local_bounds(&"target".into())
        .unwrap()
        .unwrap();
    let pivot = gpui_flow_editor::ResizeHandle::Top.position(bounds);
    let target = lab.editor.engine.handle(&"target".into()).unwrap();
    lab.host
        .execute(
            &mut lab.editor,
            SceneCommand::SetGeometry {
                target,
                edit: GeometryEdit::Pivot {
                    position: Some(pivot),
                },
            },
        )
        .unwrap();
    let rotation = SceneRotationGeometry::for_selection(&lab.editor).unwrap();
    close(rotation.pivot, rotation.stem);
    let handle = rotation.screen_handle(&lab.editor).unwrap();
    assert!(handle.position.is_finite());
    assert!((handle.position.distance(handle.stem) - ROTATION_HANDLE_OFFSET).abs() < 0.001);
}

#[test]
fn shift_constrains_local_points_and_oversize_shear_preserves_last_accepted_sample() {
    for control in [GeometryControl::PathPoint(CONTROL), GeometryControl::Pivot] {
        let mut lab = Lab::new(control, false);
        let geometry = lab.geometry();
        let start = lab.down(control);
        let mut pointer =
            PointerInput::left(lab.delta(start, Point::new(5., 20.)), HitTarget::Background);
        pointer.modifiers.shift = true;
        lab.dispatch(CanvasEvent::PointerMove(pointer));
        let original = if control == GeometryControl::Pivot {
            geometry.pivot
        } else {
            Point::new(30., 80.)
        };
        let actual = if control == GeometryControl::Pivot {
            lab.object().pivot.unwrap()
        } else {
            PathControlGeometry::new(&lab.object().shape, Affine::IDENTITY)
                .unwrap()
                .point(CONTROL)
                .unwrap()
        };
        close(actual, original + Point::new(0., 20.));
    }
    let mut lab = Lab::new(GeometryControl::Skew(SkewAxis::X), false);
    let geometry = lab.geometry();
    let start = lab.down(GeometryControl::Skew(SkewAxis::X));
    lab.moved(
        start + geometry.world.transform_vector(Point::new(18., 0.)) * lab.editor.viewport.zoom,
    );
    let accepted = lab.editor.scene().clone();
    let revision = lab.editor.engine.revision();
    lab.moved(
        start
            + geometry.world.transform_vector(Point::new(999_999., 0.)) * lab.editor.viewport.zoom,
    );
    assert_eq!(lab.editor.scene(), &accepted);
    assert_eq!(lab.editor.engine.revision(), revision);
    assert!(lab.host.has_capture());
    lab.up(start);
    assert!(lab.editor.undo());
    assert!(!lab.editor.undo());
}

struct MoveChildOnParent {
    original: Affine,
}
impl LayoutProvider for MoveChildOnParent {
    fn derive(&self, _: &SceneEngine, scene: &Scene) -> Result<Scene, EngineError> {
        let mut next = scene.clone();
        if scene.get(&"parent".into()).unwrap().object.transform != self.original {
            next.update_object(&"target".into(), |o| o.transform.tx = 444.)?;
        }
        Ok(next)
    }
}
#[test]
fn group_skew_preserves_child_local_geometry_and_rejects_layout_that_changes_it() {
    for sabotage in [false, true] {
        let mut lab = Lab::new(GeometryControl::Skew(SkewAxis::Y), false);
        lab.editor.selection.select("parent".into(), false);
        let geometry = lab.geometry();
        if sabotage {
            lab.editor
                .engine
                .install_layout_provider(MoveChildOnParent {
                    original: geometry.original,
                })
                .unwrap();
        }
        let before = lab.editor.scene().clone();
        let start = lab.down(GeometryControl::Skew(SkewAxis::Y));
        let end =
            start + geometry.world.transform_vector(Point::new(0., 20.)) * lab.editor.viewport.zoom;
        let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
            end,
            HitTarget::Background,
        )));
        if sabotage {
            assert!(!report.default.errors.is_empty());
            assert_eq!(lab.editor.scene(), &before);
        } else {
            assert!(report.default.errors.is_empty());
            lab.up(end);
            assert_eq!(lab.object(), &before.get(&"target".into()).unwrap().object);
            assert_ne!(
                lab.editor
                    .scene()
                    .world_transform(&"target".into())
                    .unwrap(),
                before.world_transform(&"target".into()).unwrap()
            );
            close(
                lab.editor
                    .scene()
                    .world_transform(&"parent".into())
                    .unwrap()
                    .transform_point(geometry.pivot),
                geometry.world.transform_point(geometry.pivot),
            );
            assert!(lab.editor.undo());
            assert_eq!(lab.editor.scene(), &before);
        }
        assert!(!lab.editor.undo());
    }
}
