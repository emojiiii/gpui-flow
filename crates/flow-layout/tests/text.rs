//! Contract fixtures only: native glyph shaping is exercised by text_measure_probe.
use gpui_flow_animation::{AnimationController, AnimationSpec, Tween};
use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::*;
use gpui_flow_scene::{DisplayObject, Scene, Shape, Size, display};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

#[derive(Default)]
struct FixtureMetrics {
    mode: AtomicUsize,
}
impl TextMeasurer for FixtureMetrics {
    fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
        match self.mode.load(Ordering::SeqCst) {
            1 => return Err("no fonts".into()),
            2 => panic!("host failure"),
            3 => {
                return Ok(TextMetrics {
                    width: f32::NAN,
                    height: 0.,
                    line_height: r.line_height,
                    lines: 0,
                });
            }
            5 => {
                return Ok(TextMetrics {
                    width: 40.,
                    height: r.line_height * 2.,
                    line_height: r.line_height,
                    lines: 2,
                });
            }
            _ => {}
        }
        // Known fixture outcomes; this is deliberately not a text-width algorithm.
        let width = if r.content == "long fixture" {
            120.
        } else if r.content.is_empty() {
            0.
        } else {
            40.
        };
        let rows = if r.content.ends_with('\n') || r.wrap_width.is_some_and(|w| w < width) {
            2
        } else {
            1
        };
        Ok(TextMetrics {
            width: r.wrap_width.map_or(width, |w| w.min(width)),
            height: r.line_height * rows as f32,
            line_height: r.line_height,
            lines: rows,
        })
    }
    fn invalidate(&self) {
        self.mode.store(4, Ordering::SeqCst);
    }
}
fn fixture(sizing: TextSizing) -> (Scene, FlowLayout, Arc<FixtureMetrics>) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("frame", display::Frame::new(100., 200.)),
            None,
            None,
        )
        .unwrap();
    let mut text = display::Text::new("fixture", Size::new(80., 50.));
    text.font_size = 20.;
    text.line_height = 1.5;
    text.wrap = sizing == TextSizing::AutoHeight;
    scene
        .insert(DisplayObject::new("text", text), Some("frame".into()), None)
        .unwrap();
    let mut layout = FlowLayout::default();
    layout.containers.insert(
        "frame".into(),
        FlowStyle {
            direction: FlowDirection::Column,
            height_sizing: AxisSizing::content(),
            padding: Insets::all(10.),
            ..Default::default()
        },
    );
    layout.texts.insert("text".into(), sizing);
    (scene, layout, Arc::new(FixtureMetrics::default()))
}
fn text(scene: &Scene) -> &display::Text {
    let Shape::Text(t) = &scene.get(&"text".into()).unwrap().object.shape else {
        panic!()
    };
    t
}
fn edit(
    scene: &mut Scene,
    f: impl FnOnce(&mut display::Text),
) -> Result<(), gpui_flow_scene::SceneError> {
    scene.update_object(&"text".into(), |o| {
        let Shape::Text(t) = &mut o.shape else {
            panic!()
        };
        f(t);
    })
}
fn height(scene: &Scene) -> f32 {
    scene
        .get(&"frame".into())
        .unwrap()
        .object
        .shape
        .local_bounds()
        .unwrap()
        .size
        .height
}
#[test]
fn natural_and_empty_text_measure_both_axes_and_parent_height() {
    let (mut scene, layout, backend) = fixture(TextSizing::Natural);
    let measured = layout.with_text_measurer(backend);
    measured.apply_to_scene(&mut scene).unwrap();
    assert_eq!(text(&scene).size, Size::new(40., 30.));
    assert_eq!(height(&scene), 50.);
    edit(&mut scene, |t| t.content.clear()).unwrap();
    measured.apply_to_scene(&mut scene).unwrap();
    assert_eq!(text(&scene).size, Size::new(1., 30.));
    assert_eq!(height(&scene), 50.);
    assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
}
#[test]
fn parent_allocated_width_rewraps_text_then_measures_content_height() {
    let (mut scene, mut layout, backend) = fixture(TextSizing::AutoHeight);
    layout
        .containers
        .get_mut(&"frame".into())
        .unwrap()
        .align_items = AlignItems::Stretch;
    edit(&mut scene, |t| {
        t.content = "long fixture".into();
        t.size.width = 300.;
    })
    .unwrap();
    let measured = layout.with_text_measurer(backend);
    let report = measured.apply_to_scene(&mut scene).unwrap();
    assert_eq!(text(&scene).size, Size::new(80., 60.));
    assert_eq!(height(&scene), 80.);
    assert!(report.passes >= 2);
    assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
}
#[test]
fn trailing_newline_and_no_wrap_keep_explicit_line_boxes() {
    let (mut scene, layout, backend) = fixture(TextSizing::AutoHeight);
    let measured = layout.with_text_measurer(backend);
    edit(&mut scene, |t| t.content = "A\n".into()).unwrap();
    measured.apply_to_scene(&mut scene).unwrap();
    assert_eq!(text(&scene).size.height, 60.);
    edit(&mut scene, |t| {
        t.content = "long fixture".into();
        t.wrap = false;
    })
    .unwrap();
    measured.apply_to_scene(&mut scene).unwrap();
    assert_eq!(text(&scene).size, Size::new(80., 30.));
}
#[test]
fn missing_backend_and_invalid_text_bindings_fail_without_installing() {
    let (scene, mut layout, _) = fixture(TextSizing::AutoHeight);
    assert!(layout.arranged(&scene).is_err());
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    assert!(layout.install_reactive(&mut editor.engine).is_err());
    layout.texts.insert("frame".into(), TextSizing::Natural);
    assert!(layout.validate(&scene).is_err());
    layout.texts.remove(&"frame".into());
    layout.texts.insert("absent".into(), TextSizing::Natural);
    assert!(layout.validate(&scene).is_err());
    assert_eq!(editor.scene(), &scene);
}
#[test]
fn ambiguous_dimension_ownership_and_wrapped_natural_are_rejected() {
    for natural in [false, true] {
        let (scene, mut layout, _) = fixture(if natural {
            TextSizing::Natural
        } else {
            TextSizing::AutoHeight
        });
        layout.items.insert(
            "text".into(),
            FlowItem {
                basis: Some(50.),
                ..Default::default()
            },
        );
        assert!(layout.validate(&scene).is_err());
        layout.items.get_mut(&"text".into()).unwrap().in_flow = false;
        assert!(layout.validate(&scene).is_ok());
    }
    let (mut scene, mut layout, _) = fixture(TextSizing::Natural);
    layout
        .containers
        .get_mut(&"frame".into())
        .unwrap()
        .align_items = AlignItems::Stretch;
    assert!(layout.validate(&scene).is_err());
    layout.containers.clear();
    edit(&mut scene, |t| t.wrap = true).unwrap();
    assert!(layout.validate(&scene).is_err());
}
#[test]
fn normal_edits_derive_before_one_undo_and_skip_backend_during_undo_redo() {
    let (mut scene, layout, backend) = fixture(TextSizing::AutoHeight);
    let measured = layout.with_text_measurer(backend.clone());
    measured.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    measured.install_reactive(&mut editor.engine).unwrap();
    editor
        .transaction(|s| {
            edit(s, |t| {
                t.content = "long fixture".into();
                t.font_size = 30.;
                t.font_weight = 700;
            })
        })
        .unwrap();
    assert_eq!(text(editor.scene()).size.height, 90.);
    assert_eq!(height(editor.scene()), 110.);
    let after = editor.scene().clone();
    backend.mode.store(1, Ordering::SeqCst);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
}
#[test]
fn backend_error_panic_and_invalid_metrics_are_atomic_for_scene_and_history() {
    for mode in 1..=3 {
        let (mut scene, layout, backend) = fixture(TextSizing::AutoHeight);
        let measured = layout.with_text_measurer(backend.clone());
        measured.apply_to_scene(&mut scene).unwrap();
        let mut editor = SceneEditor::new(scene.clone()).unwrap();
        measured.install_reactive(&mut editor.engine).unwrap();
        editor
            .transaction(|s| s.update_object(&"text".into(), |o| o.name = "prior".into()))
            .unwrap();
        let before = editor.scene().clone();
        let revision = editor.engine.revision();
        backend.mode.store(mode, Ordering::SeqCst);
        assert!(
            editor
                .transaction(|s| edit(s, |t| t.content = "long fixture".into()))
                .is_err()
        );
        assert_eq!(editor.scene(), &before);
        assert_eq!(editor.engine.revision(), revision);
        assert!(editor.undo());
        assert_eq!(editor.scene(), &scene);
        assert!(!editor.undo());
    }
}
#[test]
fn invalidation_entry_point_actually_reflows_and_is_one_edit() {
    struct ChangedFont(AtomicUsize);
    impl TextMeasurer for ChangedFont {
        fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
            let lines = self.0.load(Ordering::SeqCst) + 1;
            Ok(TextMetrics {
                width: 30.,
                height: r.line_height * lines as f32,
                line_height: r.line_height,
                lines,
            })
        }
        fn invalidate(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let (mut scene, layout, _) = fixture(TextSizing::AutoHeight);
    let measured = layout.with_text_measurer(Arc::new(ChangedFont(AtomicUsize::new(0))));
    measured.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    measured.install_reactive(&mut editor.engine).unwrap();
    measured.refresh_text_metrics(&mut editor).unwrap();
    assert_eq!(height(editor.scene()), 80.);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
}
#[test]
fn failed_measurement_preserves_active_animation_and_success_finishes_one_boundary() {
    let (mut scene, layout, backend) = fixture(TextSizing::AutoHeight);
    let measured = layout.with_text_measurer(backend.clone());
    measured.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    measured.install_reactive(&mut editor.engine).unwrap();
    let token = editor.begin_playback().unwrap();
    let mut animation = AnimationController::new(&editor.engine);
    animation
        .animate(
            &editor.engine,
            editor.engine.handle(&"text".into()).unwrap(),
            AnimationSpec::new(1000, vec![Tween::opacity(0.2)]),
        )
        .unwrap();
    animation
        .tick_editor(&mut editor, token, Duration::from_millis(500))
        .unwrap();
    let playing = editor.scene().clone();
    backend.mode.store(1, Ordering::SeqCst);
    assert!(
        editor
            .transaction(|s| edit(s, |t| t.content = "long fixture".into()))
            .is_err()
    );
    assert!(editor.is_playback_active(token));
    assert_eq!(editor.scene(), &playing);
    backend.mode.store(0, Ordering::SeqCst);
    editor
        .transaction(|s| edit(s, |t| t.content = "long fixture".into()))
        .unwrap();
    let after = editor.scene().clone();
    assert!(!editor.is_playback_active(token));
    assert!(animation.cancel_editor(&mut editor, token).is_err());
    assert_eq!(editor.scene(), &after);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &playing);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
}
#[test]
fn serialized_layout_and_legacy_text_remain_compatible() {
    let (scene, layout, _) = fixture(TextSizing::AutoHeight);
    let json = serde_json::to_string(&layout).unwrap();
    assert_eq!(serde_json::from_str::<FlowLayout>(&json).unwrap(), layout);
    assert!(
        serde_json::from_str::<FlowLayout>("{}")
            .unwrap()
            .texts
            .is_empty()
    );
    let mut value = serde_json::to_value(text(&scene)).unwrap();
    value.as_object_mut().unwrap().remove("font_weight");
    assert_eq!(
        serde_json::from_value::<display::Text>(value)
            .unwrap()
            .font_weight,
        400
    );
}
#[test]
fn bad_requests_and_bad_weights_fail_without_mutation() {
    let (scene, layout, backend) = fixture(TextSizing::AutoHeight);
    let measured = layout.with_text_measurer(backend);
    for weight in [0, 1001] {
        let mut candidate = scene.clone();
        assert!(edit(&mut candidate, |t| t.font_weight = weight).is_err());
    }
    let mut candidate = scene.clone();
    edit(&mut candidate, |t| t.content = "x".repeat(65_537)).unwrap();
    let before = candidate.clone();
    assert!(measured.apply_to_scene(&mut candidate).is_err());
    assert_eq!(candidate, before);
}

#[test]
fn destroyed_text_binding_does_not_measure_reused_id_and_reset_drops_service() {
    let (mut scene, layout, backend) = fixture(TextSizing::AutoHeight);
    let measured = layout.with_text_measurer(backend.clone());
    measured.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    measured.install_reactive(&mut editor.engine).unwrap();
    editor
        .engine
        .destroy(editor.engine.handle(&"text".into()).unwrap())
        .unwrap();
    backend.mode.store(1, Ordering::SeqCst);
    editor
        .transaction(|s| {
            s.insert(
                DisplayObject::new(
                    "text",
                    display::Text::new("new instance", Size::new(70., 77.)),
                ),
                Some("frame".into()),
                None,
            )
        })
        .unwrap();
    assert_eq!(text(editor.scene()).size.height, 77.);
    editor.load_scene(editor.scene().clone()).unwrap();
    assert!(editor.engine.layout_provider().is_none());
}
#[test]
fn custom_schema_and_text_input_normalize_to_one_stable_commit() {
    use gpui_flow_engine::{
        CustomDefinition, DirtyFlags, ElementRegistry, PropertyDescriptor, PropertySchema,
        SceneEngine,
    };
    use serde_json::json;
    let (scene, config, backend) = fixture(TextSizing::AutoHeight);
    let measured = config.with_text_measurer(backend);
    let mut registry = ElementRegistry::default();
    registry
        .register(CustomDefinition::new(
            "disc",
            PropertySchema {
                properties: vec![PropertyDescriptor::number(
                    "radius",
                    5.,
                    1.,
                    40.,
                    DirtyFlags::GEOMETRY,
                )],
                allow_unknown: false,
            },
            |data: &serde_json::Value| {
                let shape = display::Ellipse::circle(data["radius"].as_f64().unwrap() as f32);
                Ok((shape.bounds(), shape.flatten(0.1)))
            },
        ))
        .unwrap();
    let mut engine = SceneEngine::with_registry(scene, registry).unwrap();
    engine
        .create(
            "disc",
            "disc",
            json!({}),
            Some(engine.handle(&"frame".into()).unwrap()),
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::from_engine(engine);
    measured.apply_to_editor(&mut editor).unwrap();
    measured.install_reactive(&mut editor.engine).unwrap();
    let before = editor.scene().clone();
    editor
        .transaction(|s| {
            edit(s, |t| t.content = "long fixture".into())?;
            s.update_object(&"disc".into(), |o| {
                let Shape::Custom(c) = &mut o.shape else {
                    panic!()
                };
                c.data["radius"] = json!(15.);
            })
        })
        .unwrap();
    assert_eq!(height(editor.scene()), 110.);
    assert!(
        measured
            .arranged(editor.scene())
            .unwrap()
            .1
            .changed
            .is_empty()
    );
    assert_eq!(
        editor.engine.prepare_scene(editor.scene().clone()).unwrap(),
        *editor.scene()
    );
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
}
#[test]
fn invalidation_failure_and_inconsistent_line_boxes_cannot_commit() {
    struct Bad;
    impl TextMeasurer for Bad {
        fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
            Ok(TextMetrics {
                width: 30.,
                height: 90.,
                line_height: r.line_height,
                lines: 1,
            })
        }
        fn invalidate(&self) {
            panic!("resource failure")
        }
    }
    let (scene, config, _) = fixture(TextSizing::AutoHeight);
    let measured = config.with_text_measurer(Arc::new(Bad));
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    assert!(measured.apply_to_editor(&mut editor).is_err());
    assert_eq!(editor.scene(), &scene);
    assert!(measured.refresh_text_metrics(&mut editor).is_err());
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
}
#[test]
fn model_measurement_is_independent_of_ancestor_zoom() {
    let (scene, config, backend) = fixture(TextSizing::AutoHeight);
    let measured = config.with_text_measurer(backend);
    let first = measured.arranged(&scene).unwrap().0;
    for scale in [0.5, 2., 1.] {
        let mut changed = first.clone();
        changed
            .update_object(&"frame".into(), |o| {
                o.transform = gpui_flow_scene::Affine::scale(scale, scale)
            })
            .unwrap();
        let result = measured.arranged(&changed).unwrap();
        assert_eq!(text(&result.0).size, text(&first).size);
        assert_eq!(height(&result.0), height(&first));
        assert!(result.1.changed.is_empty());
    }
}

#[test]
fn owned_text_content_session_remeasures_and_reflows_in_one_undo() {
    use gpui_flow_engine::PropertyKey;
    use serde_json::json;
    let (mut scene, layout, backend) = fixture(TextSizing::AutoHeight);
    let measured = layout.with_text_measurer(backend);
    measured.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    measured.install_reactive(&mut editor.engine).unwrap();
    let baseline = editor.scene().clone();
    let handle = editor.engine.handle(&"text".into()).unwrap();
    let token = editor.begin_edit_session().unwrap();
    for content in ["long fixture", "A\n"] {
        editor
            .set_edit_session_property(token, handle, PropertyKey::TextContent, json!(content))
            .unwrap();
        assert_eq!(text(editor.scene()).size.height, 60.);
        assert_eq!(height(editor.scene()), 80.);
        assert!(editor.is_edit_session_active(token));
    }
    let finished = editor.scene().clone();
    assert!(editor.finish_edit_session(token));
    assert!(editor.undo());
    assert_eq!(editor.scene(), &baseline);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert_eq!(editor.scene(), &finished);
}

#[test]
fn failed_text_measurement_preserves_owned_edit_and_last_accepted_content() {
    use gpui_flow_engine::PropertyKey;
    use serde_json::json;
    let (mut scene, layout, backend) = fixture(TextSizing::AutoHeight);
    let measured = layout.with_text_measurer(backend.clone());
    measured.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    measured.install_reactive(&mut editor.engine).unwrap();
    let handle = editor.engine.handle(&"text".into()).unwrap();
    let token = editor.begin_edit_session().unwrap();
    editor
        .set_edit_session_property(
            token,
            handle,
            PropertyKey::TextContent,
            json!("long fixture"),
        )
        .unwrap();
    let accepted = editor.scene().clone();
    backend.mode.store(1, Ordering::SeqCst);
    assert!(
        editor
            .set_edit_session_property(token, handle, PropertyKey::TextContent, json!("A\n"))
            .is_err()
    );
    assert_eq!(editor.scene(), &accepted);
    assert!(editor.is_edit_session_active(token));
    backend.mode.store(0, Ordering::SeqCst);
    assert!(editor.cancel_edit_session(token));
    assert_eq!(text(editor.scene()).content, "fixture");
    assert!(!editor.undo());
}

#[test]
fn horizontal_autowidth_keeps_height_handles_empty_text_and_rejects_extra_backend_lines() {
    let (mut scene, layout, backend) = fixture(TextSizing::AutoWidth);
    let measured = layout.with_text_measurer(backend.clone());
    for (content, width) in [
        ("fixture", 40.),
        ("long fixture", 120.),
        ("", 1.),
        ("fixture\n", 40.),
    ] {
        edit(&mut scene, |t| t.content = content.into()).unwrap();
        measured.apply_to_scene(&mut scene).unwrap();
        assert_eq!(text(&scene).size, Size::new(width, 50.));
        assert_eq!(height(&scene), 70.);
        assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
    }
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    let revision = editor.engine.revision();
    backend.mode.store(5, Ordering::SeqCst);
    assert!(
        measured
            .transaction_editor(&mut editor, |s| edit(s, |t| t.content = "fixture".into()))
            .unwrap_err()
            .to_string()
            .contains("backend metrics disagree")
    );
    assert_eq!(editor.scene(), &scene);
    assert_eq!(editor.engine.revision(), revision);
    assert!(!editor.undo());
}
