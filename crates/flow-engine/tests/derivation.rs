use gpui_flow_engine::*;
use gpui_flow_scene::{Affine, DisplayObject, Scene, display};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("a", display::Rect::new(20., 20.)),
            None,
            None,
        )
        .unwrap();
    SceneEngine::new(scene).unwrap()
}
struct Provider {
    mode: &'static str,
    enabled: Arc<AtomicBool>,
    calls: Arc<AtomicUsize>,
}
impl LayoutProvider for Provider {
    fn derive(&self, engine: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if !self.enabled.load(Ordering::SeqCst) {
            return Ok(candidate.clone());
        }
        let mut next = candidate.clone();
        match self.mode {
            "panic" => panic!("extension failed"),
            "error" => return Err(EngineError::Layout("extension unavailable".into())),
            "cycle" => next.update_object(&"a".into(), |o| {
                o.transform.tx = if o.transform.tx == 1. { 2. } else { 1. }
            })?,
            "name" => next.update_object(&"a".into(), |o| o.name = "illegal".into())?,
            "topology" => {
                next.remove(&"a".into())?;
            }
            "data" => next.data = json!("illegal"),
            "recursive" => engine.validate_properties_batch([(
                engine.handle(&"a".into()).unwrap(),
                PropertyKey::Transform,
                serde_json::to_value(Affine::translation(10., 0.)).unwrap(),
            )])?,
            "fixed" => next.update_object(&"a".into(), |o| o.transform.tx = 10.)?,
            _ => unreachable!(),
        }
        Ok(next)
    }
}
fn provider(mode: &'static str) -> (Provider, Arc<AtomicBool>, Arc<AtomicUsize>) {
    let enabled = Arc::new(AtomicBool::new(false));
    let calls = Arc::new(AtomicUsize::new(0));
    (
        Provider {
            mode,
            enabled: enabled.clone(),
            calls: calls.clone(),
        },
        enabled,
        calls,
    )
}
#[test]
fn install_requires_stable_scene_and_does_not_change_revision() {
    let mut e = engine();
    let before = e.scene().clone();
    let (p, enabled, _) = provider("fixed");
    enabled.store(true, Ordering::SeqCst);
    assert!(e.install_layout_provider(p).is_err());
    assert!(e.layout_provider().is_none());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), 0);
}
#[test]
fn paint_and_metadata_skip_provider_but_geometry_runs_before_one_notification() {
    let mut e = engine();
    let (p, enabled, calls) = provider("fixed");
    e.install_layout_provider(p).unwrap();
    let initial = calls.load(Ordering::SeqCst);
    let a = e.handle(&"a".into()).unwrap();
    for (key, value) in [
        (PropertyKey::Opacity, json!(0.5)),
        (PropertyKey::Name, json!("name")),
        (PropertyKey::Data, json!({"app":1})),
    ] {
        e.set_property(a, key, value).unwrap();
    }
    assert_eq!(calls.load(Ordering::SeqCst), initial);
    enabled.store(true, Ordering::SeqCst);
    let events = Arc::new(AtomicUsize::new(0));
    let seen = events.clone();
    e.on_change(None, move |changes| {
        assert_eq!(changes.origin, ChangeOrigin::Edit);
        seen.fetch_add(1, Ordering::SeqCst);
    })
    .unwrap();
    e.transaction(|s| s.update_object(&"a".into(), |o| o.transform.ty = 5.))
        .unwrap();
    assert_eq!(e.object(a).unwrap().transform.tx, 10.);
    assert_eq!(events.load(Ordering::SeqCst), 1);
    assert!(calls.load(Ordering::SeqCst) > initial);
}
#[test]
fn panic_nonconvergence_and_reentrant_preparation_are_atomic_and_guard_resets() {
    for mode in [
        "panic",
        "error",
        "cycle",
        "recursive",
        "name",
        "topology",
        "data",
    ] {
        let mut e = engine();
        let (p, enabled, _) = provider(mode);
        let token = e.install_layout_provider(p).unwrap();
        let old = e.scene().clone();
        let handle = e.handle(&"a".into()).unwrap();
        enabled.store(true, Ordering::SeqCst);
        let error = e
            .transaction(|s| s.update_object(&"a".into(), |o| o.transform.ty = 1.))
            .unwrap_err();
        assert!(matches!(error, EngineError::Layout(_)), "{mode}: {error}");
        assert_eq!(e.scene(), &old);
        assert_eq!(e.revision(), 0);
        assert_eq!(e.handle(&"a".into()).unwrap(), handle);
        assert_eq!(e.layout_provider(), Some(token));
        enabled.store(false, Ordering::SeqCst);
        e.transaction(|s| s.update_object(&"a".into(), |o| o.transform.ty = 2.))
            .unwrap();
    }
}
#[test]
fn foreign_and_replaced_tokens_cannot_uninstall_and_reset_drops_configuration() {
    let mut e = engine();
    let (p, _, _) = provider("fixed");
    let old = e.install_layout_provider(p).unwrap();
    let mut other = engine();
    let (p, _, _) = provider("fixed");
    let foreign = other.install_layout_provider(p).unwrap();
    assert!(!e.remove_layout_provider(foreign));
    let (p, _, _) = provider("fixed");
    let current = e.install_layout_provider(p).unwrap();
    assert!(!e.remove_layout_provider(old));
    assert_eq!(e.layout_provider(), Some(current));
    e.reset_scene(e.scene().clone()).unwrap();
    assert!(e.layout_provider().is_none());
    assert!(!e.remove_layout_provider(current));
}
#[test]
fn snapshot_restore_bypasses_provider_and_failed_replace_keeps_existing_provider() {
    let mut e = engine();
    let (p, enabled, _) = provider("fixed");
    let token = e.install_layout_provider(p).unwrap();
    let snapshot = e.scene().clone();
    enabled.store(true, Ordering::SeqCst);
    e.transaction(|s| s.update_object(&"a".into(), |o| o.transform.ty = 3.))
        .unwrap();
    e.replace_scene(snapshot.clone()).unwrap();
    assert_eq!(e.scene(), &snapshot);
    assert_eq!(e.layout_provider(), Some(token));
    let (bad, on, _) = provider("panic");
    on.store(true, Ordering::SeqCst);
    assert!(e.install_layout_provider(bad).is_err());
    assert_eq!(e.layout_provider(), Some(token));
}
#[test]
fn removing_provider_is_runtime_only_and_does_not_rewrite_last_geometry() {
    let mut e = engine();
    let (p, enabled, _) = provider("fixed");
    let token = e.install_layout_provider(p).unwrap();
    enabled.store(true, Ordering::SeqCst);
    e.transaction(|s| s.update_object(&"a".into(), |o| o.transform.ty = 3.))
        .unwrap();
    let before = e.scene().clone();
    let rev = e.revision();
    assert!(e.remove_layout_provider(token));
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), rev);
    e.transaction(|s| s.update_object(&"a".into(), |o| o.transform.tx = 90.))
        .unwrap();
    assert_eq!(
        e.object(e.handle(&"a".into()).unwrap())
            .unwrap()
            .transform
            .tx,
        90.
    );
}

struct ShapeMutator {
    enabled: Arc<AtomicBool>,
    change: fn(&mut gpui_flow_scene::Shape),
}
impl LayoutProvider for ShapeMutator {
    fn derive(&self, _: &SceneEngine, scene: &Scene) -> Result<Scene, EngineError> {
        let mut next = scene.clone();
        if self.enabled.load(Ordering::SeqCst) {
            next.update_object(&"a".into(), |o| (self.change)(&mut o.shape))?;
        }
        Ok(next)
    }
}
#[test]
fn provider_cannot_rewrite_text_content_or_image_resources_or_other_non_size_fields() {
    use gpui_flow_scene::{Shape, Size};
    type Mutation = fn(&mut Shape);
    let text = Shape::Text(display::Text::new("original", Size::new(30., 20.)));
    let image = Shape::Image(display::Image::new("original.png", Size::new(30., 20.)));
    let cases: Vec<(Shape, Mutation)> = vec![
        (text.clone(), |s| {
            if let Shape::Text(t) = s {
                t.content = "rewritten".into()
            }
        }),
        (text.clone(), |s| {
            if let Shape::Text(t) = s {
                t.font_family = "other".into()
            }
        }),
        (text.clone(), |s| {
            if let Shape::Text(t) = s {
                t.font_size = 18.
            }
        }),
        (text.clone(), |s| {
            if let Shape::Text(t) = s {
                t.line_height = 2.
            }
        }),
        (text.clone(), |s| {
            if let Shape::Text(t) = s {
                t.align = display::TextAlign::End
            }
        }),
        (text, |s| {
            if let Shape::Text(t) = s {
                t.wrap = false
            }
        }),
        (image.clone(), |s| {
            if let Shape::Image(i) = s {
                i.source = "injected.png".into()
            }
        }),
        (image.clone(), |s| {
            if let Shape::Image(i) = s {
                i.fit = display::ImageFit::Cover
            }
        }),
        (image, |s| {
            if let Shape::Image(i) = s {
                i.natural_size = Some(Size::new(50., 50.))
            }
        }),
        (Shape::Frame(display::Frame::new(30., 20.)), |s| {
            if let Shape::Frame(frame) = s {
                frame.clip_children = false
            }
        }),
        (
            Shape::Polygon(display::Polygon::new(vec![
                gpui_flow_scene::Point::new(0., 0.),
                gpui_flow_scene::Point::new(20., 0.),
                gpui_flow_scene::Point::new(0., 20.),
            ])),
            |s| {
                if let Shape::Polygon(p) = s {
                    p.points[0].x = 5.
                }
            },
        ),
        (
            Shape::Path(display::Path::new(vec![
                display::PathCommand::MoveTo {
                    to: gpui_flow_scene::Point::new(0., 0.),
                },
                display::PathCommand::LineTo {
                    to: gpui_flow_scene::Point::new(20., 20.),
                },
            ])),
            |s| {
                if let Shape::Path(p) = s {
                    p.commands.push(display::PathCommand::LineTo {
                        to: gpui_flow_scene::Point::new(40., 40.),
                    });
                }
            },
        ),
        (Shape::Rect(display::Rect::new(30., 20.)), |s| {
            if let Shape::Rect(r) = s {
                r.corner_radius = 8.
            }
        }),
    ];
    for (shape, change) in cases {
        let mut scene = Scene::new();
        scene
            .insert(DisplayObject::new("a", shape), None, None)
            .unwrap();
        let mut engine = SceneEngine::new(scene).unwrap();
        let enabled = Arc::new(AtomicBool::new(false));
        engine
            .install_layout_provider(ShapeMutator {
                enabled: enabled.clone(),
                change,
            })
            .unwrap();
        let before = engine.scene().clone();
        let old = engine.handle(&"a".into()).unwrap();
        enabled.store(true, Ordering::SeqCst);
        assert!(matches!(
            engine.transaction(|s| s.update_object(&"a".into(), |o| o.transform.tx = 1.)),
            Err(EngineError::Layout(_))
        ));
        assert_eq!(engine.scene(), &before);
        assert_eq!(engine.revision(), 0);
        assert_eq!(engine.handle(&"a".into()), Some(old));
    }
}

#[test]
fn text_and_image_size_changes_are_allowed_without_rewriting_content_or_resource() {
    use gpui_flow_scene::{Shape, Size};
    for shape in [
        Shape::Text(display::Text::new("original", Size::new(30., 20.))),
        Shape::Image(display::Image::new("original.png", Size::new(30., 20.))),
    ] {
        let mut scene = Scene::new();
        scene
            .insert(DisplayObject::new("a", shape.clone()), None, None)
            .unwrap();
        let mut e = SceneEngine::new(scene).unwrap();
        let enabled = Arc::new(AtomicBool::new(false));
        e.install_layout_provider(ShapeMutator {
            enabled: enabled.clone(),
            change: |shape| match shape {
                Shape::Text(text) => text.size = Size::new(50., 40.),
                Shape::Image(image) => image.size = Size::new(50., 40.),
                _ => {}
            },
        })
        .unwrap();
        enabled.store(true, Ordering::SeqCst);
        e.transaction(|s| s.update_object(&"a".into(), |o| o.transform.tx = 1.))
            .unwrap();
        let object = e.scene().get(&"a".into()).unwrap();
        assert_eq!(
            object.object.shape.local_bounds().unwrap().size,
            Size::new(50., 40.)
        );
        match &object.object.shape {
            Shape::Text(text) => assert_eq!(text.content, "original"),
            Shape::Image(image) => assert_eq!(image.source, "original.png"),
            _ => unreachable!(),
        }
    }
}

struct StrokeDependency {
    enabled: bool,
    calls: Arc<AtomicUsize>,
    declarations: Arc<AtomicUsize>,
}
impl LayoutProvider for StrokeDependency {
    fn uses_stroke_bounds(&self) -> bool {
        self.declarations.fetch_add(1, Ordering::SeqCst);
        self.enabled
    }
    fn derive(&self, _: &SceneEngine, scene: &Scene) -> Result<Scene, EngineError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(scene.clone())
    }
}

#[test]
fn stroke_dependencies_are_opt_in_and_queried_once_per_installation() {
    use gpui_flow_scene::{Color, Stroke, StrokeCap};
    for enabled in [false, true] {
        let mut e = engine();
        let calls = Arc::new(AtomicUsize::new(0));
        let declarations = Arc::new(AtomicUsize::new(0));
        e.install_layout_provider(StrokeDependency {
            enabled,
            calls: calls.clone(),
            declarations: declarations.clone(),
        })
        .unwrap();
        let initial = calls.load(Ordering::SeqCst);
        let a = e.handle(&"a".into()).unwrap();
        for (key, value) in [
            (
                PropertyKey::Stroke,
                serde_json::to_value(Stroke::new(Color::BLACK, 4.)).unwrap(),
            ),
            (
                PropertyKey::StrokeLayers,
                serde_json::to_value(vec![Stroke::new(Color::BLACK, 8.)]).unwrap(),
            ),
            (
                PropertyKey::Stroke,
                serde_json::to_value(Stroke::new(Color::BLACK, 4.).with_cap(StrokeCap::Square))
                    .unwrap(),
            ),
            (
                PropertyKey::Stroke,
                serde_json::to_value(Stroke::new(Color::TRANSPARENT, 4.)).unwrap(),
            ),
        ] {
            e.set_property(a, key, value).unwrap();
        }
        if enabled {
            // Public property writes may prepare both validation and commit.
            assert!(calls.load(Ordering::SeqCst) >= initial + 4);
        } else {
            assert_eq!(calls.load(Ordering::SeqCst), initial);
        }
        let after_strokes = calls.load(Ordering::SeqCst);
        for (key, value) in [
            (
                PropertyKey::Fill,
                serde_json::to_value(gpui_flow_scene::Paint::from(Color::WHITE)).unwrap(),
            ),
            (PropertyKey::Opacity, json!(0.5)),
            (PropertyKey::Visible, json!(false)),
            (PropertyKey::Name, json!("metadata")),
        ] {
            e.set_property(a, key, value).unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), after_strokes);
        assert_eq!(declarations.load(Ordering::SeqCst), 1);
    }
}

#[test]
fn default_provider_still_skips_stroke_edits() {
    let mut e = engine();
    let (provider, _, calls) = provider("fixed");
    e.install_layout_provider(provider).unwrap();
    let initial = calls.load(Ordering::SeqCst);
    e.set_property(
        e.handle(&"a".into()).unwrap(),
        PropertyKey::Stroke,
        serde_json::to_value(gpui_flow_scene::Stroke::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), initial);
}

#[test]
fn panicking_dependency_declaration_retains_previous_provider_and_document() {
    struct Panic;
    impl LayoutProvider for Panic {
        fn uses_stroke_bounds(&self) -> bool {
            panic!("invalid dependency declaration")
        }
        fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
            Ok(candidate.clone())
        }
    }
    let mut e = engine();
    let (provider, _, _) = provider("fixed");
    let token = e.install_layout_provider(provider).unwrap();
    let before = e.scene().clone();
    let revision = e.revision();
    assert!(
        e.install_layout_provider(Panic)
            .unwrap_err()
            .to_string()
            .contains("dependency declaration")
    );
    assert_eq!(e.layout_provider(), Some(token));
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
}

#[test]
fn fill_rule_is_a_reactive_stroke_box_input_only_for_opted_in_providers() {
    struct StrokeBoxes {
        calls: Arc<AtomicUsize>,
        stroke: bool,
    }
    impl LayoutProvider for StrokeBoxes {
        fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(candidate.clone())
        }
        fn uses_stroke_bounds(&self) -> bool {
            self.stroke
        }
    }
    for stroke in [false, true] {
        let mut e = engine();
        let calls = Arc::new(AtomicUsize::new(0));
        e.install_layout_provider(StrokeBoxes {
            calls: calls.clone(),
            stroke,
        })
        .unwrap();
        let initial = calls.load(Ordering::SeqCst);
        e.set_property(
            e.handle(&"a".into()).unwrap(),
            PropertyKey::FillRule,
            json!("even_odd"),
        )
        .unwrap();
        assert_eq!(calls.load(Ordering::SeqCst) > initial, stroke);
    }
}
