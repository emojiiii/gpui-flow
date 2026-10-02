use gpui_flow_editor::{ImageCropPart, ResizeHandle, SceneEditor};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{Affine, Bounds, DisplayObject, Point, Scene, Shape, Size, display};
fn crop(x: f32, y: f32, w: f32, h: f32) -> Bounds {
    Bounds::new(Point::new(x, y), Size::new(w, h))
}
struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
}
impl Lab {
    fn new(transform: Affine) -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("p", display::Group::new()).with_transform(transform),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new(
                    "i",
                    display::Image::new("pixels", Size::new(200., 100.)).rounded(10.),
                ),
                Some("p".into()),
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        editor.selection.select("i".into(), false);
        let mut host = ScenePluginHost::with_defaults();
        host.install(SceneImageCropPlugin::default()).unwrap();
        Self {
            editor,
            host,
            router: SceneEventRouter::new(),
        }
    }
    fn geometry(&self) -> SceneImageCropGeometry {
        SceneImageCropGeometry::for_selection(&self.editor).unwrap()
    }
    fn value(&self) -> Option<Bounds> {
        match &self.editor.scene().get(&"i".into()).unwrap().object.shape {
            Shape::Image(i) => i.source_crop,
            _ => panic!(),
        }
    }
    fn event(&mut self, event: CanvasEvent) -> RoutedReport {
        self.router
            .dispatch(&mut self.editor, &mut self.host, event)
    }
    fn dispatch(&mut self, event: CanvasEvent) {
        let r = self.event(event);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(r.default.errors.is_empty(), "{:?}", r.default.errors);
    }
    fn down(&mut self, part: ImageCropPart, point: Point) {
        self.dispatch(CanvasEvent::PointerDown(PointerInput::left(
            point,
            HitTarget::ImageCrop {
                id: "i".into(),
                part,
            },
        )));
    }
    fn moved(&mut self, point: Point) {
        self.dispatch(CanvasEvent::PointerMove(PointerInput::left(
            point,
            HitTarget::Background,
        )));
    }
    fn up(&mut self, point: Point) {
        self.dispatch(CanvasEvent::PointerUp(PointerInput::left(
            point,
            HitTarget::Background,
        )));
    }
    fn screen(&self, p: Point) -> Point {
        self.editor
            .viewport
            .world_to_screen(self.geometry().world.transform_point(p))
    }
}
#[test]
fn actual_edge_drags_affine_reflection_zoom_keep_outer_pose_and_one_undo() {
    for transform in [
        Affine::IDENTITY,
        Affine::translation(400., 100.)
            * Affine::rotation(25.)
            * Affine::skew(10., 0.)
            * Affine::scale(-1.4, 0.8),
    ] {
        let mut l = Lab::new(transform);
        l.editor.viewport.zoom = 1.7;
        l.editor.viewport.pan = Point::new(11., 21.);
        let before = l.editor.scene().clone();
        let h = l.geometry().target;
        let start = l.screen(Point::new(0., 50.));
        l.down(ImageCropPart::Handle(ResizeHandle::Left), start);
        assert_eq!(l.host.capture_owner(), Some(IMAGE_CROP_PLUGIN));
        for x in [10., 20., 50.] {
            l.moved(l.screen(Point::new(x, 50.)));
        }
        l.up(l.screen(Point::new(50., 50.)));
        let actual = l.value().unwrap();
        assert!((actual.origin.x - 0.25).abs() < 0.0001);
        assert!((actual.size.width - 0.75).abs() < 0.0001);
        let after = l.editor.scene().clone();
        let mut expected = before.clone();
        expected
            .update_object(&"i".into(), |o| {
                if let Shape::Image(i) = &mut o.shape {
                    i.source_crop = Some(actual)
                }
            })
            .unwrap();
        assert_eq!(after, expected);
        assert!(!l.host.has_capture());
        assert!(l.editor.undo());
        assert_eq!(l.editor.scene(), &before);
        assert!(!l.editor.undo());
        assert!(l.editor.redo());
        assert_eq!(l.editor.scene(), &after);
        assert_eq!(l.geometry().target, h);
    }
}
#[test]
fn body_drag_uses_frozen_delta_and_clamps_inside_source() {
    let mut l = Lab::new(Affine::IDENTITY);
    l.editor
        .transaction(|s| {
            s.update_object(&"i".into(), |o| {
                if let Shape::Image(i) = &mut o.shape {
                    i.source_crop = Some(crop(0.25, 0.25, 0.5, 0.5))
                }
            })
        })
        .unwrap();
    l.down(ImageCropPart::Move, Point::new(75., 40.));
    l.moved(Point::new(95., 50.));
    assert_eq!(l.value(), Some(crop(0.35, 0.35, 0.5, 0.5)));
    l.moved(Point::new(900., -200.));
    l.up(Point::new(900., -200.));
    assert_eq!(l.value(), Some(crop(0.5, 0., 0.5, 0.5)));
}
#[test]
fn escape_focus_loss_cancel_policy_and_pointer_mismatch_restore_owned_baseline() {
    for event in [
        CanvasEvent::PointerCancel,
        CanvasEvent::FocusLost,
        CanvasEvent::KeyDown {
            key: KeyCode::Escape,
            modifiers: Modifiers::default(),
        },
    ] {
        let mut l = Lab::new(Affine::IDENTITY);
        let before = l.editor.scene().clone();
        l.down(
            ImageCropPart::Handle(ResizeHandle::Right),
            Point::new(200., 50.),
        );
        l.moved(Point::new(120., 50.));
        assert_ne!(l.editor.scene(), &before);
        l.dispatch(event);
        assert_eq!(l.editor.scene(), &before);
        assert!(!l.host.has_capture());
        assert!(!l.editor.undo());
    }
    let mut l = Lab::new(Affine::IDENTITY);
    let before = l.editor.scene().clone();
    l.down(
        ImageCropPart::Handle(ResizeHandle::Bottom),
        Point::new(100., 100.),
    );
    l.moved(Point::new(100., 60.));
    l.host.set_editable(false, &mut l.editor);
    assert_eq!(l.editor.scene(), &before);
    assert!(!l.host.has_capture());
    l.down(
        ImageCropPart::Handle(ResizeHandle::Bottom),
        Point::new(100., 100.),
    );
    l.moved(Point::new(100., 60.));
    assert_eq!(l.editor.scene(), &before);
}
#[test]
fn noop_and_return_to_start_do_not_create_undo_or_replace_default_none() {
    let mut l = Lab::new(Affine::IDENTITY);
    l.down(
        ImageCropPart::Handle(ResizeHandle::Right),
        Point::new(200., 50.),
    );
    l.moved(Point::new(200., 50.));
    assert_eq!(l.value(), None);
    l.moved(Point::new(100., 50.));
    l.moved(Point::new(200., 50.));
    l.up(Point::new(200., 50.));
    assert_eq!(l.value(), None);
    assert!(!l.editor.undo());
}
#[test]
fn stale_source_selection_and_remount_revoke_capture_without_old_snapshot_replay() {
    for action in 0..4 {
        let mut l = Lab::new(Affine::IDENTITY);
        l.down(
            ImageCropPart::Handle(ResizeHandle::Right),
            Point::new(200., 50.),
        );
        l.moved(Point::new(120., 50.));
        match action {
            0 => {
                l.editor
                    .transaction(|s| {
                        s.update_object(&"i".into(), |o| {
                            if let Shape::Image(i) = &mut o.shape {
                                i.source = "new".into()
                            }
                        })
                    })
                    .unwrap();
            }
            1 => {
                l.editor.selection.clear();
            }
            2 => {
                let h = l.editor.engine.handle(&"i".into()).unwrap();
                let (detached, _) = l.editor.engine.detach(h).unwrap();
                l.editor
                    .engine
                    .attach(&detached, l.editor.engine.handle(&"p".into()), None)
                    .unwrap();
            }
            _ => {
                let mut scene = l.editor.scene().clone();
                scene
                    .update_object(&"i".into(), |o| {
                        if let Shape::Image(i) = &mut o.shape {
                            i.source_crop = None
                        }
                    })
                    .unwrap();
                l.editor.load_scene(scene).unwrap();
            }
        }
        let external = l.editor.scene().clone();
        l.moved(Point::new(50., 50.));
        assert_eq!(l.editor.scene(), &external);
        assert!(!l.host.has_capture());
        l.up(Point::new(50., 50.));
        assert_eq!(l.editor.scene(), &external);
    }
}
#[test]
fn rounded_and_clipped_geometry_does_not_expand_source_frame_hit_and_minimum_is_bounded() {
    let l = Lab::new(Affine::translation(20., 30.));
    let g = l.geometry();
    assert_eq!(
        g.hit(&l.editor, Point::new(20., 80.)),
        Some(ImageCropPart::Handle(ResizeHandle::Left))
    );
    assert_eq!(
        g.hit(&l.editor, Point::new(100., 80.)),
        Some(ImageCropPart::Move)
    );
    assert_eq!(g.hit(&l.editor, Point::new(400., 400.)), None);
}
struct Retarget;
impl CanvasPlugin<SceneEditor, SceneCommand> for Retarget {
    fn id(&self) -> &str {
        "retarget"
    }
    fn on_event(
        &mut self,
        _: &CanvasEvent,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut PluginOutput<SceneCommand>,
    ) {
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if matches!(command, SceneCommand::SetImageCrop { .. }) {
            *command = SceneCommand::DeleteSelection;
        }
        Ok(())
    }
}
#[test]
fn middleware_cannot_change_crop_command_kind_or_bypass_readonly() {
    let mut l = Lab::new(Affine::IDENTITY);
    l.host.install(Retarget).unwrap();
    let before = l.editor.scene().clone();
    l.down(
        ImageCropPart::Handle(ResizeHandle::Right),
        Point::new(200., 50.),
    );
    let r = l.event(CanvasEvent::PointerMove(PointerInput::left(
        Point::new(100., 50.),
        HitTarget::Background,
    )));
    assert!(!r.default.errors.is_empty());
    assert_eq!(l.editor.scene(), &before);
}
#[test]
fn wrong_pointer_does_not_take_capture_and_missing_button_cancels() {
    let mut l = Lab::new(Affine::IDENTITY);
    let before = l.editor.scene().clone();
    l.down(
        ImageCropPart::Handle(ResizeHandle::Right),
        Point::new(200., 50.),
    );
    l.moved(Point::new(100., 50.));
    let accepted = l.value();
    let mut p = PointerInput::left(Point::new(50., 50.), HitTarget::Background);
    p.pointer_id = 4;
    l.dispatch(CanvasEvent::PointerMove(p));
    assert_eq!(l.value(), accepted);
    assert!(l.host.has_capture());
    let p = PointerInput {
        position: Point::new(50., 50.),
        hit: HitTarget::Background,
        ..Default::default()
    };
    l.dispatch(CanvasEvent::PointerMove(p));
    assert_eq!(l.editor.scene(), &before);
    assert!(!l.host.has_capture());
}
struct RejectCrop;
impl gpui_flow_engine::LayoutProvider for RejectCrop {
    fn derive(
        &self,
        _: &gpui_flow_engine::SceneEngine,
        original: &Scene,
    ) -> Result<Scene, gpui_flow_engine::EngineError> {
        let mut scene = original.clone();
        scene.update_object(&"i".into(), |object| {
            if let Shape::Image(i) = &mut object.shape {
                i.source_crop = None
            }
        })?;
        Ok(scene)
    }
}
#[test]
fn derived_layout_cannot_replace_crop_or_outer_pose_and_failure_is_atomic() {
    let mut l = Lab::new(Affine::IDENTITY);
    l.editor.engine.install_layout_provider(RejectCrop).unwrap();
    let before = l.editor.scene().clone();
    l.down(
        ImageCropPart::Handle(ResizeHandle::Right),
        Point::new(200., 50.),
    );
    let r = l.event(CanvasEvent::PointerMove(PointerInput::left(
        Point::new(100., 50.),
        HitTarget::Background,
    )));
    assert!(!r.default.errors.is_empty());
    assert_eq!(l.editor.scene(), &before);
    assert!(!l.editor.undo());
}
#[test]
fn locked_ancestor_singular_transform_and_wrong_shape_hide_geometry() {
    for action in 0..3 {
        let mut l = Lab::new(Affine::IDENTITY);
        l.editor
            .transaction(|s| {
                s.update_object(
                    &if action == 0 { "p" } else { "i" }.into(),
                    |o| match action {
                        0 => o.locked = true,
                        1 => o.transform = Affine::scale(0., 1.),
                        _ => o.shape = display::Rect::new(200., 100.).into(),
                    },
                )
            })
            .unwrap();
        assert!(SceneImageCropGeometry::for_selection(&l.editor).is_none());
    }
}
