use gpui_flow_core::{Point, Size};
use gpui_flow_editor::{ResizeHandle, SceneEditor};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{Affine, DisplayObject, Scene, Shape, display as d};

struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
}
impl Lab {
    fn new(shape: impl Into<Shape>, transform: Affine) -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("target", shape).with_transform(transform),
                None,
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        editor.selection.select("target".into(), false);
        Self {
            editor,
            host: ScenePluginHost::with_defaults(),
            router: SceneEventRouter::new(),
        }
    }
    fn rect() -> Self {
        Self::new(d::Rect::new(120., 60.), Affine::translation(100., 80.))
    }
    fn geometry(&self) -> SceneResizeGeometry {
        SceneResizeGeometry::for_target(&self.editor, &"target".into()).unwrap()
    }
    fn transform(&self) -> Affine {
        self.editor
            .scene()
            .get(&"target".into())
            .unwrap()
            .object
            .transform
    }
    fn dispatch(&mut self, event: CanvasEvent) {
        let report = self
            .router
            .dispatch(&mut self.editor, &mut self.host, event);
        assert!(report.errors.is_empty(), "{:?}", report.errors);
        assert!(
            report.default.errors.is_empty(),
            "{:?}",
            report.default.errors
        );
    }
    fn down(&mut self, handle: ResizeHandle, shift: bool) -> Point {
        let geometry = self.geometry();
        let start = self.editor.viewport.world_to_screen(
            geometry
                .world
                .transform_point(handle.position(geometry.bounds)),
        );
        let mut input = PointerInput::left(
            start,
            HitTarget::ObjectResizeHandle {
                id: "target".into(),
                handle,
            },
        );
        input.modifiers.shift = shift;
        self.dispatch(CanvasEvent::PointerDown(input));
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
}
fn close(a: Point, b: Point) {
    assert!(a.distance(b) < 0.004, "{a:?} != {b:?}");
}

#[test]
fn every_handle_drag_uses_shared_geometry_and_one_undo_redo() {
    for handle in ResizeHandle::ALL {
        let mut lab = Lab::rect();
        let g = lab.geometry();
        let start = lab.down(handle, false);
        assert_eq!(lab.host.capture_owner(), Some("resize"));
        let delta = g
            .world
            .transform_vector((handle.position(g.bounds) - handle.anchor(g.bounds)) * 0.25);
        lab.movement(start + delta * 0.5, false);
        lab.movement(start + delta, false);
        let next = lab.transform();
        close(
            next.transform_point(handle.anchor(g.bounds)),
            g.original.transform_point(handle.anchor(g.bounds)),
        );
        lab.up(start + delta);
        assert!(!lab.host.has_capture());
        assert!(lab.editor.undo());
        assert_eq!(lab.transform(), g.original);
        assert!(!lab.editor.undo());
        assert!(lab.editor.redo());
        assert_eq!(lab.transform(), next);
    }
}

#[test]
fn shift_handle_does_not_toggle_selection_and_uniform_side_keeps_midpoint() {
    let mut lab = Lab::rect();
    let g = lab.geometry();
    let start = lab.down(ResizeHandle::Left, true);
    assert_eq!(lab.editor.selection.ids.len(), 1);
    lab.movement(start + Point::new(60., 0.), true);
    let next = lab.transform();
    assert_eq!((next.a, next.d), (0.5, 0.5));
    close(
        next.transform_point(ResizeHandle::Left.anchor(g.bounds)),
        g.original
            .transform_point(ResizeHandle::Left.anchor(g.bounds)),
    );
    close(
        next.transform_point(g.bounds.center()),
        g.original.transform_point(g.bounds.center()) + Point::new(30., 0.),
    );
}

#[test]
fn rotated_parent_nonuniform_axes_and_zoom_do_not_change_world_floor() {
    let mut lab = Lab::new(d::Rect::new(4., 6.), Affine::rotation(31.));
    lab.editor
        .transaction(|scene| {
            scene.insert(
                DisplayObject::new("parent", d::Group::new()).with_transform(
                    Affine::translation(90., 120.) * Affine::rotation(22.) * Affine::scale(0.5, 2.),
                ),
                None,
                None,
            )?;
            scene.reparent(&"target".into(), Some("parent".into()), None, false)
        })
        .unwrap();
    let g = lab.geometry();
    let expected_x = 1. / ((g.world.a as f64).hypot(g.world.b as f64) * 4.);
    let expected_y = 1. / ((g.world.c as f64).hypot(g.world.d as f64) * 6.);
    assert!((g.minimum_scale.x as f64 - expected_x).abs() < 0.00001);
    assert!((g.minimum_scale.y as f64 - expected_y).abs() < 0.00001);
    lab.editor.viewport.zoom = 2.;
    lab.editor.viewport.pan = Point::new(45., -12.);
    assert_eq!(lab.geometry().minimum_scale, g.minimum_scale);
    let start = lab.down(ResizeHandle::TopLeft, false);
    let delta = g.world.transform_vector(Point::new(100., 100.)) * lab.editor.viewport.zoom;
    lab.movement(start + delta, false);
    let world = lab.editor.engine.world_transform(&"target".into()).unwrap();
    assert!(((world.a as f64).hypot(world.b as f64) * 4. - 1.).abs() < 0.0001);
    assert!(((world.c as f64).hypot(world.d as f64) * 6. - 1.).abs() < 0.0001);
    close(
        world.transform_point(ResizeHandle::TopLeft.anchor(g.bounds)),
        g.world
            .transform_point(ResizeHandle::TopLeft.anchor(g.bounds)),
    );
}

#[test]
fn tiny_existing_target_never_jumps_or_shrinks_and_can_grow() {
    let mut lab = Lab::new(d::Rect::new(0.5, 0.25), Affine::IDENTITY);
    let start = lab.down(ResizeHandle::BottomRight, false);
    lab.movement(start, false);
    assert_eq!(lab.transform(), Affine::IDENTITY);
    lab.movement(Point::new(-30., -30.), false);
    assert_eq!(lab.transform(), Affine::IDENTITY);
    lab.movement(start + Point::new(0.5, 0.25), false);
    assert_eq!(lab.transform(), Affine::scale(2., 2.));
}

#[test]
fn native_text_and_image_corner_shrink_and_side_resize_are_uniform() {
    for shape in [
        Shape::from(d::Text::new("hello", Size::new(100., 50.))),
        d::Image::new("asset://image", Size::new(100., 50.)).into(),
    ] {
        for handle in [ResizeHandle::BottomRight, ResizeHandle::Left] {
            let mut lab = Lab::new(shape.clone(), Affine::translation(50., 80.));
            let g = lab.geometry();
            assert!(g.uniform);
            let start = lab.down(handle, false);
            let delta = if handle == ResizeHandle::Left {
                Point::new(50., 0.)
            } else {
                Point::new(-50., 0.)
            };
            lab.movement(start + delta, false);
            let next = lab.transform();
            assert_eq!(next.a, next.d);
            assert!(
                (next.a
                    - if handle == ResizeHandle::Left {
                        0.5
                    } else {
                        0.6
                    })
                .abs()
                    < 0.0001
            );
            close(
                next.transform_point(handle.anchor(g.bounds)),
                g.original.transform_point(handle.anchor(g.bounds)),
            );
        }
    }
}

#[test]
fn native_descendants_force_uniform_and_unsupported_native_matrices_hide_handles() {
    let mut lab = Lab::new(d::Group::new(), Affine::translation(80., 40.));
    lab.editor
        .transaction(|scene| {
            scene.insert(
                DisplayObject::new("text", d::Text::new("native", Size::new(100., 50.))),
                Some("target".into()),
                None,
            )
        })
        .unwrap();
    assert!(lab.geometry().uniform);
    let start = lab.down(ResizeHandle::Right, false);
    lab.movement(start + Point::new(50., 0.), false);
    assert_eq!((lab.transform().a, lab.transform().d), (1.5, 1.5));
    lab.up(start);
    lab.editor
        .transaction(|scene| {
            scene.update_object(&"target".into(), |o| o.transform = Affine::scale(2., 1.))
        })
        .unwrap();
    assert!(SceneResizeGeometry::for_target(&lab.editor, &"target".into()).is_none());
    lab.editor
        .transaction(|scene| {
            scene.update_object(&"target".into(), |o| o.transform = Affine::rotation(20.))
        })
        .unwrap();
    assert!(SceneResizeGeometry::for_target(&lab.editor, &"target".into()).is_none());
}

#[test]
fn no_motion_preserves_redo_and_cancel_variants_restore_exact_baseline() {
    let events = [
        CanvasEvent::PointerCancel,
        CanvasEvent::FocusLost,
        CanvasEvent::KeyDown {
            key: KeyCode::Escape,
            modifiers: Modifiers::default(),
        },
    ];
    for event in events {
        let mut lab = Lab::rect();
        let before = lab.editor.scene().clone();
        let start = lab.down(ResizeHandle::Top, false);
        lab.movement(start + Point::new(0., -40.), false);
        lab.dispatch(event);
        assert_eq!(*lab.editor.scene(), before);
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.undo());
    }
    let mut lab = Lab::rect();
    let start = lab.down(ResizeHandle::Right, false);
    lab.movement(start + Point::new(30., 0.), false);
    lab.up(start);
    assert!(lab.editor.undo());
    let start = lab.down(ResizeHandle::Right, false);
    lab.movement(start, false);
    lab.up(start);
    assert!(lab.editor.redo());
}

#[test]
fn policy_disable_and_plugin_removal_rollback_and_readonly_prevents_capture() {
    for mode in 0..3 {
        let mut lab = Lab::rect();
        let before = lab.editor.scene().clone();
        let start = lab.down(ResizeHandle::Bottom, false);
        lab.movement(start + Point::new(0., 30.), false);
        match mode {
            0 => {
                lab.host.set_editable(false, &mut lab.editor);
            }
            1 => {
                lab.host
                    .set_enabled("resize", false, &mut lab.editor)
                    .unwrap();
            }
            _ => {
                lab.host.uninstall("resize", &mut lab.editor).unwrap();
            }
        }
        assert_eq!(*lab.editor.scene(), before);
        assert!(!lab.host.has_capture());
        lab.down(ResizeHandle::Right, false);
        assert!(!lab.host.has_capture());
    }
}

#[test]
fn overlap_picking_is_nearest_and_directional_cursors_follow_rotation() {
    let mut lab = Lab::new(d::Rect::new(6., 4.), Affine::IDENTITY);
    let handles = lab.geometry().screen_handles(&lab.editor).unwrap();
    for expected in handles {
        assert_eq!(
            hit_resize_handle(&handles, expected.position)
                .unwrap()
                .handle,
            expected.handle
        );
    }
    assert!(hit_resize_handle(&handles, Point::new(100., 100.)).is_none());
    assert!(hit_resize_handle(&handles, Point::new(f32::NAN, 0.)).is_none());
    assert_eq!(handles[3].cursor, CanvasCursor::ResizeHorizontal);
    assert_eq!(handles[1].cursor, CanvasCursor::ResizeVertical);
    lab.editor
        .transaction(|scene| {
            scene.update_object(&"target".into(), |o| o.transform = Affine::rotation(90.))
        })
        .unwrap();
    assert_eq!(
        lab.geometry().screen_handles(&lab.editor).unwrap()[3].cursor,
        CanvasCursor::ResizeVertical
    );
}

#[test]
fn stale_same_id_replacement_cannot_be_mutated_or_resurrected_by_cancel() {
    for cancel in [false, true] {
        let mut lab = Lab::rect();
        let old = lab.editor.engine.handle(&"target".into()).unwrap();
        let start = lab.down(ResizeHandle::Right, false);
        lab.movement(start + Point::new(20., 0.), false);
        lab.editor
            .transaction(|scene| scene.remove(&"target".into()).map(|_| ()))
            .unwrap();
        lab.editor
            .transaction(|scene| {
                scene.insert(
                    DisplayObject::new("target", d::Rect::new(31., 27.))
                        .with_transform(Affine::translation(300., 200.)),
                    None,
                    None,
                )
            })
            .unwrap();
        assert_ne!(lab.editor.engine.handle(&"target".into()).unwrap(), old);
        let replacement = lab.editor.scene().clone();
        if cancel {
            lab.host.cancel(&mut lab.editor);
        } else {
            lab.movement(start + Point::new(80., 0.), false);
        }
        assert_eq!(*lab.editor.scene(), replacement);
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.editing());
    }
}

#[test]
fn document_reset_and_detach_reattach_invalidate_capture() {
    for reset in [false, true] {
        let mut lab = Lab::rect();
        let target = lab.editor.engine.handle(&"target".into()).unwrap();
        let start = lab.down(ResizeHandle::Right, false);
        lab.movement(start + Point::new(20., 0.), false);
        if reset {
            let current = lab.editor.scene().clone();
            lab.editor.engine.reset_scene(current).unwrap();
        } else {
            let (detached, _) = lab.editor.engine.detach(target).unwrap();
            lab.editor.engine.attach(&detached, None, None).unwrap();
        }
        let current = lab.editor.scene().clone();
        lab.movement(start + Point::new(80., 0.), false);
        assert_eq!(*lab.editor.scene(), current);
        assert!(!lab.host.has_capture());
    }
}

#[test]
fn inner_text_ownership_and_outer_registry_availability_stay_exclusive() {
    let mut lab = Lab::new(d::Text::new("keep", Size::new(120., 60.)), Affine::IDENTITY);
    let mut tools = SceneToolController::with_text();
    assert!(tools.outer_tool_active(DEFAULT_OUTER_TOOL));
    let target = lab.editor.engine.handle(&"target".into()).unwrap();
    let token = tools
        .open_text(&mut lab.editor, &mut lab.host, target)
        .unwrap();
    assert!(!tools.outer_tool_active(DEFAULT_OUTER_TOOL));
    lab.down(ResizeHandle::Right, false);
    assert!(!lab.host.has_capture());
    assert_eq!(tools.active_token(), Some(token));
    assert!(tools.cancel(&mut lab.editor, &mut lab.host, token).unwrap());
    assert!(tools.outer_tool_active(DEFAULT_OUTER_TOOL));
    tools.register_outer("inspect").unwrap();
    tools
        .select_outer(&mut lab.editor, &mut lab.host, "inspect")
        .unwrap();
    assert!(!tools.outer_tool_active(DEFAULT_OUTER_TOOL));
    assert!(tools.outer_tool_active("inspect"));
    tools
        .set_enabled(&mut lab.editor, &mut lab.host, false)
        .unwrap();
    assert!(!tools.outer_tool_active("inspect"));
}

#[test]
fn multiselect_singular_custom_and_locked_targets_are_not_resizeable() {
    let mut lab = Lab::rect();
    lab.editor.selection.ids.insert("other".into());
    lab.down(ResizeHandle::Right, false);
    assert!(!lab.host.has_capture());
    lab.editor.selection.ids.remove(&"other".into());
    lab.editor
        .transaction(|scene| scene.update_object(&"target".into(), |o| o.locked = true))
        .unwrap();
    assert!(SceneResizeGeometry::for_target(&lab.editor, &"target".into()).is_none());
    lab.editor
        .transaction(|scene| {
            scene.update_object(&"target".into(), |o| {
                o.locked = false;
                o.transform = Affine::scale(0., 1.);
            })
        })
        .unwrap();
    assert!(SceneResizeGeometry::for_target(&lab.editor, &"target".into()).is_none());
    let custom = Lab::new(
        d::Custom::new(
            "unsupported",
            gpui_flow_core::Rect::new(Point::ZERO, Size::new(100., 60.)),
            vec![],
        ),
        Affine::IDENTITY,
    );
    assert!(SceneResizeGeometry::for_target(&custom.editor, &"target".into()).is_none());
}

#[test]
fn capture_ignores_foreign_pointer_and_missing_button_cancels_then_restarts() {
    let mut lab = Lab::rect();
    let original = lab.transform();
    let start = lab.down(ResizeHandle::Right, false);
    let mut other = PointerInput::left(start + Point::new(40., 0.), HitTarget::Background);
    other.pointer_id = 42;
    lab.dispatch(CanvasEvent::PointerMove(other));
    assert_eq!(lab.transform(), original);
    lab.movement(start + Point::new(40., 0.), false);
    let mut lost = PointerInput::left(start, HitTarget::Background);
    lost.button = None;
    lab.dispatch(CanvasEvent::PointerMove(lost));
    assert_eq!(lab.transform(), original);
    assert!(!lab.host.has_capture());
    let start = lab.down(ResizeHandle::Bottom, false);
    lab.movement(start + Point::new(0., 30.), false);
    lab.up(start);
    assert_eq!(lab.transform().d, 1.5);
}

#[test]
fn space_pan_on_a_named_handle_keeps_transform_and_selection() {
    let mut lab = Lab::rect();
    let original = lab.transform();
    let selection = lab.editor.selection.clone();
    let start = lab.geometry().screen_handles(&lab.editor).unwrap()[3].position;
    let mut input = PointerInput::left(
        start,
        HitTarget::ObjectResizeHandle {
            id: "target".into(),
            handle: ResizeHandle::Right,
        },
    );
    input.modifiers.space = true;
    lab.dispatch(CanvasEvent::PointerDown(input));
    assert_eq!(lab.host.capture_owner(), Some("pan"));
    lab.movement(start + Point::new(30., 20.), false);
    lab.up(start);
    assert_eq!(lab.transform(), original);
    assert_eq!(lab.editor.selection, selection);
    assert_eq!(lab.editor.viewport.pan, Point::new(30., 20.));
}

#[test]
fn native_handle_double_click_remains_a_resize_target_not_text_activation() {
    // The adapter's click-count regression covers actual activation filtering;
    // the pure typed target is routed through the same instance event path.
    let mut lab = Lab::new(
        d::Text::new("content", Size::new(120., 60.)),
        Affine::IDENTITY,
    );
    let start = lab.down(ResizeHandle::Right, false);
    lab.up(start);
    let start = lab.down(ResizeHandle::Right, false);
    lab.movement(start + Point::new(-30., 0.), false);
    lab.up(start);
    assert_eq!((lab.transform().a, lab.transform().d), (0.75, 0.75));
    assert!(lab.editor.undo());
    assert_eq!(lab.transform(), Affine::IDENTITY);
    assert!(!lab.editor.undo());
}

#[test]
fn outer_switch_disable_and_dispose_cancel_active_resize() {
    for mode in 0..3 {
        let mut lab = Lab::rect();
        let mut tools = SceneToolController::new();
        tools.register_outer("inspect").unwrap();
        let original = lab.transform();
        let start = lab.down(ResizeHandle::TopLeft, false);
        lab.movement(start + Point::new(-50., -20.), false);
        match mode {
            0 => {
                tools
                    .select_outer(&mut lab.editor, &mut lab.host, "inspect")
                    .unwrap();
            }
            1 => {
                tools
                    .set_enabled(&mut lab.editor, &mut lab.host, false)
                    .unwrap();
            }
            _ => {
                tools.dispose(&mut lab.editor, &mut lab.host).unwrap();
            }
        }
        assert_eq!(lab.transform(), original);
        assert!(!lab.host.has_capture());
        assert!(!tools.outer_tool_active(DEFAULT_OUTER_TOOL));
    }
}

struct DistortResize(bool);
impl CanvasPlugin<SceneEditor, SceneCommand> for DistortResize {
    fn id(&self) -> &str {
        "distort-resize"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetResizeTransform { transform, .. } = command {
            if self.0 {
                *command = SceneCommand::SetTransform {
                    id: "target".into(),
                    transform: *transform,
                };
            } else {
                transform.a *= 2.;
            }
        }
        Ok(())
    }
}
#[test]
fn middleware_cannot_downgrade_typed_identity_or_break_native_uniformness() {
    for downgrade in [false, true] {
        let mut lab = Lab::new(
            d::Text::new("native", Size::new(120., 60.)),
            Affine::IDENTITY,
        );
        lab.host.install(DistortResize(downgrade)).unwrap();
        let start = lab.down(ResizeHandle::Right, false);
        let report = lab.router.dispatch(
            &mut lab.editor,
            &mut lab.host,
            CanvasEvent::PointerMove(PointerInput::left(
                start + Point::new(30., 0.),
                HitTarget::Background,
            )),
        );
        assert!(!report.default.errors.is_empty());
        assert_eq!(lab.transform(), Affine::IDENTITY);
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.undo());
    }
}

#[test]
fn typed_handles_obey_prevented_down_and_up_and_inherited_locks() {
    use gpui_flow_scene::{EventKind, ListenerOptions};
    for kind in [EventKind::PointerDown, EventKind::PointerUp] {
        let mut lab = Lab::rect();
        let target = lab.editor.engine.handle(&"target".into()).unwrap();
        lab.editor
            .engine
            .on(target, kind.clone(), ListenerOptions::default(), |event| {
                event.prevent_default()
            })
            .unwrap();
        let original = lab.transform();
        let start = lab.down(ResizeHandle::BottomRight, false);
        if kind == EventKind::PointerUp {
            lab.movement(start + Point::new(30., 15.), false);
        }
        lab.up(start);
        assert_eq!(lab.transform(), original);
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.undo());
    }
    let mut lab = Lab::rect();
    lab.editor
        .transaction(|scene| {
            let mut parent = DisplayObject::new("parent", d::Group::new());
            parent.locked = true;
            scene.insert(parent, None, None)?;
            scene.reparent(&"target".into(), Some("parent".into()), None, false)
        })
        .unwrap();
    assert!(SceneResizeGeometry::for_target(&lab.editor, &"target".into()).is_none());
}
