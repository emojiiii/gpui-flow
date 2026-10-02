use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{EngineError, LayoutProvider, SceneEngine};
use gpui_flow_layout::*;
use gpui_flow_scene::{Affine, DisplayObject, Point, Scene, Shape, Size, display};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn fixture() -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Box::new(220., 120.)),
            None,
            None,
        )
        .unwrap();
    for id in ["a", "b"] {
        scene
            .insert(
                DisplayObject::new(id, display::Rect::new(20., 10.)),
                Some("root".into()),
                None,
            )
            .unwrap();
    }
    let mut layout = FlowLayout::default();
    layout.containers.insert(
        "root".into(),
        FlowStyle {
            padding: Insets::all(10.),
            ..Default::default()
        },
    );
    (scene, layout)
}
fn size(scene: &Scene, id: &str) -> Size {
    scene
        .get(&id.into())
        .unwrap()
        .object
        .shape
        .local_bounds()
        .unwrap()
        .size
}
fn pos(scene: &Scene, id: &str) -> Point {
    let t = scene.get(&id.into()).unwrap().object.transform;
    Point::new(t.tx, t.ty)
}
fn resize(
    scene: &mut Scene,
    id: &str,
    width: f32,
    height: f32,
) -> Result<(), gpui_flow_scene::SceneError> {
    scene.update_object(&id.into(), |o| match &mut o.shape {
        Shape::Rect(s) => s.size = Size::new(width, height),
        Shape::Box(s) => s.size = Size::new(width, height),
        _ => panic!(),
    })
}
#[test]
fn percentages_use_fixed_content_axes_not_whole_box_and_reflow() {
    let (mut scene, mut layout) = fixture();
    layout.items.insert(
        "a".into(),
        FlowItem {
            width_percent: Some(0.5),
            height_percent: Some(0.25),
            ..Default::default()
        },
    );
    layout.items.insert(
        "b".into(),
        FlowItem {
            width_percent: Some(0.25),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "a"), Size::new(100., 25.));
    assert_eq!(pos(&scene, "b"), Point::new(110., 10.));
    resize(&mut scene, "root", 420., 220.).unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "a"), Size::new(200., 50.));
    assert_eq!(pos(&scene, "b"), Point::new(210., 10.));
    assert!(
        layout
            .apply_to_scene(&mut scene)
            .unwrap()
            .changed
            .is_empty()
    );
}
#[test]
fn percentage_slots_respect_scale_clamps_reverse_wrap_and_exclusion() {
    let (mut scene, mut layout) = fixture();
    layout.containers.get_mut(&"root".into()).unwrap().direction = FlowDirection::RowReverse;
    layout.items.insert(
        "a".into(),
        FlowItem {
            width_percent: Some(2.),
            max_width: Some(120.),
            height_percent: Some(0.5),
            ..Default::default()
        },
    );
    scene
        .update_object(&"a".into(), |o| o.transform = Affine::scale(2., 2.))
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "a"), Size::new(60., 25.));
    assert_eq!(pos(&scene, "a"), Point::new(30., 10.));
    layout.items.get_mut(&"a".into()).unwrap().in_flow = false;
    let before = scene.get(&"a".into()).unwrap().object.clone();
    resize(&mut scene, "root", 400., 200.).unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(&scene.get(&"a".into()).unwrap().object, &before);
    layout.items.get_mut(&"a".into()).unwrap().in_flow = true;
    layout.containers.get_mut(&"root".into()).unwrap().wrap = true;
    resize(&mut scene, "root", 130., 120.).unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(pos(&scene, "a").y, 20.);
}
#[test]
fn percent_ownership_and_indefinite_parent_reject_without_mutation() {
    let (scene, layout) = fixture();
    for mode in 0..8 {
        let mut scene = scene.clone();
        let before = scene.clone();
        let mut layout = layout.clone();
        let mut item = FlowItem {
            width_percent: Some(0.5),
            ..Default::default()
        };
        match mode {
            0 => {
                layout
                    .containers
                    .get_mut(&"root".into())
                    .unwrap()
                    .width_sizing = AxisSizing::content()
            }
            1 => item.grow = 1.,
            2 => item.basis = Some(40.),
            3 => item.lock_ratio = true,
            4 => {
                item.height_percent = Some(0.5);
                item.align_self = Some(AlignItems::Stretch);
            }
            5 => item.width_percent = Some(f32::NAN),
            6 => item.width_percent = Some(0.),
            7 => item.width_percent = Some(f32::MAX),
            _ => unreachable!(),
        }
        layout.items.insert("a".into(), item);
        assert!(layout.apply_to_scene(&mut scene).is_err(), "mode {mode}");
        assert_eq!(scene, before);
    }
}
#[test]
fn percent_child_content_size_conflict_is_atomic() {
    let (mut scene, mut layout) = fixture();
    scene
        .insert(
            DisplayObject::new("nested", display::Box::new(60., 60.)),
            Some("root".into()),
            None,
        )
        .unwrap();
    layout.containers.insert(
        "nested".into(),
        FlowStyle {
            width_sizing: AxisSizing::content(),
            ..Default::default()
        },
    );
    layout.items.insert(
        "nested".into(),
        FlowItem {
            width_percent: Some(0.5),
            ..Default::default()
        },
    );
    let before = scene.clone();
    assert!(
        layout
            .apply_to_scene(&mut scene)
            .unwrap_err()
            .to_string()
            .contains("also allocated")
    );
    assert_eq!(scene, before);
}
#[test]
fn shrink_content_matches_pinned_unsign_and_cancel_preserves_outer_geometry() {
    let (mut scene, mut layout) = fixture();
    let original = size(&scene, "root");
    layout
        .containers
        .get_mut(&"root".into())
        .unwrap()
        .content_bounds_policy = ContentBoundsPolicy::LeaferShrink;
    let content = layout.content_bounds(&scene, &"root".into()).unwrap();
    assert_eq!(content.origin, Point::new(10., 10.));
    assert_eq!(content.size, Size::new(200., 100.));
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), original);
    assert_eq!(size(&scene, "a"), Size::new(20., 10.));
    layout.containers.get_mut(&"root".into()).unwrap().padding = Insets {
        left: 180.,
        right: 100.,
        top: 90.,
        bottom: 60.,
    };
    let content = layout.content_bounds(&scene, &"root".into()).unwrap();
    assert_eq!(content.origin, Point::new(120., 60.));
    assert_eq!(content.size, Size::new(60., 30.));
    layout.items.insert(
        "a".into(),
        FlowItem {
            width_percent: Some(0.5),
            height_percent: Some(0.5),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "a"), Size::new(30., 15.));
    assert_eq!(pos(&scene, "a"), content.origin);
    layout.containers.get_mut(&"root".into()).unwrap().padding = Insets::default();
    assert_eq!(
        layout.content_bounds(&scene, &"root".into()).unwrap().size,
        original
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(pos(&scene, "a"), Point::new(0., 0.));
}
#[test]
fn legacy_padding_and_json_stay_unchanged() {
    let (scene, mut layout) = fixture();
    layout.containers.get_mut(&"root".into()).unwrap().padding = Insets::all(500.);
    let c = layout.content_bounds(&scene, &"root".into()).unwrap();
    assert_eq!(c.origin, Point::new(500., 500.));
    assert_eq!(c.size, Size::new(0., 0.));
    let style: FlowStyle = serde_json::from_str("{}").unwrap();
    let item: FlowItem = serde_json::from_str("{}").unwrap();
    assert_eq!(style, FlowStyle::default());
    assert_eq!(item, FlowItem::default());
    let json = serde_json::to_value(&style).unwrap();
    assert!(json.get("content_bounds_policy").is_none());
    let json = serde_json::to_value(&item).unwrap();
    assert!(json.get("width_percent").is_none());
    assert!(json.get("height_percent").is_none());
}
#[derive(Default)]
struct Metrics {
    failure: AtomicUsize,
    calls: Mutex<Vec<String>>,
}
impl TextMeasurer for Metrics {
    fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
        self.calls.lock().unwrap().push(r.content.clone());
        let width = if r.content.ends_with("long") {
            80.
        } else {
            40.
        };
        let lines = if r.wrap_width.is_some_and(|w| w < width) {
            2
        } else {
            1
        };
        Ok(TextMetrics {
            width: r.wrap_width.map_or(width, |w| w.min(width)),
            height: r.block_height(lines),
            line_height: r.line_height,
            lines,
        })
    }
    fn first_baseline(&self, r: &TextMeasureRequest) -> Result<f32, String> {
        match self.failure.load(Ordering::SeqCst) {
            1 => Err("missing font".into()),
            2 => panic!("baseline panic"),
            3 => Ok(f32::NAN),
            4 => Ok(r.line_height + 1.),
            _ => Ok(if r.font_size == 20. { 15. } else { 12. }),
        }
    }
}
fn texts() -> (Scene, FlowLayout, Arc<Metrics>) {
    let (mut scene, mut layout) = fixture();
    for (id, font_size) in [("a", 20.), ("b", 30.)] {
        scene
            .update_object(&id.into(), |o| {
                let mut t = display::Text::new(id, Size::new(50., 40.));
                t.font_size = font_size;
                t.line_height = 1.;
                t.wrap = false;
                o.shape = Shape::Text(t);
            })
            .unwrap();
        layout.texts.insert(id.into(), TextSizing::Natural);
    }
    let style = layout.containers.get_mut(&"root".into()).unwrap();
    style.align_items = AlignItems::Baseline;
    style.height_sizing = AxisSizing::content();
    style.padding = Insets::all(5.);
    (scene, layout, Arc::new(Metrics::default()))
}
#[test]
fn first_text_baseline_grows_line_to_max_ascent_plus_descent() {
    let (mut scene, layout, metrics) = texts();
    let measured = layout.with_text_measurer(metrics);
    measured.apply_to_scene(&mut scene).unwrap();
    assert_eq!(pos(&scene, "a"), Point::new(5., 5.));
    assert_eq!(pos(&scene, "b"), Point::new(45., 8.));
    assert_eq!(size(&scene, "root").height, 43.);
    assert_eq!(pos(&scene, "a").y + 15., pos(&scene, "b").y + 12.);
    assert!(
        measured
            .apply_to_scene(&mut scene)
            .unwrap()
            .changed
            .is_empty()
    );
}
#[test]
fn baseline_respects_padding_scale_and_reverse() {
    let (mut scene, mut layout, metrics) = texts();
    layout.containers.get_mut(&"root".into()).unwrap().direction = FlowDirection::RowReverse;
    scene
        .update_object(&"b".into(), |o| {
            o.transform = Affine::scale(2., 2.);
            if let Shape::Text(t) = &mut o.shape {
                t.padding = [3., 0., 2., 0.];
            }
        })
        .unwrap();
    layout
        .with_text_measurer(metrics)
        .apply_to_scene(&mut scene)
        .unwrap();
    assert_eq!(pos(&scene, "b"), Point::new(5., 5.));
    assert_eq!(pos(&scene, "a"), Point::new(85., 20.));
    assert_eq!(size(&scene, "root").height, 80.);
}
#[test]
fn wrapped_baselines_have_independent_origins() {
    let (mut scene, mut layout, metrics) = texts();
    resize(&mut scene, "root", 70., 120.).unwrap();
    let style = layout.containers.get_mut(&"root".into()).unwrap();
    style.wrap = true;
    style.line_gap = 7.;
    layout
        .with_text_measurer(metrics)
        .apply_to_scene(&mut scene)
        .unwrap();
    assert_eq!(pos(&scene, "a").y, 5.);
    assert_eq!(pos(&scene, "b").y, 32.);
    assert_eq!(size(&scene, "root").height, 67.);
}
#[test]
fn baseline_failure_missing_backend_and_unsupported_alignment_are_atomic() {
    let (scene, layout, metrics) = texts();
    assert!(layout.arranged(&scene).is_err());
    for failure in 1..=4 {
        metrics.failure.store(failure, Ordering::SeqCst);
        let mut editor = SceneEditor::new(scene.clone()).unwrap();
        assert!(
            layout
                .clone()
                .with_text_measurer(metrics.clone())
                .apply_to_editor(&mut editor)
                .is_err()
        );
        assert_eq!(editor.scene(), &scene);
        assert!(!editor.undo());
    }
    metrics.failure.store(0, Ordering::SeqCst);
    let mut invalid = layout.clone();
    invalid
        .containers
        .get_mut(&"root".into())
        .unwrap()
        .direction = FlowDirection::Column;
    assert!(
        invalid
            .with_text_measurer(metrics.clone())
            .arranged(&scene)
            .is_err()
    );
    let mut invalid = layout.clone();
    invalid.texts.remove(&"a".into());
    assert!(
        invalid
            .with_text_measurer(metrics)
            .arranged(&scene)
            .is_err()
    );
}
#[test]
fn legacy_measurer_fails_only_for_opted_in_baseline() {
    struct Legacy;
    impl TextMeasurer for Legacy {
        fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
            Ok(TextMetrics {
                width: 40.,
                height: r.line_height,
                line_height: r.line_height,
                lines: 1,
            })
        }
    }
    let (scene, mut layout, _) = texts();
    assert!(
        layout
            .clone()
            .with_text_measurer(Arc::new(Legacy))
            .arranged(&scene)
            .unwrap_err()
            .to_string()
            .contains("does not provide")
    );
    layout
        .containers
        .get_mut(&"root".into())
        .unwrap()
        .align_items = AlignItems::Start;
    assert!(
        layout
            .with_text_measurer(Arc::new(Legacy))
            .arranged(&scene)
            .is_ok()
    );
}
#[test]
fn percentage_text_width_reflows_autoheight_and_rejects_natural_or_height_ownership() {
    let (mut scene, mut layout, metrics) = texts();
    layout
        .containers
        .get_mut(&"root".into())
        .unwrap()
        .align_items = AlignItems::Start;
    layout.texts.insert("a".into(), TextSizing::AutoHeight);
    scene
        .update_object(&"a".into(), |o| {
            if let Shape::Text(t) = &mut o.shape {
                t.content = "a long".into();
                t.wrap = true;
            }
        })
        .unwrap();
    layout.items.insert(
        "a".into(),
        FlowItem {
            width_percent: Some(0.25),
            ..Default::default()
        },
    );
    let measured = layout.clone().with_text_measurer(metrics.clone());
    measured.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "a"), Size::new(52.5, 40.));
    resize(&mut scene, "root", 410., 120.).unwrap();
    measured.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "a"), Size::new(100., 20.));
    layout.items.get_mut(&"a".into()).unwrap().height_percent = Some(0.5);
    assert!(layout.with_text_measurer(metrics).arranged(&scene).is_err());
}
#[test]
fn incremental_skips_unrelated_text_subtree_and_matches_full_reflow() {
    let (mut scene, mut layout, metrics) = texts();
    scene
        .insert(
            DisplayObject::new("other", display::Box::new(200., 100.)),
            None,
            None,
        )
        .unwrap();
    let mut other = display::Text::new("unrelated", Size::new(80., 40.));
    other.wrap = false;
    scene
        .insert(
            DisplayObject::new("other_text", other),
            Some("other".into()),
            None,
        )
        .unwrap();
    layout
        .containers
        .insert("other".into(), FlowStyle::default());
    layout
        .texts
        .insert("other_text".into(), TextSizing::Natural);
    let measured = layout.with_text_measurer(metrics.clone());
    measured.apply_to_scene(&mut scene).unwrap();
    let mut next = scene.clone();
    next.update_object(&"a".into(), |o| {
        if let Shape::Text(t) = &mut o.shape {
            t.content = "a long".into();
        }
    })
    .unwrap();
    metrics.calls.lock().unwrap().clear();
    let (incremental, report) = measured.arranged_dirty(&scene, &next).unwrap();
    assert_eq!(report.containers, 1);
    assert!(
        !metrics
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|s| s == "unrelated")
    );
    assert_eq!(incremental, measured.arranged(&next).unwrap().0);
    let (_, report) = measured.arranged_dirty(&incremental, &incremental).unwrap();
    assert_eq!(report.passes, 0);
}
#[test]
fn seeded_nested_percentage_dependency_reflow_matches_full_and_is_idempotent() {
    let mut seed = 0x123456789u64;
    for _ in 0..32 {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        let width = 300. + ((seed >> 32) % 100) as f32;
        let (mut scene, mut layout) = fixture();
        resize(&mut scene, "root", width, 160.).unwrap();
        for (id, parent) in [("branch", "root"), ("nested", "branch")] {
            scene
                .insert(
                    DisplayObject::new(id, display::Box::new(80., 80.)),
                    Some(parent.into()),
                    None,
                )
                .unwrap();
            layout.containers.insert(
                id.into(),
                FlowStyle {
                    height_sizing: AxisSizing::content(),
                    padding: Insets::all(2.),
                    ..Default::default()
                },
            );
            layout.items.insert(
                id.into(),
                FlowItem {
                    width_percent: Some(0.5),
                    ..Default::default()
                },
            );
        }
        scene
            .insert(
                DisplayObject::new("leaf", display::Rect::new(30., 17.)),
                Some("nested".into()),
                None,
            )
            .unwrap();
        layout.items.insert(
            "leaf".into(),
            FlowItem {
                width_percent: Some(0.75),
                ..Default::default()
            },
        );
        layout.apply_to_scene(&mut scene).unwrap();
        let mut next = scene.clone();
        resize(&mut next, "root", width + 100., 160.).unwrap();
        let (dirty, report) = layout.arranged_dirty(&scene, &next).unwrap();
        assert_eq!(report.containers, 3);
        assert_eq!(dirty, layout.arranged(&next).unwrap().0);
        assert!(layout.arranged(&dirty).unwrap().1.changed.is_empty());
        let mut leaf_change = dirty.clone();
        resize(&mut leaf_change, "leaf", size(&dirty, "leaf").width, 31.).unwrap();
        assert_eq!(
            layout.arranged_dirty(&dirty, &leaf_change).unwrap().0,
            layout.arranged(&leaf_change).unwrap().0
        );
    }
}
#[test]
fn queued_edits_coalesce_one_atomic_undo_and_noop_flush() {
    let (mut scene, layout) = fixture();
    layout.apply_to_scene(&mut scene).unwrap();
    let before = scene.clone();
    let mut editor = SceneEditor::new(scene).unwrap();
    let mut queue = FlowFrameQueue::new(layout);
    assert!(
        queue
            .enqueue(&editor, |s| resize(s, "a", 35., 10.))
            .unwrap()
    );
    assert!(
        !queue
            .enqueue(&editor, |s| resize(s, "a", 70., 10.))
            .unwrap()
    );
    assert_eq!(editor.scene(), &before);
    assert_eq!(queue.pending_edits(), 2);
    let report = queue.flush(&mut editor).unwrap().unwrap();
    assert_eq!(report.containers, 1);
    assert_eq!(pos(editor.scene(), "b").x, 80.);
    assert_eq!(size(editor.scene(), "a").width, 70.);
    assert!(queue.flush(&mut editor).unwrap().is_none());
    assert!(!queue.is_pending());
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(!editor.undo());
    assert!(editor.redo());
}
#[test]
fn queue_budget_staleness_panics_and_cancel_do_not_leak_changes() {
    let (scene, layout) = fixture();
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    let mut queue = FlowFrameQueue::new(layout);
    assert!(queue.enqueue(&editor, |_| panic!("bad input")).is_err());
    assert!(!queue.is_pending());
    for _ in 0..MAX_FLOW_FRAME_EDITS {
        queue.enqueue(&editor, |_| Ok(())).unwrap();
    }
    assert!(queue.enqueue(&editor, |_| Ok(())).is_err());
    assert_eq!(queue.pending_edits(), MAX_FLOW_FRAME_EDITS);
    editor.transaction(|s| resize(s, "a", 99., 10.)).unwrap();
    let current = editor.scene().clone();
    assert!(queue.flush(&mut editor).is_err());
    assert_eq!(editor.scene(), &current);
    assert!(queue.cancel());
    assert!(!queue.cancel());
    assert!(queue.enqueue(&editor, |_| Ok(())).unwrap());
    queue.flush(&mut editor).unwrap();
}
#[test]
fn queued_invalid_percent_flush_preserves_pending_and_document_history() {
    let (scene, mut layout) = fixture();
    layout.items.insert(
        "a".into(),
        FlowItem {
            width_percent: Some(0.5),
            ..Default::default()
        },
    );
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    let mut queue = FlowFrameQueue::new(layout);
    queue
        .enqueue(&editor, |s| resize(s, "root", 20., 120.))
        .unwrap();
    assert!(queue.flush(&mut editor).is_err());
    assert!(queue.is_pending());
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
    queue
        .enqueue(&editor, |s| resize(s, "root", 220., 120.))
        .unwrap();
    queue.flush(&mut editor).unwrap();
    assert_eq!(size(editor.scene(), "a").width, 100.);
}
#[test]
fn queued_competing_provider_rejects_final_derived_ownership_atomically() {
    struct Conflict;
    impl LayoutProvider for Conflict {
        fn derive(&self, _: &SceneEngine, c: &Scene) -> Result<Scene, EngineError> {
            let mut n = c.clone();
            if size(c, "a").width == 70. {
                n.update_object(&"b".into(), |o| o.transform.tx = 999.)?;
            }
            Ok(n)
        }
    }
    let (mut scene, layout) = fixture();
    layout.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    editor.engine.install_layout_provider(Conflict).unwrap();
    let mut queue = FlowFrameQueue::new(layout);
    queue
        .enqueue(&editor, |s| resize(s, "a", 70., 10.))
        .unwrap();
    assert!(
        queue
            .flush(&mut editor)
            .unwrap_err()
            .to_string()
            .contains("conflicts")
    );
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
}
#[test]
fn reactive_percentage_resize_and_history_share_one_commit() {
    let (mut scene, mut layout) = fixture();
    layout.items.insert(
        "a".into(),
        FlowItem {
            width_percent: Some(0.5),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    layout.install_reactive(&mut editor.engine).unwrap();
    editor
        .transaction(|s| resize(s, "root", 420., 120.))
        .unwrap();
    assert_eq!(size(editor.scene(), "a").width, 200.);
    assert_eq!(pos(editor.scene(), "b").x, 210.);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
    assert!(editor.redo());
}

#[test]
fn percentage_column_uses_physical_axes_and_serializes_additively() {
    let (mut scene, mut layout) = fixture();
    layout.containers.get_mut(&"root".into()).unwrap().direction = FlowDirection::Column;
    layout.items.insert(
        "a".into(),
        FlowItem {
            width_percent: Some(0.25),
            height_percent: Some(0.5),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "a"), Size::new(50., 50.));
    assert_eq!(pos(&scene, "b"), Point::new(10., 60.));
    let document = FlowDocument { scene, layout };
    let json = document.to_json().unwrap();
    assert!(json.contains("width_percent"));
    assert_eq!(FlowDocument::from_json(&json).unwrap(), document);
}
#[test]
fn malformed_metrics_and_fractional_percent_rotation_fail_atomically() {
    let (scene, mut layout) = fixture();
    layout.items.insert(
        "a".into(),
        FlowItem {
            width_percent: Some(0.1),
            ..Default::default()
        },
    );
    let mut rotated = scene.clone();
    rotated
        .update_object(&"a".into(), |o| {
            o.transform = Affine {
                a: 0.,
                b: 1.,
                c: -1.,
                d: 0.,
                tx: 0.,
                ty: 0.,
            }
        })
        .unwrap();
    let before = rotated.clone();
    assert!(layout.apply_to_scene(&mut rotated).is_err());
    assert_eq!(before, rotated);
    struct Bad;
    impl TextMeasurer for Bad {
        fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
            Ok(TextMetrics {
                width: 20.,
                height: r.line_height + 10.,
                line_height: r.line_height,
                lines: 1,
            })
        }
        fn first_baseline(&self, _: &TextMeasureRequest) -> Result<f32, String> {
            Ok(15.)
        }
    }
    let (scene, layout, _) = texts();
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    assert!(
        layout
            .with_text_measurer(Arc::new(Bad))
            .apply_to_editor(&mut editor)
            .is_err()
    );
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
}
#[test]
fn queue_rejects_foreign_engine_document_reset_and_provider_replacement() {
    struct Noop;
    impl LayoutProvider for Noop {
        fn derive(&self, _: &SceneEngine, c: &Scene) -> Result<Scene, EngineError> {
            Ok(c.clone())
        }
    }
    for mode in 0..3 {
        let (scene, layout) = fixture();
        let mut editor = SceneEditor::new(scene.clone()).unwrap();
        let mut queue = FlowFrameQueue::new(layout);
        queue
            .enqueue(&editor, |s| resize(s, "a", 70., 10.))
            .unwrap();
        match mode {
            0 => editor = SceneEditor::new(scene.clone()).unwrap(),
            1 => editor.load_scene(scene.clone()).unwrap(),
            2 => {
                editor.engine.install_layout_provider(Noop).unwrap();
            }
            _ => unreachable!(),
        }
        assert!(
            queue
                .flush(&mut editor)
                .unwrap_err()
                .to_string()
                .contains("stale")
        );
        assert_eq!(editor.scene(), &scene);
        assert!(!editor.undo());
    }
}
#[test]
fn failed_queued_edit_retains_earlier_input_and_cycle_is_atomic() {
    let (scene, layout) = fixture();
    let mut editor = SceneEditor::new(scene).unwrap();
    let mut queue = FlowFrameQueue::new(layout);
    queue
        .enqueue(&editor, |s| resize(s, "a", 33., 10.))
        .unwrap();
    assert!(
        queue
            .enqueue(&editor, |s| {
                resize(s, "a", 99., 10.)?;
                Err(gpui_flow_scene::SceneError::InvalidGeometry(
                    "reject".into(),
                ))
            })
            .is_err()
    );
    assert_eq!(queue.pending_edits(), 1);
    assert!(
        queue
            .enqueue(&editor, |s| s.reparent(
                &"root".into(),
                Some("a".into()),
                None,
                false,
            ))
            .is_err()
    );
    assert_eq!(queue.pending_edits(), 1);
    queue.flush(&mut editor).unwrap();
    assert_eq!(size(editor.scene(), "a").width, 33.);
}
#[test]
fn queue_retry_after_baseline_recovery_keeps_single_history_boundary() {
    let (mut scene, layout, metrics) = texts();
    layout
        .clone()
        .with_text_measurer(metrics.clone())
        .apply_to_scene(&mut scene)
        .unwrap();
    let before = scene.clone();
    let mut editor = SceneEditor::new(scene).unwrap();
    let mut queue = FlowFrameQueue::measured(layout.with_text_measurer(metrics.clone()));
    queue
        .enqueue(&editor, |s| {
            s.update_object(&"a".into(), |o| {
                if let Shape::Text(t) = &mut o.shape {
                    t.content = "a long".into()
                }
            })
        })
        .unwrap();
    metrics.failure.store(1, Ordering::SeqCst);
    assert!(queue.flush(&mut editor).is_err());
    assert_eq!(editor.scene(), &before);
    assert!(queue.is_pending());
    metrics.failure.store(0, Ordering::SeqCst);
    assert!(queue.flush(&mut editor).unwrap().is_some());
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(!editor.undo());
}
#[test]
fn settled_queue_skips_paint_only_work_until_explicit_font_invalidation() {
    let (mut scene, layout, metrics) = texts();
    let measured = layout.with_text_measurer(metrics.clone());
    measured.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    let mut queue = FlowFrameQueue::measured(measured);
    queue.enqueue(&editor, |_| Ok(())).unwrap();
    queue.flush(&mut editor).unwrap();
    metrics.calls.lock().unwrap().clear();
    queue
        .enqueue(&editor, |s| {
            s.update_object(&"a".into(), |o| o.style.opacity = 0.5)
        })
        .unwrap();
    let report = queue.flush(&mut editor).unwrap().unwrap();
    assert_eq!(report.containers, 0);
    assert_eq!(report.passes, 0);
    assert!(metrics.calls.lock().unwrap().is_empty());
    queue.invalidate();
    queue.enqueue(&editor, |_| Ok(())).unwrap();
    assert!(queue.flush(&mut editor).unwrap().unwrap().passes > 0);
    assert!(!metrics.calls.lock().unwrap().is_empty());
}
#[test]
fn reactive_dirty_text_branch_skips_unrelated_measurer_and_undo_is_exact() {
    let (mut scene, mut layout, metrics) = texts();
    scene
        .insert(
            DisplayObject::new("other", display::Box::new(200., 100.)),
            None,
            None,
        )
        .unwrap();
    let mut other = display::Text::new("unrelated", Size::new(80., 40.));
    other.wrap = false;
    scene
        .insert(
            DisplayObject::new("other_text", other),
            Some("other".into()),
            None,
        )
        .unwrap();
    layout
        .containers
        .insert("other".into(), FlowStyle::default());
    layout
        .texts
        .insert("other_text".into(), TextSizing::Natural);
    let measured = layout.with_text_measurer(metrics.clone());
    measured.apply_to_scene(&mut scene).unwrap();
    let before = scene.clone();
    let mut editor = SceneEditor::new(scene).unwrap();
    measured.install_reactive(&mut editor.engine).unwrap();
    metrics.calls.lock().unwrap().clear();
    editor
        .transaction(|s| {
            s.update_object(&"a".into(), |o| {
                if let Shape::Text(t) = &mut o.shape {
                    t.content = "a long".into()
                }
            })
        })
        .unwrap();
    assert!(
        !metrics
            .calls
            .lock()
            .unwrap()
            .iter()
            .any(|s| s == "unrelated")
    );
    assert_eq!(size(editor.scene(), "a").width, 80.);
    metrics.calls.lock().unwrap().clear();
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(metrics.calls.lock().unwrap().is_empty());
}
#[test]
fn nonconverging_text_dependency_hits_pass_budget_without_mutation() {
    struct Unstable(AtomicUsize);
    impl TextMeasurer for Unstable {
        fn measure(&self, r: &TextMeasureRequest) -> Result<TextMetrics, String> {
            Ok(TextMetrics {
                width: 20. + self.0.fetch_add(1, Ordering::SeqCst) as f32,
                height: r.line_height,
                line_height: r.line_height,
                lines: 1,
            })
        }
    }
    let (scene, mut layout, _) = texts();
    layout
        .containers
        .get_mut(&"root".into())
        .unwrap()
        .align_items = AlignItems::Start;
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    let err = layout
        .with_text_measurer(Arc::new(Unstable(AtomicUsize::new(0))))
        .apply_to_editor(&mut editor)
        .unwrap_err();
    assert_eq!(
        err,
        FlowError::NonConvergent {
            passes: MAX_FLOW_LAYOUT_PASSES
        }
    );
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
}
#[test]
fn fractional_scaled_percentage_reflow_keeps_unowned_axis_exact() {
    let (mut scene, mut layout) = fixture();
    scene
        .update_object(&"a".into(), |o| o.transform = Affine::scale(1.3, 0.7))
        .unwrap();
    layout.items.insert(
        "a".into(),
        FlowItem {
            width_percent: Some(0.37),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    let settled = scene.clone();
    for _ in 0..100 {
        assert!(
            layout
                .apply_to_scene(&mut scene)
                .unwrap()
                .changed
                .is_empty()
        );
        assert_eq!(size(&scene, "a").height, 10.);
    }
    assert_eq!(scene, settled);
}
