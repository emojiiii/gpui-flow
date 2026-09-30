use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ChangeOrigin, DirtyFlags, SceneEngine};
use gpui_flow_layout::*;
use gpui_flow_scene::{
    Affine, Bounds, DisplayId, DisplayObject, EventKind, HitTestOptions, ListenerOptions, Point,
    Scene, SceneEvent, Shape, Size, display,
};
use std::{cell::Cell, rc::Rc};

fn fixture(width: f32, height: f32, sizes: &[(f32, f32)]) -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Box::new(width, height)),
            None,
            None,
        )
        .unwrap();
    for (index, &(width, height)) in sizes.iter().enumerate() {
        scene
            .insert(
                DisplayObject::new(index.to_string(), display::Rect::new(width, height))
                    .with_transform(Affine::translation(300.0, 400.0)),
                Some("root".into()),
                None,
            )
            .unwrap();
    }
    let mut layout = FlowLayout::default();
    layout
        .containers
        .insert("root".into(), FlowStyle::default());
    (scene, layout)
}
fn style(layout: &mut FlowLayout) -> &mut FlowStyle {
    layout.containers.get_mut(&"root".into()).unwrap()
}
fn object<'a>(scene: &'a Scene, id: &str) -> &'a DisplayObject {
    &scene.get(&id.into()).unwrap().object
}
fn box_in_parent(scene: &Scene, id: &str) -> Bounds {
    let object = object(scene, id);
    object
        .transform
        .transform_bounds(object.shape.local_bounds().unwrap())
}
fn assert_box(scene: &Scene, id: &str, x: f32, y: f32, width: f32, height: f32) {
    assert_eq!(
        box_in_parent(scene, id),
        Bounds::new(Point::new(x, y), Size::new(width, height)),
        "{id}"
    );
}

#[test]
fn row_padding_gap_center_and_end_alignment() {
    let (mut scene, mut layout) = fixture(200., 100., &[(30., 20.), (50., 40.)]);
    *style(&mut layout) = FlowStyle {
        padding: Insets::all(10.),
        gap: 10.,
        justify: JustifyContent::Center,
        align_items: AlignItems::End,
        ..Default::default()
    };
    let report = layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 55., 70., 30., 20.);
    assert_box(&scene, "1", 95., 50., 50., 40.);
    assert_eq!(report.items, 2);
    assert_eq!(report.containers, 1);
    assert_eq!(report.changed, vec![DisplayId::from("0"), "1".into()]);
    assert!(
        layout
            .apply_to_scene(&mut scene)
            .unwrap()
            .changed
            .is_empty()
    );
}

#[test]
fn column_swaps_main_cross_axes_and_asymmetric_padding() {
    let (mut scene, mut layout) = fixture(100., 200., &[(20., 30.), (40., 50.)]);
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::Column,
        padding: Insets {
            left: 10.,
            right: 20.,
            top: 30.,
            bottom: 10.,
        },
        gap: 10.,
        justify: JustifyContent::End,
        align_items: AlignItems::Center,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 35., 100., 20., 30.);
    assert_box(&scene, "1", 25., 140., 40., 50.);
}

#[test]
fn wrap_uses_complete_gap_and_packs_lines_with_cross_alignment() {
    let (mut scene, mut layout) = fixture(120., 140., &[(45., 20.), (45., 30.), (80., 10.)]);
    *style(&mut layout) = FlowStyle {
        padding: Insets::all(10.),
        gap: 10.,
        line_gap: 15.,
        wrap: true,
        justify: JustifyContent::End,
        align_items: AlignItems::Center,
        align_content: AlignContent::Center,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 10., 47.5, 45., 20.);
    assert_box(&scene, "1", 65., 42.5, 45., 30.);
    assert_box(&scene, "2", 30., 87.5, 80., 10.);
    // Increasing gap beyond the exact first-line fit must wrap the second item.
    style(&mut layout).gap = 11.;
    layout.apply_to_scene(&mut scene).unwrap();
    assert!(box_in_parent(&scene, "1").min_y() > box_in_parent(&scene, "0").max_y());
}

#[test]
fn column_wrap_and_align_self() {
    let (mut scene, mut layout) = fixture(150., 100., &[(20., 40.), (40., 40.), (30., 50.)]);
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::Column,
        gap: 10.,
        line_gap: 20.,
        wrap: true,
        align_content: AlignContent::End,
        ..Default::default()
    };
    layout.items.insert(
        "0".into(),
        FlowItem {
            align_self: Some(AlignItems::End),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 80., 0., 20., 40.);
    assert_box(&scene, "1", 60., 50., 40., 40.);
    assert_box(&scene, "2", 120., 0., 30., 50.);
}

#[test]
fn space_between_uses_minimum_gap_and_never_shrinks() {
    let (mut scene, mut layout) = fixture(200., 50., &[(40., 20.), (40., 20.), (40., 20.)]);
    style(&mut layout).justify = JustifyContent::SpaceBetween;
    style(&mut layout).gap = 5.;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "1", 80., 0., 40., 20.);
    assert_box(&scene, "2", 160., 0., 40., 20.);
    scene
        .update_object(&"root".into(), |o| {
            o.shape = display::Box::new(100., 50.).into()
        })
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "2", 90., 0., 40., 20.);
}

#[test]
fn grow_weights_redistribute_multiple_max_constraints() {
    let (mut scene, mut layout) = fixture(300., 80., &[(10., 20.), (10., 20.), (10., 20.)]);
    style(&mut layout).gap = 10.;
    for (id, max) in [("0", Some(30.)), ("1", Some(50.)), ("2", None)] {
        layout.items.insert(
            id.into(),
            FlowItem {
                grow: 1.,
                max_width: max,
                ..Default::default()
            },
        );
    }
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 30., 20.);
    assert_box(&scene, "1", 40., 0., 50., 20.);
    assert_box(&scene, "2", 100., 0., 200., 20.);
    assert!(
        layout
            .apply_to_scene(&mut scene)
            .unwrap()
            .changed
            .is_empty()
    );
    scene
        .update_object(&"root".into(), |o| {
            o.shape = display::Box::new(180., 80.).into()
        })
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "2", 100., 0., 80., 20.);
}

#[test]
fn grow_basis_is_saved_input_not_previous_output_and_wrap_is_stable() {
    let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.), (20., 20.), (20., 30.)]);
    style(&mut layout).wrap = true;
    style(&mut layout).gap = 10.;
    for id in ["0", "1", "2"] {
        layout.items.insert(
            id.into(),
            FlowItem {
                basis: Some(40.),
                grow: 1.,
                ..Default::default()
            },
        );
    }
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 45., 10.);
    assert_box(&scene, "1", 55., 0., 45., 20.);
    assert_box(&scene, "2", 0., 20., 100., 30.);
    assert!(
        layout
            .apply_to_scene(&mut scene)
            .unwrap()
            .changed
            .is_empty()
    );
}

#[test]
fn column_weighted_grow_minimum_overflow_and_stretch_constraints() {
    let (mut scene, mut layout) = fixture(100., 140., &[(20., 10.), (20., 10.)]);
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::Column,
        gap: 20.,
        align_items: AlignItems::Stretch,
        ..Default::default()
    };
    layout.items.insert(
        "0".into(),
        FlowItem {
            grow: 1.,
            max_width: Some(70.),
            ..Default::default()
        },
    );
    layout.items.insert(
        "1".into(),
        FlowItem {
            grow: 2.,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 70., 40.);
    assert_box(&scene, "1", 0., 60., 100., 80.);
    layout.items.get_mut(&"0".into()).unwrap().min_height = Some(200.);
    // Keep the second allocated shape positive in the exhausted-space case.
    layout.items.get_mut(&"1".into()).unwrap().min_height = Some(10.);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 70., 200.);
    assert_box(&scene, "1", 0., 220., 100., 10.);
}

#[test]
fn nested_containers_are_parent_first_not_id_sorted() {
    let (mut scene, mut layout) = fixture(300., 200., &[]);
    scene
        .insert(
            DisplayObject::new("aaa_nested", display::Frame::new(50., 50.)),
            Some("root".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("leaf", display::Rect::new(20., 20.)),
            Some("aaa_nested".into()),
            None,
        )
        .unwrap();
    style(&mut layout).padding = Insets::all(10.);
    style(&mut layout).align_items = AlignItems::Stretch;
    layout.items.insert(
        "aaa_nested".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    layout.containers.insert(
        "aaa_nested".into(),
        FlowStyle {
            padding: Insets::all(5.),
            justify: JustifyContent::End,
            align_items: AlignItems::End,
            ..Default::default()
        },
    );
    let report = layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(report.containers, 2);
    assert_box(&scene, "aaa_nested", 10., 10., 280., 180.);
    assert_box(&scene, "leaf", 255., 155., 20., 20.);
    assert!(
        layout
            .apply_to_scene(&mut scene)
            .unwrap()
            .changed
            .is_empty()
    );
}

#[test]
fn hidden_keeps_slot_and_excluded_group_keeps_transform() {
    let (mut scene, mut layout) = fixture(100., 100., &[(20., 20.), (20., 20.)]);
    scene
        .update_object(&"0".into(), |o| o.visible = false)
        .unwrap();
    scene
        .insert(
            DisplayObject::new("group", display::Group::new())
                .with_transform(Affine::translation(73., 91.)),
            Some("root".into()),
            Some(1),
        )
        .unwrap();
    layout.items.insert(
        "group".into(),
        FlowItem {
            in_flow: false,
            ..Default::default()
        },
    );
    let report = layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(report.items, 2);
    assert_box(&scene, "1", 20., 0., 20., 20.);
    assert_eq!(
        object(&scene, "group").transform,
        Affine::translation(73., 91.)
    );
    assert!(!object(&scene, "0").visible);
}

#[test]
fn transformed_negative_origin_geometry_preserves_linear_transform() {
    let (mut scene, layout) = fixture(200., 100., &[]);
    let transform = Affine::translation(90., 80.) * Affine::rotation(90.);
    scene
        .insert(
            DisplayObject::new(
                "line",
                display::Line::new(Point::new(-10., -20.), Point::new(10., 20.)),
            )
            .with_transform(transform),
            Some("root".into()),
            None,
        )
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    let after = object(&scene, "line").transform;
    assert_eq!(
        (after.a, after.b, after.c, after.d),
        (transform.a, transform.b, transform.c, transform.d)
    );
    let bounds = box_in_parent(&scene, "line");
    assert!(bounds.min_x().abs() < 0.00001 && bounds.min_y().abs() < 0.00001);
    assert!(layout.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn positive_scale_resize_uses_parent_local_units() {
    let (mut scene, mut layout) = fixture(200., 100., &[(20., 10.)]);
    scene
        .update_object(&"0".into(), |o| o.transform = Affine::scale(2., 4.))
        .unwrap();
    style(&mut layout).align_items = AlignItems::Stretch;
    layout.items.insert(
        "0".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 200., 100.);
    assert_eq!(
        object(&scene, "0").shape.local_bounds().unwrap().size,
        Size::new(100., 25.)
    );
    assert!(
        layout
            .apply_to_scene(&mut scene)
            .unwrap()
            .changed
            .is_empty()
    );
}

#[test]
fn engine_one_batch_retains_handles_listeners_and_updates_bounds_hit_damage() {
    let (scene, layout) = fixture(200., 100., &[(20., 20.), (20., 20.)]);
    let mut engine = SceneEngine::new(scene).unwrap();
    let handle = engine.handle(&"0".into()).unwrap();
    let called = Rc::new(Cell::new(0));
    let copy = called.clone();
    let batches = Rc::new(Cell::new(0));
    let batch_count = batches.clone();
    engine
        .on_change(None, move |_| batch_count.set(batch_count.get() + 1))
        .unwrap();
    engine
        .on(
            handle,
            EventKind::Click,
            ListenerOptions::default(),
            move |_| copy.set(copy.get() + 1),
        )
        .unwrap();
    let (report, changes) = layout.apply_to_engine(&mut engine).unwrap();
    assert_eq!(report.changed.len(), 2);
    assert_eq!(changes.origin, ChangeOrigin::Layout);
    assert_eq!(engine.revision(), 1);
    assert_eq!(batches.get(), 1);
    assert!(changes.lifecycle.is_empty());
    assert!(changes.structure.is_empty());
    assert!(changes.dirty[&"0".into()].contains(DirtyFlags::TRANSFORM));
    assert_eq!(engine.handle(&"0".into()), Some(handle));
    assert_eq!(
        engine.world_bounds(&"0".into()).unwrap(),
        engine.scene().world_bounds(&"0".into()).unwrap()
    );
    assert!(!changes.damage.is_empty());
    assert_eq!(
        engine
            .hit_test(Point::new(5., 5.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id,
        "0".into()
    );
    let event = SceneEvent::new(EventKind::Click, "0");
    engine.dispatch(event).unwrap();
    assert_eq!(called.get(), 1);
    assert!(!layout.apply_to_engine(&mut engine).unwrap().1.changed());
    assert_eq!(engine.revision(), 1);
    assert_eq!(batches.get(), 1);
}

#[test]
fn editor_single_undo_and_redo_restore_all_children_and_keep_identity() {
    let (scene, layout) = fixture(200., 100., &[(20., 20.), (20., 20.), (20., 20.)]);
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    let handle = editor.engine.handle(&"1".into());
    layout.apply_to_editor(&mut editor).unwrap();
    let after = editor.scene().clone();
    layout.apply_to_editor(&mut editor).unwrap();
    assert!(editor.undo());
    assert_eq!(*editor.scene(), scene);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert_eq!(*editor.scene(), after);
    assert_eq!(editor.engine.handle(&"1".into()), handle);
}

#[test]
fn failed_late_resize_keeps_document_revision_history_and_callbacks_unchanged() {
    let (scene, mut layout) = fixture(200., 100., &[(20., 20.), (20., 20.)]);
    let mut editor = SceneEditor::new(scene).unwrap();
    editor
        .transaction(|scene| {
            scene.update_object(&"1".into(), |o| o.transform = Affine::rotation(45.))
        })
        .unwrap();
    let before = editor.scene().clone();
    let revision = editor.engine.revision();
    let last_change = editor.engine.last_change().clone();
    let batches = Rc::new(Cell::new(0));
    let batch_count = batches.clone();
    editor
        .engine
        .on_change(None, move |_| batch_count.set(batch_count.get() + 1))
        .unwrap();
    layout.items.insert(
        "1".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    assert!(
        layout
            .apply_to_editor(&mut editor)
            .unwrap_err()
            .to_string()
            .contains("axis-aligned")
    );
    assert_eq!(*editor.scene(), before);
    assert_eq!(editor.engine.revision(), revision);
    assert_eq!(*editor.engine.last_change(), last_change);
    assert_eq!(batches.get(), 0);
    assert!(editor.undo()); // Existing history survived the failed layout.
    assert!(!editor.undo());
}

#[test]
fn invalid_configuration_and_nonfinite_values_fail_before_commit() {
    let (scene, layout) = fixture(200., 100., &[(20., 20.)]);
    for value in [-1., f32::NAN, f32::INFINITY] {
        let mut invalid = layout.clone();
        style(&mut invalid).gap = value;
        let mut next = scene.clone();
        assert!(invalid.apply_to_scene(&mut next).is_err());
        assert_eq!(next, scene);
    }
    for item in [
        FlowItem {
            min_width: Some(30.),
            max_width: Some(10.),
            ..Default::default()
        },
        FlowItem {
            grow: f32::NAN,
            ..Default::default()
        },
        FlowItem {
            basis: Some(-1.),
            ..Default::default()
        },
        FlowItem {
            max_height: Some(0.),
            ..Default::default()
        },
    ] {
        let mut invalid = layout.clone();
        invalid.items.insert("0".into(), item);
        assert!(invalid.arranged(&scene).is_err());
    }
}

#[test]
fn missing_ids_wrong_container_and_orphan_item_are_rejected() {
    let (scene, mut layout) = fixture(100., 100., &[(20., 20.)]);
    layout
        .containers
        .insert("missing".into(), FlowStyle::default());
    assert!(layout.arranged(&scene).is_err());
    layout.containers.remove(&"missing".into());
    layout.containers.insert("0".into(), FlowStyle::default());
    assert!(layout.arranged(&scene).is_err());
    layout.containers.remove(&"0".into());
    layout.items.insert("root".into(), FlowItem::default());
    assert!(layout.arranged(&scene).is_err());
}

#[test]
fn exhausted_grow_is_atomic_and_fixed_overflow_remains_valid() {
    let (mut scene, mut layout) = fixture(20., 20., &[(10., 10.)]);
    style(&mut layout).padding = Insets::all(30.);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 30., 30., 10., 10.);
    let before = scene.clone();
    layout.items.insert(
        "0".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    assert!(layout.apply_to_scene(&mut scene).is_err());
    assert_eq!(scene, before);
    layout.items.get_mut(&"0".into()).unwrap().min_width = Some(5.);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 30., 30., 5., 10.);
}

#[test]
fn document_roundtrip_preserves_config_application_data_and_geometry() {
    let (mut scene, mut layout) = fixture(200., 100., &[(20., 20.)]);
    scene.data = serde_json::json!({"layout": "application-owned"});
    scene
        .update_object(&"0".into(), |o| o.data = serde_json::json!({"keep": true}))
        .unwrap();
    layout.items.insert(
        "0".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    let document = FlowDocument { scene, layout };
    let json = document.to_json().unwrap();
    let mut restored = FlowDocument::from_json(&json).unwrap();
    assert_eq!(restored, document);
    assert!(
        restored
            .layout
            .apply_to_scene(&mut restored.scene)
            .unwrap()
            .changed
            .is_empty()
    );
    assert!(!json.contains("revision") && !json.contains("handle") && !json.contains("cache"));
    let mut malformed = serde_json::to_value(&document).unwrap();
    malformed["layout"]["containers"]["root"]["gapp"] = serde_json::json!(20);
    assert!(FlowDocument::from_json(&malformed.to_string()).is_err());
}

#[test]
fn huge_finite_gap_overflow_rejects_whole_pass() {
    let (mut scene, mut layout) = fixture(100., 100., &[(20., 20.), (20., 20.), (20., 20.)]);
    style(&mut layout).gap = f32::MAX;
    let before = scene.clone();
    assert!(layout.apply_to_scene(&mut scene).is_err());
    assert_eq!(scene, before);
}

#[test]
fn resize_preserves_non_size_shape_properties_and_parent_transform() {
    let (mut scene, mut layout) = fixture(200., 100., &[(20., 20.)]);
    scene
        .update_object(&"root".into(), |o| o.transform = Affine::rotation(25.))
        .unwrap();
    scene
        .update_object(&"0".into(), |o| {
            o.shape = display::Rect::new(20., 20.).rounded(9.).into()
        })
        .unwrap();
    layout.items.insert(
        "0".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(object(&scene, "root").transform, Affine::rotation(25.));
    let Shape::Rect(rect) = &object(&scene, "0").shape else {
        panic!("rect")
    };
    assert_eq!(rect.corner_radius, 9.);
    assert_eq!(rect.size.width, 200.);
}

#[test]
fn fractional_scales_repeated_grow_do_not_drift() {
    for scale in [0.1, 0.3, 0.7, 1.1, 1.7, 3.3, 100.1] {
        let (mut scene, mut layout) = fixture(97.3, 58.1, &[(13.7, 17.9)]);
        scene
            .update_object(&"0".into(), |o| o.transform = Affine::scale(scale, scale))
            .unwrap();
        layout.items.insert(
            "0".into(),
            FlowItem {
                grow: 1.,
                ..Default::default()
            },
        );
        layout.apply_to_scene(&mut scene).unwrap();
        let first = scene.clone();
        for _ in 0..5 {
            assert!(
                layout
                    .apply_to_scene(&mut scene)
                    .unwrap()
                    .changed
                    .is_empty(),
                "scale {scale}"
            );
            assert_eq!(scene, first);
        }
        assert_eq!(
            object(&scene, "0")
                .shape
                .local_bounds()
                .unwrap()
                .size
                .height,
            17.9
        );
    }
}
