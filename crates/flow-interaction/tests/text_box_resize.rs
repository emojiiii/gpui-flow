use gpui_flow_core::{Point, Size};
use gpui_flow_editor::{ResizeHandle, SceneEditor};
use gpui_flow_engine::{EngineError, LayoutProvider, SceneEngine};
use gpui_flow_interaction::{
    scene::{
        plugins::{SceneResizePlugin, TextResizeMode},
        *,
    },
    *,
};
use gpui_flow_scene::{Affine, DisplayObject, Scene, Shape, display};
struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
}
impl Lab {
    fn new(mode: TextResizeMode) -> Self {
        let mut s = Scene::new();
        let mut t = display::Text::new("Font stays fixed; content wraps.", Size::new(120., 60.));
        t.padding = [4., 8., 6., 7.];
        t.font_size = 18.;
        s.insert(
            DisplayObject::new("t", t).with_transform(Affine::translation(40., 30.)),
            None,
            None,
        )
        .unwrap();
        let mut editor = SceneEditor::new(s).unwrap();
        editor.selection.select("t".into(), false);
        let mut host = ScenePluginHost::with_defaults();
        host.replace(
            SceneResizePlugin::default().with_text_resize_mode(mode),
            &mut editor,
        )
        .unwrap();
        Self {
            editor,
            host,
            router: SceneEventRouter::new(),
        }
    }
    fn text(&self) -> &display::Text {
        match &self.editor.scene().get(&"t".into()).unwrap().object.shape {
            Shape::Text(t) => t,
            _ => panic!(),
        }
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
        let g = SceneResizeGeometry::for_target_with_capabilities(
            &self.editor,
            &"t".into(),
            self.host.transform_capabilities(),
        )
        .unwrap();
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
}
#[test]
fn box_mode_changes_dimensions_and_anchor_but_keeps_font_content_and_linear_pose() {
    for h in ResizeHandle::ALL {
        let mut l = Lab::new(TextResizeMode::Box);
        let old = l.text().clone();
        let m = l.matrix();
        let before = l.editor.scene().clone();
        let p = l.down(h);
        let delta = (h.position(old.bounds()) - h.anchor(old.bounds())) * 0.3;
        l.movement(p + delta, false);
        l.up(p + delta);
        assert_eq!(l.text().font_size, old.font_size);
        assert_eq!(l.text().content, old.content);
        assert_eq!(l.text().padding, old.padding);
        assert_eq!(
            [l.matrix().a, l.matrix().b, l.matrix().c, l.matrix().d],
            [m.a, m.b, m.c, m.d]
        );
        assert!(
            l.matrix()
                .transform_point(h.anchor(l.text().bounds()))
                .distance(m.transform_point(h.anchor(old.bounds())))
                < 0.004
        );
        let after = l.editor.scene().clone();
        assert!(l.editor.undo());
        assert_eq!(l.editor.scene(), &before);
        assert!(!l.editor.undo());
        assert!(l.editor.redo());
        assert_eq!(l.editor.scene(), &after);
    }
}
#[test]
fn box_mode_native_only_side_resize_and_shift_are_distinct_from_default_transform() {
    for (mode, shift) in [
        (TextResizeMode::Box, false),
        (TextResizeMode::Box, true),
        (TextResizeMode::Transform, false),
    ] {
        let mut l = Lab::new(mode);
        let p = l.down(ResizeHandle::Right);
        l.movement(p + Point::new(60., 0.), shift);
        l.up(p);
        if mode == TextResizeMode::Box {
            assert_eq!(
                l.text().size,
                Size::new(180., if shift { 90. } else { 60. })
            );
            assert_eq!(l.matrix().a, 1.);
            assert_eq!(l.matrix().d, 1.);
        } else {
            assert_eq!(l.text().size, Size::new(120., 60.));
            assert_eq!(l.matrix().a, 1.5);
            assert_eq!(l.matrix().d, 1.5);
        }
    }
}
#[test]
fn box_mode_works_at_existing_affine_pose_without_changing_linear_terms() {
    let mut l = Lab::new(TextResizeMode::Box);
    l.host
        .set_transform_capabilities(TransformCapabilities { affine_text: true }, &mut l.editor);
    l.editor
        .engine
        .transaction(|s| {
            s.update_object(&"t".into(), |o| {
                o.transform = Affine::translation(100., 100.)
                    * Affine::rotation(30.)
                    * Affine::scale(-1., 1.2)
            })
        })
        .unwrap();
    let original = l.matrix();
    let p = l.down(ResizeHandle::Right);
    l.movement(p + original.transform_vector(Point::new(30., 0.)), false);
    l.up(p);
    assert!((l.text().size.width - 150.).abs() < 0.001);
    assert_eq!(l.text().size.height, 60.);
    assert_eq!(
        [l.matrix().a, l.matrix().b, l.matrix().c, l.matrix().d],
        [original.a, original.b, original.c, original.d]
    );
}
#[test]
fn changing_resize_plugin_cancels_owned_box_preview_and_preserves_redo() {
    let mut l = Lab::new(TextResizeMode::Box);
    l.editor
        .transaction(|s| s.update_object(&"t".into(), |o| o.transform.tx += 5.))
        .unwrap();
    let redo = l.editor.scene().clone();
    assert!(l.editor.undo());
    let before = l.editor.scene().clone();
    let p = l.down(ResizeHandle::Right);
    l.movement(p + Point::new(50., 0.), false);
    assert!(l.host.has_capture());
    let r = l
        .host
        .replace(SceneResizePlugin::default(), &mut l.editor)
        .unwrap();
    assert!(r.document_changed);
    assert!(!l.host.has_capture());
    assert_eq!(l.editor.scene(), &before);
    assert!(l.editor.redo());
    assert_eq!(l.editor.scene(), &redo);
}
#[test]
fn stale_content_edit_is_not_overwritten_and_direct_box_command_uses_current_text() {
    let mut l = Lab::new(TextResizeMode::Box);
    let p = l.down(ResizeHandle::Right);
    l.movement(p + Point::new(20., 0.), false);
    let h = l.editor.engine.handle(&"t".into()).unwrap();
    l.editor
        .engine
        .set_property(
            h,
            gpui_flow_engine::PropertyKey::TextContent,
            "external new content".into(),
        )
        .unwrap();
    let external = l.editor.scene().clone();
    l.movement(p + Point::new(40., 0.), false);
    assert_eq!(l.editor.scene(), &external);
    assert!(!l.host.has_capture());
    let matrix = l.matrix();
    l.host
        .execute(
            &mut l.editor,
            SceneCommand::SetTextBoxResize {
                target: h,
                size: Size::new(220., 80.),
                transform: matrix,
            },
        )
        .unwrap();
    assert_eq!(l.text().content, "external new content");
}
#[test]
fn typed_box_command_rejects_linear_changes_padding_failure_size_budgets_and_readonly() {
    let mut l = Lab::new(TextResizeMode::Box);
    let h = l.editor.engine.handle(&"t".into()).unwrap();
    let before = l.editor.scene().clone();
    for (size, transform) in [
        (Size::new(200., 80.), Affine::scale(2., 1.)),
        (Size::new(14., 60.), l.matrix()),
        (Size::new(f32::NAN, 60.), l.matrix()),
        (Size::new(1_000_001., 60.), l.matrix()),
    ] {
        assert!(
            l.host
                .execute(
                    &mut l.editor,
                    SceneCommand::SetTextBoxResize {
                        target: h,
                        size,
                        transform
                    }
                )
                .is_err()
        );
        assert_eq!(l.editor.scene(), &before);
    }
    l.host.set_editable(false, &mut l.editor);
    let transform = l.matrix();
    assert!(
        l.host
            .execute(
                &mut l.editor,
                SceneCommand::SetTextBoxResize {
                    target: h,
                    size: Size::new(150., 60.),
                    transform
                }
            )
            .is_err()
    );
}
struct Downgrade;
impl CanvasPlugin<SceneEditor, SceneCommand> for Downgrade {
    fn id(&self) -> &str {
        "downgrade"
    }
    fn filter_command(
        &mut self,
        c: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetTextBoxResize { transform, .. } = c {
            *c = SceneCommand::SetTransform {
                id: "t".into(),
                transform: *transform,
            };
        }
        Ok(())
    }
}
#[test]
fn middleware_cannot_downgrade_typed_box_resize() {
    let mut l = Lab::new(TextResizeMode::Box);
    l.host.install(Downgrade).unwrap();
    let before = l.editor.scene().clone();
    let p = l.down(ResizeHandle::Right);
    let r = l.event(CanvasEvent::PointerMove(PointerInput::left(
        p + Point::new(20., 0.),
        HitTarget::Background,
    )));
    assert!(!r.default.errors.is_empty());
    assert_eq!(l.editor.scene(), &before);
    assert!(!l.host.has_capture());
    assert!(!l.editor.undo());
}
struct OwnedSize;
impl LayoutProvider for OwnedSize {
    fn derive(&self, _: &SceneEngine, s: &Scene) -> Result<Scene, EngineError> {
        let mut n = s.clone();
        if matches!(&s.get(&"t".into()).unwrap().object.shape,Shape::Text(t) if t.size.width!=120.)
        {
            n.update_object(&"t".into(), |o| {
                if let Shape::Text(t) = &mut o.shape {
                    t.size.height = 61.;
                }
            })?;
        }
        Ok(n)
    }
}
#[test]
fn derived_layout_cannot_silently_overwrite_requested_box() {
    let mut l = Lab::new(TextResizeMode::Box);
    l.editor.engine.install_layout_provider(OwnedSize).unwrap();
    let before = l.editor.scene().clone();
    let revision = l.editor.engine.revision();
    let p = l.down(ResizeHandle::Right);
    let r = l.event(CanvasEvent::PointerMove(PointerInput::left(
        p + Point::new(20., 0.),
        HitTarget::Background,
    )));
    assert!(!r.default.errors.is_empty());
    assert_eq!(l.editor.scene(), &before);
    assert_eq!(l.editor.engine.revision(), revision);
    assert!(!l.host.has_capture());
}
