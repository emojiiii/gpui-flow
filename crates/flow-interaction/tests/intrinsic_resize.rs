use gpui_flow_editor::{BuiltinResizeGeometry, ResizeHandle, ResizeOptions, SceneEditor};
use gpui_flow_interaction::{
    scene::{
        plugins::{SceneResizePlugin, TextResizeMode},
        *,
    },
    *,
};
use gpui_flow_layout::{
    AxisSizing, FlowItem, FlowLayout, FlowStyle, JustifyContent, TextMeasureRequest, TextMeasurer,
    TextMetrics, TextSizing,
};
use gpui_flow_scene::{
    Affine, Color, DisplayObject, Point, Scene, Shape, Size, Stroke, Style, display,
};
use std::sync::Arc;

struct Lab {
    editor: SceneEditor,
    host: ScenePluginHost,
    router: SceneEventRouter,
}
impl Lab {
    fn new(scene: Scene) -> Self {
        let mut editor = SceneEditor::new(scene).unwrap();
        editor.selection.select("s".into(), false);
        let mut host = ScenePluginHost::with_defaults();
        host.replace(
            SceneResizePlugin::default()
                .with_builtin_resize(true)
                .with_text_resize_mode(TextResizeMode::Box),
            &mut editor,
        )
        .unwrap();
        Self {
            editor,
            host,
            router: SceneEventRouter::new(),
        }
    }
    fn object(&self) -> &DisplayObject {
        &self.editor.scene().get(&"s".into()).unwrap().object
    }
    fn event(&mut self, event: CanvasEvent) -> RoutedReport {
        self.router
            .dispatch(&mut self.editor, &mut self.host, event)
    }
    fn dispatch(&mut self, event: CanvasEvent) {
        let r = self.event(event);
        assert!(
            r.errors.is_empty() && r.default.errors.is_empty(),
            "{:?} {:?}",
            r.errors,
            r.default.errors
        );
    }
    fn down(&mut self, handle: ResizeHandle) -> Point {
        let g = self
            .host
            .resize_geometry(&self.editor, &"s".into(), &self.editor.engine)
            .unwrap();
        let p = self
            .editor
            .viewport
            .world_to_screen(g.world.transform_point(handle.position(g.bounds)));
        self.dispatch(CanvasEvent::PointerDown(PointerInput::left(
            p,
            HitTarget::ObjectResizeHandle {
                id: "s".into(),
                handle,
            },
        )));
        assert!(self.host.has_capture());
        p
    }
    fn movement(&mut self, p: Point) -> RoutedReport {
        self.event(CanvasEvent::PointerMove(PointerInput::left(
            p,
            HitTarget::Background,
        )))
    }
    fn up(&mut self, p: Point) {
        self.dispatch(CanvasEvent::PointerUp(PointerInput::left(
            p,
            HitTarget::Background,
        )));
    }
}
fn scene(shape: impl Into<Shape>) -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(DisplayObject::new("s", shape), None, None)
        .unwrap();
    scene
}
fn ok(report: RoutedReport) {
    assert!(
        report.errors.is_empty() && report.default.errors.is_empty(),
        "{:?} {:?}",
        report.errors,
        report.default.errors
    );
}

#[test]
fn intrinsic_gestures_preserve_pose_style_and_one_undo_for_every_builtin_policy() {
    let path = display::Pen::new().move_to(Point::new(-10., 10.)).cubic_to(
        Point::new(20., 50.),
        Point::new(60., -10.),
        Point::new(110., 70.),
    );
    let kinds: Vec<Shape> = vec![
        display::Rect::new(120., 60.).rounded(12.).into(),
        display::Ellipse::new(120., 60.).sector(0., 100.).into(),
        display::Image::new("image", Size::new(120., 60.)).into(),
        display::Canvas::new(120., 60.).into(),
        display::Box::new(120., 60.).into(),
        display::Frame::new(120., 60.).into(),
        display::Line::new(Point::ZERO, Point::new(120., 60.)).into(),
        display::Polygon::new(vec![
            Point::ZERO,
            Point::new(120., 10.),
            Point::new(80., 60.),
        ])
        .into(),
        display::Star::new(5, 50., 20.).into(),
        path.path.clone().into(),
        path.into(),
        display::Group::new().into(),
    ];
    for shape in kinds {
        let mut scene = scene(shape);
        if matches!(
            scene.get(&"s".into()).unwrap().object.shape,
            Shape::Group(_)
        ) {
            scene
                .insert(
                    DisplayObject::new("child", display::Rect::new(120., 60.)),
                    Some("s".into()),
                    None,
                )
                .unwrap();
        }
        scene
            .update_object(&"s".into(), |o| {
                o.transform =
                    Affine::translation(40., 30.) * Affine::rotation(20.) * Affine::scale(-1., 1.2);
                o.style.stroke = Some(Stroke::new(Color::BLACK, 3.));
            })
            .unwrap();
        let mut l = Lab::new(scene);
        let before = l.editor.scene().clone();
        let old = l.object().clone();
        let p = l.down(ResizeHandle::TopLeft);
        for delta in [Point::new(-10., -5.), Point::new(-30., -20.)] {
            ok(l.movement(p + old.transform.transform_vector(delta)));
        }
        l.up(p);
        let after = l.editor.scene().clone();
        assert_ne!(after, before, "{}", old.shape.tag());
        assert_eq!(l.object().style, old.style);
        let a = old.transform;
        let b = l.object().transform;
        assert_eq!([a.a, a.b, a.c, a.d], [b.a, b.b, b.c, b.d]);
        assert!(l.editor.undo());
        assert_eq!(l.editor.scene(), &before);
        assert!(!l.editor.undo());
        assert!(l.editor.redo());
        assert_eq!(l.editor.scene(), &after);
    }
}
#[test]
fn nominal_arc_and_stroked_box_handles_share_the_plugin_input_bounds() {
    for shape in [
        Shape::Ellipse(display::Ellipse::new(120., 60.).sector(0., 80.)),
        Shape::Rect(display::Rect::new(120., 60.)),
    ] {
        let mut s = scene(shape);
        s.update_object(&"s".into(), |o| {
            o.style = Style {
                stroke: Some(Stroke::new(Color::BLACK, 10.)),
                ..Default::default()
            }
        })
        .unwrap();
        let mut l = Lab::new(s);
        let g = l
            .host
            .resize_geometry(&l.editor, &"s".into(), &l.editor.engine)
            .unwrap();
        assert_eq!(
            g.bounds,
            gpui_flow_scene::Bounds::new(Point::ZERO, Size::new(120., 60.))
        );
        let p = l.down(ResizeHandle::Right);
        ok(l.movement(p + Point::new(60., 0.)));
        l.up(p);
        let size = match &l.object().shape {
            Shape::Ellipse(s) => s.size,
            Shape::Rect(s) => s.size,
            _ => unreachable!(),
        };
        assert_eq!(size, Size::new(180., 60.));
        assert_eq!(l.object().style.stroke.as_ref().unwrap().width, 10.);
    }
}
#[test]
fn cancellation_readonly_and_stale_external_edits_keep_history_and_capture_consistent() {
    let mut l = Lab::new(scene(display::Rect::new(120., 60.)));
    l.editor
        .transaction(|s| s.update_object(&"s".into(), |o| o.transform.tx = 10.))
        .unwrap();
    let redo = l.editor.scene().clone();
    assert!(l.editor.undo());
    let before = l.editor.scene().clone();
    let p = l.down(ResizeHandle::Right);
    ok(l.movement(p + Point::new(40., 0.)));
    l.host
        .replace(SceneResizePlugin::default(), &mut l.editor)
        .unwrap();
    assert!(!l.host.has_capture());
    assert_eq!(l.editor.scene(), &before);
    assert!(l.editor.redo());
    assert_eq!(l.editor.scene(), &redo);
    l.host
        .replace(
            SceneResizePlugin::default().with_builtin_resize(true),
            &mut l.editor,
        )
        .unwrap();
    let p = l.down(ResizeHandle::Right);
    ok(l.movement(p + Point::new(20., 0.)));
    l.editor
        .engine
        .transaction(|s| {
            s.update_object(&"s".into(), |o| {
                o.data = serde_json::json!({"external":true})
            })
        })
        .unwrap();
    let external = l.editor.scene().clone();
    ok(l.movement(p + Point::new(40., 0.)));
    assert_eq!(l.editor.scene(), &external);
    assert!(!l.host.has_capture());
    let g = BuiltinResizeGeometry::new(l.editor.scene(), &"s".into()).unwrap();
    let resize = g
        .resize(
            l.object().transform,
            Point::ZERO,
            Point::new(30., 0.),
            ResizeOptions::default(),
        )
        .unwrap();
    let target = l.editor.engine.handle(&"s".into()).unwrap();
    l.host.set_editable(false, &mut l.editor);
    assert!(
        l.host
            .execute(
                &mut l.editor,
                SceneCommand::SetBuiltinResize { target, resize }
            )
            .is_err()
    );
    assert_eq!(l.editor.scene(), &external);
}
struct Downgrade;
impl CanvasPlugin<SceneEditor, SceneCommand> for Downgrade {
    fn id(&self) -> &str {
        "downgrade-intrinsic"
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        _: &PluginContext<'_, SceneEditor>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::SetBuiltinResize { resize, .. } = command {
            *command = SceneCommand::SetTransform {
                id: "s".into(),
                transform: resize.transform,
            };
        }
        Ok(())
    }
}
#[test]
fn middleware_cannot_downgrade_intrinsic_patch_or_bypass_atomic_validation() {
    let mut l = Lab::new(scene(display::Rect::new(120., 60.)));
    l.host.install(Downgrade).unwrap();
    let before = l.editor.scene().clone();
    let p = l.down(ResizeHandle::Right);
    assert!(
        !l.movement(p + Point::new(30., 0.))
            .default
            .errors
            .is_empty()
    );
    assert_eq!(l.editor.scene(), &before);
    assert!(!l.host.has_capture());
    assert!(!l.editor.undo());
}

fn container_lab(style: FlowStyle, item: Option<FlowItem>) -> Lab {
    let mut scene = scene(display::Box::new(240., 100.));
    scene
        .insert(
            DisplayObject::new("child", display::Rect::new(60., 40.)),
            Some("s".into()),
            None,
        )
        .unwrap();
    let mut layout = FlowLayout::default();
    layout.containers.insert("s".into(), style);
    if let Some(item) = item {
        layout.items.insert("child".into(), item);
    }
    let (scene, _) = layout.arranged(&scene).unwrap();
    let mut l = Lab::new(scene);
    layout.install_reactive(&mut l.editor.engine).unwrap();
    l
}
#[test]
fn real_flow_content_owned_axis_rejects_atomically_while_free_axis_remains_editable() {
    let mut l = container_lab(
        FlowStyle {
            width_sizing: AxisSizing::content(),
            ..Default::default()
        },
        None,
    );
    let before = l.editor.scene().clone();
    let revision = l.editor.engine.revision();
    let p = l.down(ResizeHandle::Right);
    let report = l.movement(p + Point::new(40., 0.));
    assert!(!report.default.errors.is_empty());
    assert_eq!(l.editor.scene(), &before);
    assert_eq!(l.editor.engine.revision(), revision);
    assert!(!l.editor.undo());
    assert!(!l.host.has_capture());
    let p = l.down(ResizeHandle::Bottom);
    ok(l.movement(p + Point::new(0., 30.)));
    l.up(p);
    assert!(matches!(&l.object().shape,Shape::Box(s) if s.size==Size::new(60.,130.)));
    assert!(l.editor.undo());
    assert_eq!(l.editor.scene(), &before);
}
#[test]
fn fixed_container_resizes_and_reflows_content_in_the_same_undo() {
    let mut l = container_lab(
        FlowStyle {
            justify: JustifyContent::End,
            ..Default::default()
        },
        None,
    );
    let before = l.editor.scene().clone();
    let p = l.down(ResizeHandle::Right);
    ok(l.movement(p + Point::new(60., 0.)));
    l.up(p);
    assert_eq!(
        l.editor
            .scene()
            .get(&"child".into())
            .unwrap()
            .object
            .transform
            .tx,
        240.
    );
    assert!(l.editor.undo());
    assert_eq!(l.editor.scene(), &before);
    assert!(!l.editor.undo());
}
#[test]
fn parent_allocated_child_size_rejects_without_overwriting_flow() {
    let mut l = container_lab(
        FlowStyle::default(),
        Some(FlowItem {
            grow: 1.,
            ..Default::default()
        }),
    );
    let before = l.editor.scene().clone();
    let g = BuiltinResizeGeometry::new(l.editor.scene(), &"child".into()).unwrap();
    let original = l
        .editor
        .scene()
        .get(&"child".into())
        .unwrap()
        .object
        .transform;
    let resize = g
        .resize(
            original,
            Point::ZERO,
            Point::new(30., 0.),
            ResizeOptions::default(),
        )
        .unwrap();
    let target = l.editor.engine.handle(&"child".into()).unwrap();
    assert!(
        l.host
            .execute(
                &mut l.editor,
                SceneCommand::SetBuiltinResize { target, resize }
            )
            .is_err()
    );
    assert_eq!(l.editor.scene(), &before);
    assert!(!l.editor.undo());
}
struct Metrics;
impl TextMeasurer for Metrics {
    fn measure(&self, request: &TextMeasureRequest) -> Result<TextMetrics, String> {
        let lines = if request.wrap_width.is_some_and(|w| w < 100.) {
            2
        } else {
            1
        };
        Ok(TextMetrics {
            width: 80.,
            height: request.block_height(lines),
            line_height: request.line_height,
            lines,
        })
    }
}
#[test]
fn real_measured_text_ownership_rejects_box_overrides_and_preserves_font_mode_history() {
    for sizing in [TextSizing::Natural, TextSizing::AutoHeight] {
        let mut text = display::Text::new("measured", Size::new(120., 60.));
        text.wrap = sizing == TextSizing::AutoHeight;
        text.font_size = 20.;
        text.line_height = 1.25;
        let mut layout = FlowLayout::default();
        layout.texts.insert("s".into(), sizing);
        let measured = layout.with_text_measurer(Arc::new(Metrics));
        let (scene, _) = measured.arranged(&scene(text)).unwrap();
        let mut l = Lab::new(scene);
        measured.install_reactive(&mut l.editor.engine).unwrap();
        let before = l.editor.scene().clone();
        let p = l.down(ResizeHandle::Bottom);
        assert!(
            !l.movement(p + Point::new(0., 25.))
                .default
                .errors
                .is_empty()
        );
        assert_eq!(l.editor.scene(), &before);
        assert!(!l.editor.undo());
        l.host
            .replace(
                SceneResizePlugin::default().with_text_resize_mode(TextResizeMode::FontSize),
                &mut l.editor,
            )
            .unwrap();
        let p = l.down(ResizeHandle::Right);
        let r = l.movement(p + Point::new(30., 0.));
        if sizing == TextSizing::Natural {
            assert!(!r.default.errors.is_empty());
            assert_eq!(l.editor.scene(), &before);
        } else {
            ok(r);
            l.up(p);
            assert!(l.editor.undo());
            assert_eq!(l.editor.scene(), &before);
            assert!(!l.editor.undo());
        }
    }
}

#[test]
fn fractional_measured_font_disagreement_is_an_atomic_rejection() {
    let mut text = display::Text::new("measured", Size::new(120., 60.));
    text.font_size = 14.;
    text.line_height = 1.2;
    let mut layout = FlowLayout::default();
    layout.texts.insert("s".into(), TextSizing::AutoHeight);
    let measured = layout.with_text_measurer(Arc::new(Metrics));
    let (scene, _) = measured.arranged(&scene(text)).unwrap();
    let mut l = Lab::new(scene);
    measured.install_reactive(&mut l.editor.engine).unwrap();
    l.host
        .replace(
            SceneResizePlugin::default().with_text_resize_mode(TextResizeMode::FontSize),
            &mut l.editor,
        )
        .unwrap();
    let before = l.editor.scene().clone();
    let revision = l.editor.engine.revision();
    let p = l.down(ResizeHandle::Right);
    assert!(
        !l.movement(p + Point::new(30., 0.))
            .default
            .errors
            .is_empty()
    );
    assert_eq!(l.editor.scene(), &before);
    assert_eq!(l.editor.engine.revision(), revision);
    assert!(!l.host.has_capture());
    assert!(!l.editor.undo());
}

#[test]
fn intrinsic_no_motion_and_return_to_start_preserve_redo_without_compounding() {
    let mut l = Lab::new(scene(display::Rect::new(120., 60.)));
    l.editor
        .transaction(|s| s.update_object(&"s".into(), |o| o.transform.tx = 10.))
        .unwrap();
    let redo = l.editor.scene().clone();
    assert!(l.editor.undo());
    let before = l.editor.scene().clone();
    let p = l.down(ResizeHandle::Right);
    ok(l.movement(p));
    l.up(p);
    assert_eq!(l.editor.scene(), &before);
    let p = l.down(ResizeHandle::Right);
    ok(l.movement(p + Point::new(30., 0.)));
    ok(l.movement(p + Point::new(60., 0.)));
    assert!(matches!(&l.object().shape,Shape::Rect(s) if s.size==Size::new(180.,60.)));
    ok(l.movement(p));
    l.up(p);
    assert_eq!(l.editor.scene(), &before);
    assert!(!l.editor.undo());
    assert!(l.editor.redo());
    assert_eq!(l.editor.scene(), &redo);
}

#[test]
fn canvas_pixel_budget_failure_rolls_back_the_whole_preview() {
    let mut l = Lab::new(scene(display::Canvas::new(1024., 1024.)));
    let before = l.editor.scene().clone();
    let p = l.down(ResizeHandle::BottomRight);
    ok(l.movement(p + Point::new(100., 100.)));
    assert_ne!(l.editor.scene(), &before);
    ok(l.movement(p + Point::new(4096., 4096.)));
    assert_eq!(l.editor.scene(), &before);
    assert!(!l.host.has_capture());
    assert!(!l.editor.undo());
}

struct AddGroupChild;
impl gpui_flow_engine::LayoutProvider for AddGroupChild {
    fn derive(
        &self,
        _: &gpui_flow_engine::SceneEngine,
        scene: &Scene,
    ) -> Result<Scene, gpui_flow_engine::EngineError> {
        let mut next = scene.clone();
        if scene.get(&"child".into()).unwrap().object.transform.a != 1.
            && scene.get(&"extra".into()).is_none()
        {
            next.insert(
                DisplayObject::new("extra", display::Rect::new(10., 10.)),
                Some("s".into()),
                None,
            )?;
        }
        Ok(next)
    }
}
#[test]
fn derived_group_membership_change_rejects_the_whole_content_resize() {
    let mut scene = scene(display::Group::new());
    scene
        .insert(
            DisplayObject::new("child", display::Rect::new(120., 60.)),
            Some("s".into()),
            None,
        )
        .unwrap();
    let mut l = Lab::new(scene);
    l.editor
        .engine
        .install_layout_provider(AddGroupChild)
        .unwrap();
    let before = l.editor.scene().clone();
    let p = l.down(ResizeHandle::Right);
    assert!(
        !l.movement(p + Point::new(30., 0.))
            .default
            .errors
            .is_empty()
    );
    assert_eq!(l.editor.scene(), &before);
    assert!(!l.host.has_capture());
    assert!(!l.editor.undo());
}
