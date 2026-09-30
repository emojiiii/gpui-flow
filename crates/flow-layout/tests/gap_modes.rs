//! Leafer spacing contracts from pinned leafer-in 3eb445bf (common/gap.ts,
//! flowX.ts and flowY.ts). Taffy remains responsible for size and cross alignment.
use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::*;
use gpui_flow_scene::{Affine, Bounds, DisplayObject, Point, Scene, Shape, Size, display};
use serde_json::json;

fn fixture(width: f32, height: f32, sizes: &[(f32, f32)]) -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Frame::new(width, height)),
            None,
            None,
        )
        .unwrap();
    for (index, &(width, height)) in sizes.iter().enumerate() {
        scene
            .insert(
                DisplayObject::new(index.to_string(), display::Rect::new(width, height)),
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
fn bounds(scene: &Scene, id: &str) -> Bounds {
    let object = &scene.get(&id.into()).unwrap().object;
    object
        .transform
        .transform_bounds(object.shape.local_bounds().unwrap())
}
fn assert_box(scene: &Scene, id: &str, x: f32, y: f32, width: f32, height: f32) {
    assert_eq!(
        bounds(scene, id),
        Bounds::new(Point::new(x, y), Size::new(width, height)),
        "{id}"
    );
}

#[test]
fn auto_shares_only_inner_space_and_overrides_numeric_gap() {
    let (mut scene, mut layout) = fixture(200., 80., &[(20., 10.), (30., 20.), (40., 30.)]);
    *style(&mut layout) = FlowStyle {
        padding: Insets::all(10.),
        gap: 200.,
        gap_mode: GapMode::Auto,
        justify: JustifyContent::End,
        align_items: AlignItems::Center,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 10., 35., 20., 10.);
    assert_box(&scene, "1", 75., 30., 30., 20.);
    assert_box(&scene, "2", 150., 25., 40., 30.);
    assert!(layout.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn fit_can_overlap_and_place_later_items_before_the_start_without_shrinking() {
    let (mut scene, mut layout) = fixture(50., 20., &[(100., 10.), (100., 10.)]);
    style(&mut layout).gap_mode = GapMode::Fit;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 100., 10.);
    assert_box(&scene, "1", -50., 0., 100., 10.);
}

#[test]
fn auto_overflow_clamps_gap_at_zero() {
    let (mut scene, mut layout) = fixture(50., 20., &[(100., 10.), (100., 10.)]);
    style(&mut layout).gap_mode = GapMode::Auto;
    style(&mut layout).justify = JustifyContent::Center;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 100., 10.);
    assert_box(&scene, "1", 100., 0., 100., 10.);
}

#[test]
fn column_reverse_keeps_normal_axis_and_structure_with_fit() {
    let (mut scene, mut layout) = fixture(30., 50., &[(10., 60.), (20., 80.)]);
    let original_order = scene.get(&"root".into()).unwrap().children.clone();
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::ColumnReverse,
        gap_mode: GapMode::Fit,
        align_items: AlignItems::End,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "1", 10., 0., 20., 80.);
    assert_box(&scene, "0", 20., -10., 10., 60.);
    assert_eq!(scene.get(&"root".into()).unwrap().children, original_order);
}

#[test]
fn wrapped_rows_distribute_item_and_line_gaps_independently() {
    let (mut scene, mut layout) = fixture(
        100.,
        100.,
        &[(40., 10.), (40., 20.), (40., 30.), (40., 10.)],
    );
    *style(&mut layout) = FlowStyle {
        wrap: true,
        gap: 500.,
        line_gap: 500.,
        gap_mode: GapMode::Auto,
        line_gap_mode: GapMode::Auto,
        align_items: AlignItems::End,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 10., 40., 10.);
    assert_box(&scene, "1", 60., 0., 40., 20.);
    assert_box(&scene, "2", 0., 70., 40., 30.);
    assert_box(&scene, "3", 60., 90., 40., 10.);
}

#[test]
fn fit_does_not_disable_wrapping_and_can_overlap_completed_lines() {
    let (mut scene, mut layout) = fixture(50., 40., &[(40., 30.), (40., 30.), (40., 30.)]);
    *style(&mut layout) = FlowStyle {
        wrap: true,
        gap_mode: GapMode::Fit,
        line_gap_mode: GapMode::Fit,
        align_content: AlignContent::End,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    for (id, y) in [("0", 0.), ("1", 5.), ("2", 10.)] {
        assert_box(&scene, id, 0., y, 40., 30.);
    }
}

#[test]
fn wrapped_columns_and_reverse_line_order_support_both_spacing_axes() {
    let (mut scene, mut layout) = fixture(
        100.,
        100.,
        &[(10., 40.), (20., 40.), (30., 40.), (10., 40.)],
    );
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::Column,
        wrap: true,
        wrap_reverse: true,
        gap_mode: GapMode::Auto,
        line_gap_mode: GapMode::Auto,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "2", 0., 0., 30., 40.);
    assert_box(&scene, "3", 0., 60., 10., 40.);
    assert_box(&scene, "0", 80., 0., 10., 40.);
    assert_box(&scene, "1", 80., 60., 20., 40.);
}

#[test]
fn auto_line_gaps_do_not_compress_overflow() {
    let (mut scene, mut layout) = fixture(50., 40., &[(40., 30.), (40., 30.)]);
    *style(&mut layout) = FlowStyle {
        wrap: true,
        line_gap_mode: GapMode::Auto,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "1", 0., 30., 40., 30.);
}

#[test]
fn grow_takes_precedence_even_after_every_growing_item_caps() {
    for mode in [GapMode::Auto, GapMode::Fit] {
        let (mut scene, mut layout) = fixture(100., 20., &[(20., 10.), (20., 10.), (20., 10.)]);
        style(&mut layout).gap_mode = mode;
        layout.items.insert(
            "0".into(),
            FlowItem {
                grow: 0.125,
                max_width: Some(30.),
                ..Default::default()
            },
        );
        layout.apply_to_scene(&mut scene).unwrap();
        assert_box(&scene, "0", 0., 0., 30., 10.);
        assert_box(&scene, "1", 30., 0., 20., 10.);
        assert_box(&scene, "2", 50., 0., 20., 10.);
    }
}

#[test]
fn exhausted_grow_does_not_trigger_fit_overlap() {
    let (mut scene, mut layout) = fixture(50., 20., &[(100., 10.), (100., 10.)]);
    style(&mut layout).gap_mode = GapMode::Fit;
    layout.items.insert(
        "0".into(),
        FlowItem {
            grow: 1.,
            min_width: Some(100.),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "1", 100., 0., 100., 10.);
}

#[test]
fn excluded_children_do_not_count_but_hidden_children_keep_their_slots() {
    let (mut scene, mut layout) = fixture(100., 20., &[(10., 10.); 4]);
    style(&mut layout).gap_mode = GapMode::Auto;
    layout.items.insert(
        "1".into(),
        FlowItem {
            in_flow: false,
            ..Default::default()
        },
    );
    scene
        .update_object(&"1".into(), |object| object.transform.tx = 77.)
        .unwrap();
    scene
        .update_object(&"2".into(), |object| object.visible = false)
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    for (id, x) in [("0", 0.), ("1", 77.), ("2", 45.), ("3", 90.)] {
        assert_box(&scene, id, x, 0., 10., 10.);
    }
}

#[test]
fn empty_and_singleton_automatic_gaps_keep_normal_alignment() {
    for mode in [GapMode::Auto, GapMode::Fit] {
        for count in [0, 1] {
            let (mut scene, mut layout) = fixture(100., 80., &vec![(20., 10.); count]);
            *style(&mut layout) = FlowStyle {
                wrap: true,
                gap_mode: mode,
                line_gap_mode: mode,
                justify: JustifyContent::Center,
                align_content: AlignContent::Center,
                ..Default::default()
            };
            layout.apply_to_scene(&mut scene).unwrap();
            if count == 1 {
                assert_box(&scene, "0", 40., 35., 20., 10.);
            }
        }
    }
}

#[test]
fn line_gap_is_unused_without_wrapping() {
    let (mut scene, mut layout) = fixture(100., 80., &[(20., 10.), (20., 20.)]);
    *style(&mut layout) = FlowStyle {
        line_gap: 1000.,
        line_gap_mode: GapMode::Fit,
        align_items: AlignItems::Center,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 35., 20., 10.);
    assert_box(&scene, "1", 20., 30., 20., 20.);
}

#[test]
fn exact_fit_has_zero_gap_and_fractional_spacing_is_not_pixel_rounded() {
    let (mut scene, mut layout) = fixture(10.5, 20., &[(2., 10.), (2.5, 10.), (3., 10.)]);
    style(&mut layout).gap_mode = GapMode::Auto;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 2., 10.);
    assert_box(&scene, "1", 3.5, 0., 2.5, 10.);
    assert_box(&scene, "2", 7.5, 0., 3., 10.);
    scene
        .update_object(&"root".into(), |object| {
            let Shape::Frame(frame) = &mut object.shape else {
                unreachable!()
            };
            frame.size.width = 7.5;
        })
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "1", 2., 0., 2.5, 10.);
    assert_box(&scene, "2", 4.5, 0., 3., 10.);
}

#[test]
fn automatic_gaps_measure_transformed_boxes_and_do_not_change_geometry() {
    let (mut scene, mut layout) = fixture(100., 30., &[(10., 10.), (20., 10.)]);
    style(&mut layout).gap_mode = GapMode::Auto;
    scene
        .update_object(&"0".into(), |object| {
            object.transform = Affine {
                a: -2.,
                d: 2.,
                tx: 91.,
                ..Affine::IDENTITY
            }
        })
        .unwrap();
    let shape = scene.get(&"0".into()).unwrap().object.shape.clone();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 20., 20.);
    assert_box(&scene, "1", 80., 0., 20., 10.);
    assert_eq!(scene.get(&"0".into()).unwrap().object.shape, shape);
}

#[test]
fn main_auto_gap_allows_independently_content_sized_cross_axis() {
    let (mut scene, mut layout) = fixture(100., 200., &[(40., 10.), (40., 20.), (40., 30.)]);
    *style(&mut layout) = FlowStyle {
        height_sizing: AxisSizing::content(),
        wrap: true,
        gap_mode: GapMode::Auto,
        line_gap: 5.,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "root", 0., 0., 100., 55.);
    assert_box(&scene, "1", 60., 0., 40., 20.);
    assert_box(&scene, "2", 0., 25., 40., 30.);
    assert!(layout.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn content_sized_spacing_axis_is_rejected_without_mutation() {
    for direction in [FlowDirection::Row, FlowDirection::ColumnReverse] {
        for main_axis in [false, true] {
            let (mut scene, mut layout) = fixture(100., 100., &[(20., 20.), (20., 20.)]);
            let original = scene.clone();
            let s = style(&mut layout);
            s.direction = direction;
            if main_axis {
                s.gap_mode = GapMode::Auto;
            } else {
                s.line_gap_mode = GapMode::Fit;
            }
            if main_axis == (direction == FlowDirection::Row) {
                s.width_sizing = AxisSizing::content();
            } else {
                s.height_sizing = AxisSizing::content();
            }
            assert!(matches!(
                layout.apply_to_scene(&mut scene),
                Err(FlowError::InvalidInput { .. })
            ));
            assert_eq!(scene, original);
        }
    }
}

#[test]
fn ignored_numeric_gaps_are_still_validated_and_failure_is_atomic() {
    for invalid in [f32::NAN, f32::INFINITY, -1.] {
        let (mut scene, mut layout) = fixture(100., 100., &[(20., 20.), (20., 20.)]);
        let original = scene.clone();
        *style(&mut layout) = FlowStyle {
            gap: invalid,
            gap_mode: GapMode::Auto,
            ..Default::default()
        };
        assert!(layout.apply_to_scene(&mut scene).is_err());
        assert_eq!(scene, original);
    }
}

#[test]
fn json_keeps_old_numeric_shape_and_round_trips_modes() {
    let old: FlowStyle = serde_json::from_value(json!({"gap": 7.0, "line_gap": 11.0})).unwrap();
    assert_eq!(old.gap_mode, GapMode::Fixed);
    let encoded = serde_json::to_value(&old).unwrap();
    assert!(encoded.get("gap_mode").is_none());
    assert!(encoded.get("line_gap_mode").is_none());
    assert_eq!(encoded["gap"], 7.0);
    let (scene, mut layout) = fixture(100., 100., &[(20., 20.), (20., 20.)]);
    style(&mut layout).gap_mode = GapMode::Fit;
    style(&mut layout).line_gap_mode = GapMode::Auto;
    let document = FlowDocument { scene, layout };
    let json = document.to_json().unwrap();
    assert_eq!(FlowDocument::from_json(&json).unwrap(), document);
    assert!(serde_json::from_value::<FlowStyle>(json!({"gap_mode": "unknown"})).is_err());
}

#[test]
fn huge_signed_gap_stays_in_widened_arithmetic() {
    let (mut scene, mut layout) = fixture(1., 20., &[(1e38, 10.), (1e38, 10.)]);
    style(&mut layout).gap_mode = GapMode::Fit;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 1e38, 10.);
    assert_box(&scene, "1", -1e38, 0., 1e38, 10.);
    assert!(layout.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn reactive_parent_resize_recomputes_gaps_in_one_undo_and_preserves_handles() {
    let (mut scene, mut layout) = fixture(100., 20., &[(20., 10.), (20., 10.)]);
    style(&mut layout).gap_mode = GapMode::Fit;
    layout.apply_to_scene(&mut scene).unwrap();
    let before = scene.clone();
    let mut editor = SceneEditor::new(scene).unwrap();
    let handle = editor.engine.handle(&"1".into()).unwrap();
    layout.install_reactive(&mut editor.engine).unwrap();
    editor
        .transaction(|scene| {
            scene.update_object(&"root".into(), |object| {
                let Shape::Frame(frame) = &mut object.shape else {
                    unreachable!()
                };
                frame.size.width = 30.;
            })
        })
        .unwrap();
    assert_box(editor.scene(), "1", 10., 0., 20., 10.);
    assert_eq!(editor.engine.handle(&"1".into()), Some(handle));
    let revision = editor.engine.revision();
    layout.apply_to_engine(&mut editor.engine).unwrap();
    assert_eq!(editor.engine.revision(), revision);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert_box(editor.scene(), "1", 10., 0., 20., 10.);
}

#[test]
fn huge_fit_line_gaps_do_not_invent_cross_alignment_space() {
    let (mut scene, mut layout) = fixture(1., 1., &[(1., 1e38), (1., 1e38)]);
    *style(&mut layout) = FlowStyle {
        wrap: true,
        line_gap_mode: GapMode::Fit,
        align_content: AlignContent::End,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 1., 1e38);
    assert_box(&scene, "1", 0., -1e38, 1., 1e38);
}

#[test]
fn exhausted_padding_still_allows_fit_spacing_without_zero_sized_children() {
    let (mut scene, mut layout) = fixture(10., 40., &[(20., 10.), (20., 10.)]);
    *style(&mut layout) = FlowStyle {
        padding: Insets::all(10.),
        gap_mode: GapMode::Fit,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 10., 10., 20., 10.);
    assert_box(&scene, "1", -10., 10., 20., 10.);
}

#[test]
fn column_grow_with_fit_item_gap_is_rejected_atomically_but_fixed_is_unchanged() {
    for direction in [FlowDirection::Column, FlowDirection::ColumnReverse] {
        for wrap in [false, true] {
            let (scene, mut layout) = fixture(20., 50., &[(10., 100.), (10., 100.)]);
            *style(&mut layout) = FlowStyle {
                direction,
                gap_mode: GapMode::Fit,
                wrap,
                ..Default::default()
            };
            layout.items.insert(
                "0".into(),
                FlowItem {
                    grow: 1.,
                    min_height: Some(100.),
                    ..Default::default()
                },
            );
            let mut editor = SceneEditor::new(scene.clone()).unwrap();
            let revision = editor.engine.revision();
            let err = layout.apply_to_editor(&mut editor).unwrap_err();
            assert!(err.to_string().contains("column grow"));
            assert_eq!(editor.scene(), &scene);
            assert_eq!(editor.engine.revision(), revision);
            assert!(!editor.undo());
            style(&mut layout).gap_mode = GapMode::Fixed;
            layout.apply_to_editor(&mut editor).unwrap();
        }
    }
}

#[test]
fn excluded_column_grow_does_not_block_automatic_spacing() {
    let (mut scene, mut layout) = fixture(20., 100., &[(10., 20.); 3]);
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::Column,
        gap_mode: GapMode::Fit,
        ..Default::default()
    };
    layout.items.insert(
        "1".into(),
        FlowItem {
            in_flow: false,
            grow: 1.,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "2", 0., 80., 10., 20.);
}

#[test]
fn automatic_line_then_singleton_restores_explicit_alignment() {
    let (mut scene, mut layout) = fixture(100., 100., &[(40., 10.); 3]);
    *style(&mut layout) = FlowStyle {
        wrap: true,
        gap_mode: GapMode::Auto,
        justify: JustifyContent::End,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 40., 10.);
    assert_box(&scene, "1", 60., 0., 40., 10.);
    assert_box(&scene, "2", 60., 10., 40., 10.);
}

#[test]
fn explicit_space_between_remains_independent_of_automatic_gap_precedence() {
    let (mut scene, mut layout) = fixture(100., 20., &[(20., 10.); 3]);
    *style(&mut layout) = FlowStyle {
        gap_mode: GapMode::Auto,
        justify: JustifyContent::SpaceBetween,
        ..Default::default()
    };
    layout.items.insert(
        "0".into(),
        FlowItem {
            grow: 1.,
            max_width: Some(30.),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., 30., 10.);
    assert_box(&scene, "1", 45., 0., 20., 10.);
    assert_box(&scene, "2", 80., 0., 20., 10.);
}

#[test]
fn subnormal_fit_uses_unchanged_wide_solver_without_losing_overlap() {
    let unit = f32::MIN_POSITIVE / 4.;
    let (mut scene, mut layout) = fixture(unit, 20., &[(unit * 2., 10.); 2]);
    style(&mut layout).gap_mode = GapMode::Fit;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_box(&scene, "0", 0., 0., unit * 2., 10.);
    assert_box(&scene, "1", -unit, 0., unit * 2., 10.);
}

#[test]
fn column_auto_gap_is_zero_after_exhausted_or_capped_grow() {
    for direction in [FlowDirection::Column, FlowDirection::ColumnReverse] {
        for available in [50., 300.] {
            let (mut scene, mut layout) = fixture(20., available, &[(10., 100.); 2]);
            *style(&mut layout) = FlowStyle {
                direction,
                gap_mode: GapMode::Auto,
                ..Default::default()
            };
            layout.items.insert(
                "0".into(),
                FlowItem {
                    grow: 1.,
                    min_height: Some(100.),
                    max_height: Some(100.),
                    ..Default::default()
                },
            );
            layout.apply_to_scene(&mut scene).unwrap();
            let (first, second) = if direction == FlowDirection::Column {
                ("0", "1")
            } else {
                ("1", "0")
            };
            assert_box(&scene, first, 0., 0., 10., 100.);
            assert_box(&scene, second, 0., 100., 10., 100.);
        }
    }
}

#[test]
fn ignored_numeric_line_gap_is_still_validated() {
    for invalid in [f32::NAN, f32::INFINITY, -1.] {
        let (mut scene, mut layout) = fixture(100., 100., &[(20., 20.); 2]);
        let original = scene.clone();
        *style(&mut layout) = FlowStyle {
            line_gap: invalid,
            line_gap_mode: GapMode::Fit,
            ..Default::default()
        };
        assert!(layout.apply_to_scene(&mut scene).is_err());
        assert_eq!(scene, original);
    }
}
