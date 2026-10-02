use gpui_flow_core::{Point, Size};
use gpui_flow_editor::{ResizeHandle, SceneEditor};
use gpui_flow_engine::{EngineError, LayoutProvider, SceneEngine};
use gpui_flow_interaction::{scene::*, *};
use gpui_flow_scene::{Affine, Color, DisplayObject, Scene, Shadow, Shape, Stroke, Style, display};
struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
}
impl Lab {
    fn new() -> Self {
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("t", display::Text::new("Text", Size::new(120., 60.)))
                .with_transform(Affine::translation(100., 80.)),
            None,
            None,
        )
        .unwrap();
        let mut editor = SceneEditor::new(s).unwrap();
        editor.selection.select("t".into(), false);
        Self {
            editor,
            host: ScenePluginHost::with_defaults(),
            router: SceneEventRouter::new(),
        }
    }
    fn allow(&mut self, value: bool) -> DispatchReport {
        self.host.set_transform_capabilities(
            TransformCapabilities { affine_text: value },
            &mut self.editor,
        )
    }
    fn caps(&self) -> TransformCapabilities {
        self.host.transform_capabilities()
    }
    fn resize(&self) -> SceneResizeGeometry {
        SceneResizeGeometry::for_target_with_capabilities(&self.editor, &"t".into(), self.caps())
            .unwrap()
    }
    fn rotation(&self) -> SceneRotationGeometry {
        SceneRotationGeometry::for_selection_with_capabilities(&self.editor, self.caps()).unwrap()
    }
    fn matrix(&self) -> Affine {
        self.editor
            .scene()
            .get(&"t".into())
            .unwrap()
            .object
            .transform
    }
    fn event(&mut self, e: CanvasEvent) -> RoutedReport {
        self.router.dispatch(&mut self.editor, &mut self.host, e)
    }
    fn dispatch(&mut self, e: CanvasEvent) {
        let r = self.event(e);
        assert!(r.errors.is_empty(), "{:?}", r.errors);
        assert!(r.default.errors.is_empty(), "{:?}", r.default.errors);
    }
    fn down(&mut self, h: ResizeHandle) -> Point {
        let g = self.resize();
        let p = self
            .editor
            .viewport
            .world_to_screen(g.world.transform_point(h.position(g.bounds)));
        self.dispatch(CanvasEvent::PointerDown(PointerInput::left(
            p,
            HitTarget::ObjectResizeHandle {
                id: "t".into(),
                handle: h,
            },
        )));
        p
    }
    fn movement(&mut self, p: Point, shift: bool) {
        let mut i = PointerInput::left(p, HitTarget::Background);
        i.modifiers.shift = shift;
        self.dispatch(CanvasEvent::PointerMove(i));
    }
    fn up(&mut self, p: Point) {
        self.dispatch(CanvasEvent::PointerUp(PointerInput::left(
            p,
            HitTarget::Background,
        )));
    }
    fn down_rotation(&mut self) -> (Point, Point) {
        let g = self.rotation();
        let p = g.screen_handle(&self.editor).unwrap().position;
        self.dispatch(CanvasEvent::PointerDown(PointerInput::left(
            p,
            HitTarget::SelectionRotateHandle,
        )));
        (p, self.editor.viewport.world_to_screen(g.pivot))
    }
}
fn close(a: Affine, b: Affine) {
    for (x, y) in [a.a, a.b, a.c, a.d, a.tx, a.ty]
        .into_iter()
        .zip([b.a, b.b, b.c, b.d, b.tx, b.ty])
    {
        assert!((x - y).abs() < 0.003, "{a:?} != {b:?}");
    }
}
#[test]
fn affine_text_handles_are_opt_in_and_named_resize_preserves_intrinsics() {
    for enabled in [false, true] {
        let mut l = Lab::new();
        l.allow(enabled);
        assert_eq!(l.resize().uniform, !enabled);
        assert_eq!(
            SceneRotationGeometry::for_selection_with_capabilities(&l.editor, l.caps()).is_some(),
            enabled
        );
        let before = l.editor.scene().clone();
        let old = l.matrix();
        let shape = l
            .editor
            .scene()
            .get(&"t".into())
            .unwrap()
            .object
            .shape
            .clone();
        let p = l.down(ResizeHandle::Right);
        l.movement(p + Point::new(60., 0.), false);
        l.up(p);
        assert_eq!(l.matrix().a, 1.5);
        assert_eq!(l.matrix().d, if enabled { 1. } else { 1.5 });
        assert_eq!(
            l.editor.scene().get(&"t".into()).unwrap().object.shape,
            shape
        );
        let after = l.editor.scene().clone();
        assert!(l.editor.undo());
        assert_eq!(l.editor.scene(), &before);
        assert!(!l.editor.undo());
        assert!(l.editor.redo());
        assert_eq!(l.editor.scene(), &after);
        assert_ne!(l.matrix(), old);
    }
}
#[test]
fn affine_text_named_handles_work_under_rotation_and_reflection_with_shift() {
    for pose in [
        Affine::rotation(30.),
        Affine::scale(-1., 1.),
        Affine::skew(20., 0.),
    ] {
        for h in ResizeHandle::ALL {
            let mut l = Lab::new();
            l.editor
                .engine
                .transaction(|s| {
                    s.update_object(&"t".into(), |o| {
                        o.transform = Affine::translation(100., 80.) * pose
                    })
                })
                .unwrap();
            l.allow(true);
            let g = l.resize();
            let p = l.down(h);
            let delta = g
                .world
                .transform_vector((h.position(g.bounds) - h.anchor(g.bounds)) * 0.25);
            l.movement(p + delta, true);
            l.up(p + delta);
            let next = l.matrix();
            let anchor = h.anchor(g.bounds);
            assert!(
                next.transform_point(anchor)
                    .distance(g.original.transform_point(anchor))
                    < 0.004
            );
            let relative = g.original.inverse().unwrap() * next;
            assert!((relative.a - relative.d).abs() < 0.0001);
        }
    }
}
#[test]
fn affine_text_rotation_keeps_pivot_and_one_undo() {
    let mut l = Lab::new();
    l.allow(true);
    let original = l.editor.scene().clone();
    let g = l.rotation();
    let (p, pivot) = l.down_rotation();
    let target = Affine::rotation(44.).around(pivot).transform_point(p);
    l.movement(target, true);
    l.up(target);
    close(
        l.matrix(),
        Affine::rotation(45.).around(g.pivot) * Affine::translation(100., 80.),
    );
    assert!(l.editor.undo());
    assert_eq!(l.editor.scene(), &original);
    assert!(!l.editor.undo());
}
#[test]
fn affine_text_capability_revocation_cancels_resize_and_preserves_redo() {
    let mut l = Lab::new();
    l.editor
        .transaction(|s| s.update_object(&"t".into(), |o| o.transform.tx += 15.))
        .unwrap();
    let redo = l.editor.scene().clone();
    assert!(l.editor.undo());
    let before = l.editor.scene().clone();
    l.allow(true);
    let p = l.down(ResizeHandle::Right);
    l.movement(p + Point::new(30., 0.), false);
    assert!(l.host.has_capture());
    let same = l.allow(true);
    assert!(!same.document_changed);
    assert!(l.host.has_capture());
    let r = l.allow(false);
    assert!(r.document_changed);
    assert!(!l.host.has_capture());
    assert!(!l.editor.editing());
    assert_eq!(l.editor.scene(), &before);
    l.up(p);
    assert!(l.editor.redo());
    assert_eq!(l.editor.scene(), &redo);
}
#[test]
fn affine_text_revocation_cancels_rotation_without_overwriting_external_edits() {
    for external in [false, true] {
        let mut l = Lab::new();
        l.allow(true);
        let before = l.editor.scene().clone();
        let (p, pivot) = l.down_rotation();
        l.movement(
            Affine::rotation(20.).around(pivot).transform_point(p),
            false,
        );
        if external {
            l.editor
                .engine
                .transaction(|s| {
                    s.update_object(&"t".into(), |o| {
                        if let Shape::Text(t) = &mut o.shape {
                            t.content = "external".into();
                        }
                    })
                })
                .unwrap();
        }
        let current = l.editor.scene().clone();
        l.allow(false);
        assert!(!l.host.has_capture());
        assert!(!l.editor.editing());
        assert_eq!(l.editor.scene(), if external { &current } else { &before });
    }
}
#[test]
fn affine_text_capability_never_grants_edit_permission_or_untyped_rotation() {
    let mut l = Lab::new();
    let h = l.editor.engine.handle(&"t".into()).unwrap();
    assert!(
        l.host
            .execute(
                &mut l.editor,
                SceneCommand::SetRotationTransforms {
                    transforms: vec![(h, Affine::rotation(30.))]
                }
            )
            .is_err()
    );
    l.allow(true);
    l.host.set_editable(false, &mut l.editor);
    let (p, _) = l.down_rotation();
    assert!(!l.host.has_capture());
    l.movement(p + Point::new(20., 20.), false);
    assert_eq!(l.matrix(), Affine::translation(100., 80.));
    let other = Lab::new();
    assert!(!other.caps().affine_text);
    assert!(other.resize().uniform);
}
#[test]
fn affine_text_styles_without_a_glyph_contract_keep_conservative_handles() {
    for mode in 0..3 {
        let mut l = Lab::new();
        l.editor
            .engine
            .transaction(|s| {
                s.update_object(&"t".into(), |o| match mode {
                    0 => o.style.stroke = Some(Stroke::new(Color::WHITE, 1.)),
                    1 => o.style.shadow = Some(Shadow::new(1., 1., 2., Color::BLACK)),
                    _ => {
                        if let Shape::Text(t) = &mut o.shape {
                            t.content = "x".repeat(65_537);
                        }
                    }
                })
            })
            .unwrap();
        l.allow(true);
        assert!(l.resize().uniform);
        assert!(
            SceneRotationGeometry::for_selection_with_capabilities(&l.editor, l.caps()).is_none()
        );
    }
}
struct UnsupportedDerived;
impl LayoutProvider for UnsupportedDerived {
    fn derive(&self, _: &SceneEngine, s: &Scene) -> Result<Scene, EngineError> {
        let mut n = s.clone();
        if s.get(&"t".into()).unwrap().object.transform.a != 1. {
            n.update_object(&"t".into(), |o| {
                o.style.stroke = Some(Stroke::new(Color::WHITE, 1.))
            })?;
        }
        Ok(n)
    }
}
#[test]
fn affine_text_post_layout_style_change_rejects_the_whole_gesture() {
    let mut l = Lab::new();
    l.allow(true);
    l.editor
        .engine
        .install_layout_provider(UnsupportedDerived)
        .unwrap();
    let before = l.editor.scene().clone();
    let revision = l.editor.engine.revision();
    let p = l.down(ResizeHandle::Right);
    let r = l.event(CanvasEvent::PointerMove(PointerInput::left(
        p + Point::new(40., 0.),
        HitTarget::Background,
    )));
    assert!(!r.default.errors.is_empty());
    assert_eq!(l.editor.scene(), &before);
    assert_eq!(l.editor.engine.revision(), revision);
    assert!(!l.host.has_capture());
    assert!(!l.editor.undo());
}
#[test]
fn affine_text_and_image_shared_selection_supports_nonuniform_root_batch() {
    let mut l = Lab::new();
    l.editor
        .engine
        .transaction(|s| {
            s.insert(
                DisplayObject::new("image", display::Image::new("fixture", Size::new(60., 40.)))
                    .with_transform(Affine::translation(280., 80.))
                    .with_style(Style::default()),
                None,
                None,
            )
        })
        .unwrap();
    l.editor.selection.select("image".into(), true);
    l.allow(true);
    let g =
        SceneSelectionResizeGeometry::for_selection_with_capabilities(&l.editor, l.caps()).unwrap();
    assert!(!g.geometry.uniform);
    let bounds = g.geometry.bounds;
    let p = l
        .editor
        .viewport
        .world_to_screen(ResizeHandle::Right.position(bounds));
    let before = l.editor.scene().clone();
    let old = l.matrix();
    l.dispatch(CanvasEvent::PointerDown(PointerInput::left(
        p,
        HitTarget::SelectionResizeHandle {
            handle: ResizeHandle::Right,
        },
    )));
    l.movement(p + Point::new(bounds.size.width * 0.5, 0.), false);
    l.up(p);
    close(
        l.matrix(),
        Affine::scale(1.5, 1.).around(ResizeHandle::Right.anchor(bounds)) * old,
    );
    assert!(l.editor.undo());
    assert_eq!(l.editor.scene(), &before);
}
