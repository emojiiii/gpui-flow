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
        let mut t = display::Text::new("Font resizes; text is preserved.", Size::new(120., 60.));
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

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.001, "{a} != {b}");
}
#[test]
fn font_mode_all_handles_update_font_padding_and_box_with_one_undo() {
    for handle in ResizeHandle::ALL {
        let mut l = Lab::new(TextResizeMode::FontSize);
        let before = l.editor.scene().clone();
        let original = l.text().clone();
        let matrix = l.matrix();
        let p = l.down(handle);
        let q = p + Point::new(12., 6.);
        l.movement(q, false);
        l.up(q);
        let scale = l.text().font_size / original.font_size;
        assert_ne!(scale, 1.);
        close(l.text().size.width, original.size.width * scale);
        close(l.text().size.height, original.size.height * scale);
        for (a, b) in l.text().padding.into_iter().zip(original.padding) {
            close(a, b * scale);
        }
        assert_eq!(l.text().content, original.content);
        assert_eq!(l.text().line_height, original.line_height);
        assert_eq!(
            [l.matrix().a, l.matrix().b, l.matrix().c, l.matrix().d],
            [matrix.a, matrix.b, matrix.c, matrix.d]
        );
        let after = l.editor.scene().clone();
        assert!(l.editor.undo());
        assert_eq!(l.editor.scene(), &before);
        assert!(l.editor.redo());
        assert_eq!(l.editor.scene(), &after);
    }
}
#[test]
fn font_preview_is_frozen_not_compounded_and_shift_does_not_change_font_axis() {
    for shift in [false, true] {
        let mut l = Lab::new(TextResizeMode::FontSize);
        let p = l.down(ResizeHandle::BottomRight);
        l.movement(p + Point::new(12., 40.), shift);
        l.movement(p + Point::new(24., 40.), shift);
        close(l.text().font_size, 21.6);
        close(l.text().size.width, 144.);
        close(l.text().size.height, 72.);
    }
    let mut l = Lab::new(TextResizeMode::FontSize);
    let p = l.down(ResizeHandle::Bottom);
    l.movement(p + Point::new(24., 30.), false);
    close(l.text().font_size, 27.);
}
#[test]
fn font_mode_works_without_bridge_for_native_pose_and_preserves_existing_affine() {
    let mut l = Lab::new(TextResizeMode::FontSize);
    let p = l.down(ResizeHandle::Right);
    l.movement(p + Point::new(60., 0.), false);
    l.up(p);
    close(l.text().font_size, 27.);
    let mut l = Lab::new(TextResizeMode::FontSize);
    l.host
        .set_transform_capabilities(TransformCapabilities { affine_text: true }, &mut l.editor);
    let pose = Affine::translation(40., 30.) * Affine::rotation(25.) * Affine::scale(-1.1, 0.8);
    l.editor
        .transaction(|s| s.update_object(&"t".into(), |o| o.transform = pose))
        .unwrap();
    let p = l.down(ResizeHandle::Right);
    let delta = pose.transform_point(Point::new(20., 0.)) - pose.transform_point(Point::ZERO);
    l.movement(p + delta, false);
    l.up(p + delta);
    close(l.text().font_size, 21.);
    assert_eq!(
        [l.matrix().a, l.matrix().b, l.matrix().c, l.matrix().d],
        [pose.a, pose.b, pose.c, pose.d]
    );
}
#[test]
fn default_transform_and_box_modes_keep_font_size() {
    for mode in [TextResizeMode::Transform, TextResizeMode::Box] {
        let mut l = Lab::new(mode);
        let p = l.down(ResizeHandle::Right);
        l.movement(p + Point::new(30., 0.), false);
        l.up(p);
        assert_eq!(l.text().font_size, 18.);
    }
}
#[test]
fn font_mode_plugin_replace_restores_owned_preview_and_redo() {
    let mut l = Lab::new(TextResizeMode::FontSize);
    l.editor
        .transaction(|s| s.update_object(&"t".into(), |o| o.transform.tx += 5.))
        .unwrap();
    let redo = l.editor.scene().clone();
    assert!(l.editor.undo());
    let before = l.editor.scene().clone();
    let p = l.down(ResizeHandle::Right);
    l.movement(p + Point::new(30., 0.), false);
    l.host
        .replace(SceneResizePlugin::default(), &mut l.editor)
        .unwrap();
    assert_eq!(l.editor.scene(), &before);
    assert!(l.editor.redo());
    assert_eq!(l.editor.scene(), &redo);
}
#[test]
fn font_mode_external_content_change_cancels_stale_capture_without_overwrite() {
    let mut l = Lab::new(TextResizeMode::FontSize);
    let p = l.down(ResizeHandle::Right);
    l.movement(p + Point::new(20., 0.), false);
    let h = l.editor.engine.handle(&"t".into()).unwrap();
    l.editor
        .engine
        .set_property(
            h,
            gpui_flow_engine::PropertyKey::TextContent,
            "external font text".into(),
        )
        .unwrap();
    let external = l.editor.scene().clone();
    l.movement(p + Point::new(40., 0.), false);
    assert_eq!(l.editor.scene(), &external);
    assert!(!l.host.has_capture());
    let t = l.text().clone();
    let r = gpui_flow_editor::TextFontResize {
        size: t.size,
        padding: t.padding,
        font_size: 20.,
        transform: l.matrix(),
    };
    l.host
        .execute(
            &mut l.editor,
            SceneCommand::SetTextFontResize {
                target: h,
                resize: r,
            },
        )
        .unwrap();
    assert_eq!(l.text().content, "external font text");
    assert_eq!(l.text().line_height, t.line_height);
}
#[test]
fn typed_font_command_rejects_invalid_values_linear_changes_and_policy() {
    let mut l = Lab::new(TextResizeMode::FontSize);
    let h = l.editor.engine.handle(&"t".into()).unwrap();
    let before = l.editor.scene().clone();
    let original = gpui_flow_editor::TextFontResize {
        size: l.text().size,
        padding: l.text().padding,
        font_size: l.text().font_size,
        transform: l.matrix(),
    };
    for bad in [
        gpui_flow_editor::TextFontResize {
            font_size: f32::NAN,
            ..original
        },
        gpui_flow_editor::TextFontResize {
            font_size: 513.,
            ..original
        },
        gpui_flow_editor::TextFontResize {
            padding: [100.; 4],
            ..original
        },
        gpui_flow_editor::TextFontResize {
            transform: Affine::scale(2., 2.),
            ..original
        },
    ] {
        assert!(
            l.host
                .execute(
                    &mut l.editor,
                    SceneCommand::SetTextFontResize {
                        target: h,
                        resize: bad
                    }
                )
                .is_err()
        );
        assert_eq!(l.editor.scene(), &before);
    }
    l.host.set_editable(false, &mut l.editor);
    assert!(
        l.host
            .execute(
                &mut l.editor,
                SceneCommand::SetTextFontResize {
                    target: h,
                    resize: original
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
        if let SceneCommand::SetTextFontResize { resize, .. } = c {
            *c = SceneCommand::SetTransform {
                id: "t".into(),
                transform: resize.transform,
            };
        }
        Ok(())
    }
}
#[test]
fn middleware_cannot_downgrade_typed_font_resize() {
    let mut l = Lab::new(TextResizeMode::FontSize);
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
fn derived_layout_cannot_silently_overwrite_requested_font_box() {
    let mut l = Lab::new(TextResizeMode::FontSize);
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
