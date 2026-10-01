use gpui_flow_core::{Point, Size};
use gpui_flow_editor::{ResizeHandle, SceneEditor};
use gpui_flow_engine::{EngineError, LayoutProvider, SceneEngine};
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
    fn geometry(&self) -> SceneSelectionResizeGeometry {
        SceneSelectionResizeGeometry::for_selection(&self.editor).unwrap()
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
    fn down(&mut self, handle: ResizeHandle, shift: bool) -> Point {
        let start = self
            .editor
            .viewport
            .world_to_screen(handle.position(self.geometry().geometry.bounds));
        self.press(start, handle, shift, false);
        start
    }
    fn press(&mut self, position: Point, handle: ResizeHandle, shift: bool, space: bool) {
        let mut input = PointerInput::left(position, HitTarget::SelectionResizeHandle { handle });
        input.modifiers.shift = shift;
        input.modifiers.space = space;
        self.dispatch(CanvasEvent::PointerDown(input));
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
fn close(a: Point, b: Point) {
    assert!(a.distance(b) < 0.006, "{a:?} != {b:?}");
}
fn matrix_close(a: Affine, b: Affine) {
    for point in [Point::ZERO, Point::new(37., 0.), Point::new(0., 21.)] {
        close(a.transform_point(point), b.transform_point(point));
    }
}
#[test]
fn all_eight_handles_share_world_delta_spacing_anchor_and_one_undo() {
    for handle in ResizeHandle::ALL {
        let mut lab = Lab::pair();
        let before = lab.editor.scene().clone();
        let bounds = lab.geometry().geometry.bounds;
        let old = [lab.world("a"), lab.world("b")];
        let start = lab.down(handle, false);
        assert_eq!(lab.host.capture_owner(), Some("resize"));
        let delta = (handle.position(bounds) - handle.anchor(bounds)) * 0.25;
        lab.movement(start + delta * 0.5, false);
        lab.movement(start + delta, false);
        let x = if matches!(handle, ResizeHandle::Top | ResizeHandle::Bottom) {
            1.
        } else {
            1.25
        };
        let y = if matches!(handle, ResizeHandle::Left | ResizeHandle::Right) {
            1.
        } else {
            1.25
        };
        let d = Affine::scale(x, y).around(handle.anchor(bounds));
        close(
            d.transform_point(handle.anchor(bounds)),
            handle.anchor(bounds),
        );
        matrix_close(lab.world("a"), d * old[0]);
        matrix_close(lab.world("b"), d * old[1]);
        close(
            lab.world("b").transform_point(Point::ZERO)
                - lab.world("a").transform_point(Point::ZERO),
            d.transform_vector(
                old[1].transform_point(Point::ZERO) - old[0].transform_point(Point::ZERO),
            ),
        );
        let after = lab.editor.scene().clone();
        lab.up(start + delta);
        assert!(lab.editor.undo());
        assert_eq!(*lab.editor.scene(), before);
        assert!(!lab.editor.undo());
        assert!(lab.editor.redo());
        assert_eq!(*lab.editor.scene(), after);
    }
}
#[test]
fn transformed_different_parents_reflections_and_zoom_use_world_axes() {
    let mut scene = Scene::new();
    add(
        &mut scene,
        "pa",
        d::Group::new(),
        Affine::translation(100., 50.) * Affine::rotation(31.) * Affine::scale(2., 0.5),
        None,
    );
    add(
        &mut scene,
        "pb",
        d::Group::new(),
        Affine::translation(350., 130.) * Affine::skew(20., -8.),
        None,
    );
    add(
        &mut scene,
        "a",
        d::Rect::new(80., 40.),
        Affine::rotation(18.) * Affine::scale(-1., 1.),
        Some("pa"),
    );
    add(
        &mut scene,
        "b",
        d::Rect::new(60., 30.),
        Affine::translation(12., 16.),
        Some("pb"),
    );
    let mut lab = Lab::new(scene, &["a", "b"]);
    lab.editor.viewport.zoom = 2.5;
    lab.editor.viewport.pan = Point::new(71., -43.);
    let bounds = lab.geometry().geometry.bounds;
    let old = [lab.world("a"), lab.world("b")];
    let start = lab.down(ResizeHandle::Right, false);
    lab.movement(start + Point::new(bounds.size.width * 0.3 * 2.5, 0.), false);
    let d = Affine::scale(1.3, 1.).around(ResizeHandle::Right.anchor(bounds));
    matrix_close(lab.world("a"), d * old[0]);
    matrix_close(lab.world("b"), d * old[1]);
    assert!(lab.world("a").determinant() < 0.);
}
#[test]
fn selected_parent_and_child_are_one_root_and_child_local_stays_unchanged() {
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
    let mut lab = Lab::new(scene, &["a", "child", "b"]);
    assert_eq!(lab.geometry().roots.len(), 2);
    let child = lab
        .editor
        .scene()
        .get(&"child".into())
        .unwrap()
        .object
        .transform;
    let old = lab.world("child");
    let bounds = lab.geometry().geometry.bounds;
    let start = lab.down(ResizeHandle::BottomRight, false);
    lab.movement(
        start + Point::new(bounds.size.width, bounds.size.height),
        false,
    );
    assert_eq!(
        lab.editor
            .scene()
            .get(&"child".into())
            .unwrap()
            .object
            .transform,
        child
    );
    matrix_close(
        lab.world("child"),
        Affine::scale(2., 2.).around(bounds.origin) * old,
    );
    let mut only = Lab::new(lab.editor.scene().clone(), &["a", "child"]);
    assert_eq!(only.geometry().roots.len(), 1);
    let start = only.down(ResizeHandle::Right, false);
    only.movement(start + Point::new(20., 0.), false);
    only.up(start);
}
#[test]
fn shift_and_native_descendants_uniformly_scale_the_complete_selection() {
    for native in 0..3 {
        let mut lab = Lab::pair();
        if native > 0 {
            lab.editor
                .transaction(|scene| {
                    scene.update_object(&"b".into(), |o| o.shape = d::Group::new().into())?;
                    let shape: Shape = if native == 1 {
                        d::Text::new("hello", Size::new(60., 30.)).into()
                    } else {
                        d::Image::new("fixture", Size::new(60., 30.)).into()
                    };
                    scene.insert(DisplayObject::new("native", shape), Some("b".into()), None)
                })
                .unwrap();
        }
        let g = lab.geometry();
        assert_eq!(g.geometry.uniform, native > 0);
        let bounds = g.geometry.bounds;
        let old = [lab.world("a"), lab.world("b")];
        let selection = lab.editor.selection.clone();
        let start = lab.down(ResizeHandle::Left, native == 0);
        lab.movement(start + Point::new(bounds.size.width * 0.5, 0.), native == 0);
        assert_eq!(lab.editor.selection, selection);
        let d = Affine::scale(0.5, 0.5).around(ResizeHandle::Left.anchor(bounds));
        matrix_close(lab.world("a"), d * old[0]);
        matrix_close(lab.world("b"), d * old[1]);
    }
}
#[test]
fn off_center_zero_motion_and_return_to_start_preserve_redo_exactly() {
    let mut lab = Lab::pair();
    let start = lab.down(ResizeHandle::BottomRight, false);
    lab.movement(start + Point::new(40., 20.), false);
    lab.up(start);
    let after = lab.editor.scene().clone();
    assert!(lab.editor.undo());
    let before = lab.editor.scene().clone();
    let start = lab
        .editor
        .viewport
        .world_to_screen(ResizeHandle::Right.position(lab.geometry().geometry.bounds))
        + Point::new(3., -2.);
    lab.press(start, ResizeHandle::Right, false, false);
    lab.movement(start, false);
    assert_eq!(*lab.editor.scene(), before);
    lab.movement(start + Point::new(20., 0.), false);
    lab.movement(start, false);
    lab.up(start);
    assert_eq!(*lab.editor.scene(), before);
    assert!(lab.editor.redo());
    assert_eq!(*lab.editor.scene(), after);
}
#[test]
fn tiny_union_floor_does_not_jump_or_shrink_and_handles_remain_screen_sized() {
    let mut scene = Scene::new();
    add(
        &mut scene,
        "a",
        d::Rect::new(0.2, 0.2),
        Affine::IDENTITY,
        None,
    );
    add(
        &mut scene,
        "b",
        d::Rect::new(0.2, 0.2),
        Affine::translation(0.3, 0.3),
        None,
    );
    let mut lab = Lab::new(scene, &["a", "b"]);
    let before = lab.editor.scene().clone();
    let g = lab.geometry();
    assert_eq!(g.geometry.minimum_scale, Point::new(1., 1.));
    let start = lab.down(ResizeHandle::BottomRight, false);
    lab.movement(start, false);
    lab.movement(start - Point::new(10., 10.), false);
    assert_eq!(*lab.editor.scene(), before);
    lab.movement(start + Point::new(0.5, 0.5), false);
    assert_eq!(lab.world("a").a, 2.);
    lab.up(start);
    for zoom in [0.25, 1., 3.] {
        lab.editor.viewport.zoom = zoom;
        lab.editor.viewport.pan = Point::new(70., 40.);
        let handles = lab.geometry().screen_handles(&lab.editor).unwrap();
        assert_eq!(
            hit_resize_handle(&handles, handles[3].position)
                .unwrap()
                .handle,
            ResizeHandle::Right
        );
        assert!(hit_resize_handle(&handles, handles[3].position + Point::new(11., 0.)).is_none());
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
        if let SceneCommand::SetResizeTransforms { transforms } = command {
            match self.0 {
                0 => {
                    transforms.pop();
                }
                1 => transforms.push(transforms[0]),
                2 => transforms.swap(0, 1),
                3 => transforms[1].0 = transforms[0].0,
                4 => transforms[1].1 = Affine::scale(0., 1.),
                5 => transforms[1].1 = Affine::scale(2., 3.),
                6 => {
                    transforms[1].1.a = f32::MAX;
                    transforms[1].1.tx = f32::MAX;
                }
                7 => *command = SceneCommand::ClearSelection,
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
fn middleware_cannot_drop_add_reorder_retarget_replace_or_invalidate_second_member() {
    for mode in 0..8 {
        let mut lab = Lab::pair();
        if mode == 5 {
            lab.editor
                .transaction(|scene| {
                    scene.update_object(&"b".into(), |o| {
                        o.shape = d::Text::new("native", Size::new(60., 30.)).into()
                    })
                })
                .unwrap();
        }
        lab.host.install(ChangeBatch(mode)).unwrap();
        let before = lab.editor.scene().clone();
        let start = lab.down(ResizeHandle::Right, false);
        let result = lab.event(CanvasEvent::PointerMove(PointerInput::left(
            start + Point::new(20., 0.),
            HitTarget::Background,
        )));
        assert!(!result.default.errors.is_empty(), "mode {mode}");
        assert_eq!(*lab.editor.scene(), before, "mode {mode}");
        assert!(!lab.host.has_capture());
    }
}
#[test]
fn middleware_matrix_normalization_keeps_atomic_batch_and_one_revision() {
    let mut lab = Lab::pair();
    lab.host.install(ChangeBatch(8)).unwrap();
    let start = lab.down(ResizeHandle::Left, false);
    let revision = lab.editor.engine.revision();
    lab.movement(start + Point::new(13.33, 0.), false);
    assert_eq!(lab.editor.engine.revision(), revision + 1);
    assert!(lab.world("a").a < 1. && lab.world("b").a < 1.);
    lab.up(start);
}
struct InvalidDerived(u8);
impl LayoutProvider for InvalidDerived {
    fn derive(&self, _: &SceneEngine, scene: &Scene) -> Result<Scene, EngineError> {
        let mut next = scene.clone();
        if scene.get(&"a".into()).unwrap().object.transform.a != 1. {
            if self.0 == 0 {
                return Err(EngineError::Layout("second member failed".into()));
            }
            next.update_object(&"b".into(), |o| o.transform = Affine::scale(2., 3.))?;
        }
        Ok(next)
    }
}
#[test]
fn layout_failure_and_post_derivation_native_invalidity_never_partially_commit() {
    for mode in 0..2 {
        let mut lab = Lab::pair();
        lab.editor
            .transaction(|scene| {
                scene.update_object(&"b".into(), |o| {
                    o.shape = d::Text::new("native", Size::new(60., 30.)).into()
                })
            })
            .unwrap();
        lab.editor
            .engine
            .install_layout_provider(InvalidDerived(mode))
            .unwrap();
        let before = lab.editor.scene().clone();
        let revision = lab.editor.engine.revision();
        let start = lab.down(ResizeHandle::Right, false);
        let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
            start + Point::new(20., 0.),
            HitTarget::Background,
        )));
        assert!(!report.default.errors.is_empty());
        assert_eq!(*lab.editor.scene(), before);
        assert_eq!(lab.editor.engine.revision(), revision);
        assert!(!lab.host.has_capture());
    }
}
#[test]
fn second_locked_singular_custom_or_unsupported_native_hides_shared_handles() {
    for mode in 0..5 {
        let mut lab = Lab::pair();
        lab.editor
            .transaction(|scene| {
                scene.update_object(&"b".into(), |o| match mode {
                    0 => o.locked = true,
                    1 => o.transform = Affine::scale(0., 1.),
                    2 => {
                        o.shape = d::Custom::new(
                            "unsupported",
                            gpui_flow_core::Rect::new(Point::ZERO, Size::new(60., 30.)),
                            vec![],
                        )
                        .into()
                    }
                    3 => {
                        o.shape = d::Text::new("native", Size::new(60., 30.)).into();
                        o.transform = Affine::rotation(10.);
                    }
                    _ => {
                        o.shape = d::Image::new("fixture", Size::new(60., 30.)).into();
                        o.transform = Affine::scale(-1., 1.);
                    }
                })
            })
            .unwrap();
        assert!(SceneSelectionResizeGeometry::for_selection(&lab.editor).is_none());
        lab.press(Point::ZERO, ResizeHandle::Right, false, false);
        assert!(!lab.host.has_capture());
    }
}
#[test]
fn frozen_transform_overflow_rejects_the_whole_candidate() {
    let lab = Lab::pair();
    let g = lab.geometry();
    assert!(g.transforms(Affine::scale(f32::MAX, f32::MAX)).is_none());
}
#[test]
fn nonprimary_replacement_reparent_detach_and_document_changes_never_replay_stale_baseline() {
    for mode in 0..6 {
        let mut lab = Lab::pair();
        let start = lab.down(ResizeHandle::Right, false);
        lab.movement(start + Point::new(20., 0.), false);
        match mode {
            0 => {
                lab.editor
                    .transaction(|scene| scene.remove(&"b".into()).map(|_| ()))
                    .unwrap();
                lab.editor
                    .transaction(|scene| {
                        scene.insert(
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
                    .transaction(|scene| {
                        scene.insert(
                            DisplayObject::new("parent", d::Group::new())
                                .with_transform(Affine::translation(70., 80.)),
                            None,
                            None,
                        )?;
                        scene.reparent(&"b".into(), Some("parent".into()), None, false)
                    })
                    .unwrap();
            }
            2 => {
                let target = lab.editor.engine.handle(&"b".into()).unwrap();
                let (detached, _) = lab.editor.engine.detach(target).unwrap();
                lab.editor.engine.attach(&detached, None, None).unwrap();
            }
            3 => {
                let scene = lab.editor.scene().clone();
                lab.editor.engine.reset_scene(scene).unwrap();
            }
            4 => {
                lab.editor.engine = SceneEngine::new(lab.editor.scene().clone()).unwrap();
            }
            _ => {
                lab.editor.selection.select("a".into(), false);
            }
        }
        let external = lab.editor.scene().clone();
        let selection = lab.editor.selection.clone();
        lab.movement(start + Point::new(80., 0.), false);
        assert_eq!(*lab.editor.scene(), external, "mode {mode}");
        assert_eq!(lab.editor.selection, selection, "mode {mode}");
        assert!(!lab.host.has_capture());
        assert!(!lab.editor.editing());
    }
}
#[test]
fn same_selection_ancestor_or_unselected_native_descendant_change_invalidates_capture() {
    for mode in 0..4 {
        let mut scene = Scene::new();
        add(
            &mut scene,
            "parent",
            d::Group::new(),
            Affine::translation(40., 20.),
            None,
        );
        add(
            &mut scene,
            "a",
            d::Rect::new(80., 40.),
            Affine::IDENTITY,
            Some("parent"),
        );
        add(
            &mut scene,
            "b",
            d::Group::new(),
            Affine::translation(160., 100.),
            None,
        );
        add(
            &mut scene,
            "child",
            d::Rect::new(60., 30.),
            Affine::IDENTITY,
            Some("b"),
        );
        let mut lab = Lab::new(scene, &["a", "b"]);
        let start = lab.down(ResizeHandle::Right, false);
        lab.movement(start + Point::new(20., 0.), false);
        lab.editor
            .transaction(|scene| {
                if mode == 0 {
                    scene.update_object(&"parent".into(), |o| o.transform = Affine::rotation(11.))
                } else {
                    scene.update_object(&"child".into(), |o| {
                        if mode == 1 {
                            o.shape = d::Text::new("new", Size::new(60., 30.)).into();
                        } else if mode == 2 {
                            o.locked = true;
                        } else {
                            o.transform = Affine::scale(0., 1.);
                        }
                    })
                }
            })
            .unwrap();
        let external = lab.editor.scene().clone();
        // Explicit policy cancellation also takes the stale-safe path.
        lab.host.set_editable(false, &mut lab.editor);
        assert_eq!(*lab.editor.scene(), external);
        assert!(!lab.host.has_capture());
    }
}
#[test]
fn all_input_cancellation_paths_restore_owned_batch_and_allow_restart() {
    for mode in 0..8 {
        let mut lab = Lab::pair();
        let before = lab.editor.scene().clone();
        let start = lab.down(ResizeHandle::BottomRight, false);
        lab.movement(start + Point::new(40., 20.), false);
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
                    .set_enabled("resize", false, &mut lab.editor)
                    .unwrap();
                lab.host
                    .set_enabled("resize", true, &mut lab.editor)
                    .unwrap();
            }
            6 => {
                let mut tools = SceneToolController::new();
                tools.register_outer("inspect").unwrap();
                tools
                    .select_outer(&mut lab.editor, &mut lab.host, "inspect")
                    .unwrap();
            }
            _ => {
                lab.host.uninstall("resize", &mut lab.editor).unwrap();
                lab.host
                    .install(gpui_flow_interaction::scene::plugins::SceneResizePlugin::default())
                    .unwrap();
            }
        }
        assert_eq!(*lab.editor.scene(), before, "mode {mode}");
        assert!(!lab.host.has_capture());
        let start = lab.down(ResizeHandle::Right, false);
        lab.movement(start + Point::new(30., 0.), false);
        lab.up(start);
        assert!(lab.editor.undo());
        assert_eq!(*lab.editor.scene(), before);
    }
}
#[test]
fn foreign_pointer_and_pointer_leave_cannot_end_shared_capture() {
    let mut lab = Lab::pair();
    let before = lab.editor.scene().clone();
    let start = lab.down(ResizeHandle::Right, false);
    let mut other = PointerInput::left(start + Point::new(50., 0.), HitTarget::Background);
    other.pointer_id = 99;
    lab.dispatch(CanvasEvent::PointerMove(other.clone()));
    lab.dispatch(CanvasEvent::PointerUp(other));
    assert_eq!(*lab.editor.scene(), before);
    assert!(lab.host.has_capture());
    lab.dispatch(CanvasEvent::PointerLeave);
    lab.movement(start + Point::new(20., 0.), false);
    lab.up(start + Point::new(20., 0.));
    assert!(!lab.host.has_capture());
    assert_ne!(*lab.editor.scene(), before);
}
#[test]
fn space_pan_and_text_inner_session_preserve_selection_and_resize_exclusivity() {
    let mut lab = Lab::pair();
    let before = lab.editor.scene().clone();
    let selection = lab.editor.selection.clone();
    let start = lab.geometry().screen_handles(&lab.editor).unwrap()[3].position;
    lab.press(start, ResizeHandle::Right, false, true);
    assert_eq!(lab.host.capture_owner(), Some("pan"));
    lab.movement(start + Point::new(30., 20.), false);
    lab.up(start);
    assert_eq!(*lab.editor.scene(), before);
    assert_eq!(lab.editor.selection, selection);
    assert_eq!(lab.editor.viewport.pan, Point::new(30., 20.));
    lab.editor
        .transaction(|scene| {
            scene.update_object(&"b".into(), |o| {
                o.shape = d::Text::new("content", Size::new(60., 30.)).into()
            })
        })
        .unwrap();
    let mut tools = SceneToolController::with_text();
    let target = lab.editor.engine.handle(&"b".into()).unwrap();
    let token = tools
        .open_text(&mut lab.editor, &mut lab.host, target)
        .unwrap();
    lab.down(ResizeHandle::Right, false);
    assert!(!lab.host.has_capture());
    assert_eq!(tools.active_token(), Some(token));
    tools.cancel(&mut lab.editor, &mut lab.host, token).unwrap();
}
#[test]
fn shared_union_uses_geometric_subtree_bounds_and_excludes_shadows() {
    let mut lab = Lab::pair();
    let bounds = lab.geometry().geometry.bounds;
    lab.editor
        .transaction(|scene| {
            scene.update_object(&"b".into(), |o| {
                o.style.shadow = Some(gpui_flow_scene::Shadow::new(
                    100.,
                    70.,
                    30.,
                    gpui_flow_scene::Color::BLACK,
                ));
            })
        })
        .unwrap();
    assert_eq!(lab.geometry().geometry.bounds, bounds);
    assert!(
        lab.editor
            .engine
            .effect_bounds(&"b".into())
            .unwrap()
            .unwrap()
            .max_x()
            > bounds.max_x()
    );
}
struct RejectSecondMove(usize);
impl CanvasPlugin<SceneEditor, SceneCommand> for RejectSecondMove {
    fn id(&self) -> &str {
        "reject-second-move"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetResizeTransforms { transforms } = command {
            self.0 += 1;
            if self.0 == 2 {
                transforms[1].1 = Affine::scale(0., 1.);
            }
        }
        Ok(())
    }
}
#[test]
fn later_second_member_failure_restores_entire_owned_gesture_and_redo() {
    let mut lab = Lab::pair();
    let start = lab.down(ResizeHandle::Right, false);
    lab.movement(start + Point::new(10., 0.), false);
    lab.up(start);
    let redo = lab.editor.scene().clone();
    assert!(lab.editor.undo());
    let baseline = lab.editor.scene().clone();
    lab.host.install(RejectSecondMove(0)).unwrap();
    let start = lab.down(ResizeHandle::Right, false);
    lab.movement(start + Point::new(20., 0.), false);
    let report = lab.event(CanvasEvent::PointerMove(PointerInput::left(
        start + Point::new(40., 0.),
        HitTarget::Background,
    )));
    assert!(!report.default.errors.is_empty());
    assert_eq!(*lab.editor.scene(), baseline);
    assert!(!lab.host.has_capture());
    assert!(lab.editor.redo());
    assert_eq!(*lab.editor.scene(), redo);
}
