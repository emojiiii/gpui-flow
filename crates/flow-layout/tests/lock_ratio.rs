//! Uniform resize fixtures derived from pinned Leafer resizeWidth/resizeHeight.
//! These test the bounded main-axis contract, not full upstream Flow pass order.
use gpui_flow_layout::*;
use gpui_flow_scene::{
    Affine, Bounds, Color, DisplayObject, Point, Scene, Shape, Size, Style, display,
};
use serde_json::json;

fn fixture(w: f32, h: f32, sizes: &[(f32, f32)]) -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Frame::new(w, h)),
            None,
            None,
        )
        .unwrap();
    let mut layout = FlowLayout::default();
    layout
        .containers
        .insert("root".into(), FlowStyle::default());
    for (i, &(w, h)) in sizes.iter().enumerate() {
        let id = i.to_string();
        scene
            .insert(
                DisplayObject::new(id.clone(), display::Rect::new(w, h)),
                Some("root".into()),
                None,
            )
            .unwrap();
        layout.items.insert(
            id.into(),
            FlowItem {
                lock_ratio: true,
                ..Default::default()
            },
        );
    }
    (scene, layout)
}
fn style(layout: &mut FlowLayout) -> &mut FlowStyle {
    layout.containers.get_mut(&"root".into()).unwrap()
}
fn item<'a>(layout: &'a mut FlowLayout, id: &str) -> &'a mut FlowItem {
    layout.items.get_mut(&id.into()).unwrap()
}
fn object<'a>(scene: &'a Scene, id: &str) -> &'a DisplayObject {
    &scene.get(&id.into()).unwrap().object
}
fn slot(scene: &Scene, id: &str) -> Bounds {
    let o = object(scene, id);
    o.transform
        .transform_bounds(o.shape.local_bounds().unwrap())
}
fn assert_slot(scene: &Scene, id: &str, x: f32, y: f32, w: f32, h: f32) {
    assert_eq!(
        slot(scene, id),
        Bounds::new(Point::new(x, y), Size::new(w, h)),
        "{id}"
    );
}
fn stable(layout: &FlowLayout, scene: &Scene) {
    let (again, report) = layout.arranged(scene).unwrap();
    assert_eq!(&again, scene);
    assert!(report.changed.is_empty());
}
fn rejected(scene: &mut Scene, layout: &FlowLayout, message: &str) {
    let before = scene.clone();
    let error = layout.apply_to_scene(scene).unwrap_err().to_string();
    assert!(error.contains(message), "{error}");
    assert_eq!(*scene, before);
}

#[test]
fn old_json_omits_false_and_true_round_trips_without_changing_numeric_fields() {
    let old: FlowItem = serde_json::from_value(json!({"grow": 2., "basis": 30.})).unwrap();
    assert!(!old.lock_ratio);
    assert!(
        serde_json::to_value(&old)
            .unwrap()
            .get("lock_ratio")
            .is_none()
    );
    let (scene, layout) = fixture(100., 100., &[(20., 10.)]);
    let document = FlowDocument { scene, layout };
    assert_eq!(
        FlowDocument::from_json(&document.to_json().unwrap()).unwrap(),
        document
    );
    assert_eq!(
        serde_json::to_value(&document.layout.items[&"0".into()]).unwrap()["lock_ratio"],
        true
    );
    for bad in [
        json!({"lock_ratio": 2.}),
        json!({"lock_ratio": "true"}),
        json!({"lock_ratio": null}),
    ] {
        assert!(serde_json::from_value::<FlowItem>(bad).is_err());
    }
}

#[test]
fn row_grow_preserves_ratio_with_padding_gap_and_final_cross_alignment() {
    let (mut scene, mut layout) = fixture(240., 160., &[(20., 10.), (30., 30.)]);
    *style(&mut layout) = FlowStyle {
        padding: Insets::all(10.),
        gap: 20.,
        align_items: AlignItems::Center,
        ..Default::default()
    };
    item(&mut layout, "0").grow = 1.;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "0", 10., 37.5, 170., 85.);
    assert_slot(&scene, "1", 200., 65., 30., 30.);
    stable(&layout, &scene);
}

#[test]
fn column_reverse_uses_main_height_and_preserves_tree_order() {
    let (mut scene, mut layout) = fixture(200., 240., &[(10., 20.), (30., 30.)]);
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::ColumnReverse,
        padding: Insets::all(10.),
        gap: 20.,
        align_items: AlignItems::End,
        ..Default::default()
    };
    item(&mut layout, "0").grow = 1.;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "1", 160., 10., 30., 30.);
    assert_slot(&scene, "0", 105., 60., 85., 170.);
    assert_eq!(
        scene.get(&"root".into()).unwrap().children,
        vec!["0".into(), "1".into()]
    );
    stable(&layout, &scene);
}

#[test]
fn reverse_row_and_column_have_the_same_uniform_size_contract() {
    for direction in [
        FlowDirection::Row,
        FlowDirection::RowReverse,
        FlowDirection::Column,
        FlowDirection::ColumnReverse,
    ] {
        let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.), (20., 10.)]);
        style(&mut layout).direction = direction;
        for input in layout.items.values_mut() {
            input.grow = 1.;
        }
        layout.apply_to_scene(&mut scene).unwrap();
        let expected = if matches!(direction, FlowDirection::Row | FlowDirection::RowReverse) {
            Size::new(50., 25.)
        } else {
            Size::new(100., 50.)
        };
        for id in ["0", "1"] {
            assert_eq!(slot(&scene, id).size, expected);
        }
        stable(&layout, &scene);
    }
}

#[test]
fn capped_grow_redistributes_with_mixed_locked_and_unlocked_siblings() {
    let (mut scene, mut layout) = fixture(300., 200., &[(10., 5.), (20., 10.), (10., 7.)]);
    style(&mut layout).gap = 10.;
    for id in ["0", "1", "2"] {
        item(&mut layout, id).grow = 1.;
    }
    item(&mut layout, "0").max_width = Some(30.);
    item(&mut layout, "1").max_width = Some(50.);
    item(&mut layout, "2").lock_ratio = false;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "0", 0., 0., 30., 15.);
    assert_slot(&scene, "1", 40., 0., 50., 25.);
    assert_slot(&scene, "2", 100., 0., 200., 7.);
    stable(&layout, &scene);
}

#[test]
fn basis_and_main_range_can_enlarge_or_reduce_both_axes() {
    for (input, w, h) in [
        (
            FlowItem {
                basis: Some(60.),
                ..Default::default()
            },
            60.,
            30.,
        ),
        (
            FlowItem {
                min_width: Some(80.),
                ..Default::default()
            },
            80.,
            40.,
        ),
        (
            FlowItem {
                max_width: Some(10.),
                ..Default::default()
            },
            10.,
            5.,
        ),
    ] {
        let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.)]);
        *item(&mut layout, "0") = FlowItem {
            lock_ratio: true,
            ..input
        };
        layout.apply_to_scene(&mut scene).unwrap();
        assert_slot(&scene, "0", 0., 0., w, h);
        stable(&layout, &scene);
    }
}

#[test]
fn positive_nonuniform_transform_preserves_intrinsic_ratio_and_linear_matrix() {
    let (mut scene, mut layout) = fixture(200., 200., &[(20., 10.)]);
    scene
        .update_object(&"0".into(), |o| {
            o.transform = Affine::translation(9., 11.) * Affine::scale(2., 4.)
        })
        .unwrap();
    item(&mut layout, "0").grow = 1.;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "0", 0., 0., 200., 200.);
    assert_eq!(
        object(&scene, "0").shape.local_bounds().unwrap().size,
        Size::new(100., 50.)
    );
    assert_eq!(object(&scene, "0").transform, Affine::scale(2., 4.));
    stable(&layout, &scene);
}

#[test]
fn fixed_shape_kinds_resize_without_changing_other_properties_or_image_natural_size() {
    for shape in [
        Shape::from(display::Rect::new(20., 10.).rounded(3.)),
        Shape::from(display::Ellipse::new(20., 10.)),
        Shape::from(display::Image {
            natural_size: Some(Size::new(400., 300.)),
            ..display::Image::new("test.png", Size::new(20., 10.))
        }),
        Shape::from(display::Box::new(20., 10.)),
        Shape::from(display::Frame::new(20., 10.)),
    ] {
        let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.)]);
        scene
            .update_object(&"0".into(), |o| o.shape = shape.clone())
            .unwrap();
        item(&mut layout, "0").basis = Some(60.);
        layout.apply_to_scene(&mut scene).unwrap();
        assert_eq!(
            object(&scene, "0").shape.local_bounds().unwrap().size,
            Size::new(60., 30.)
        );
        if let Shape::Image(image) = &object(&scene, "0").shape {
            assert_eq!(image.natural_size, Some(Size::new(400., 300.)));
        }
        if let Shape::Rect(rect) = &object(&scene, "0").shape {
            assert_eq!(rect.corner_radius, 3.);
        }
        stable(&layout, &scene);
    }
}

#[test]
fn nested_fixed_container_resizes_before_its_child_arrangement() {
    let (mut scene, mut layout) = fixture(120., 120., &[(20., 10.)]);
    scene
        .update_object(&"0".into(), |o| {
            o.shape = display::Box::new(20., 10.).into()
        })
        .unwrap();
    scene
        .insert(
            DisplayObject::new("leaf", display::Rect::new(10., 10.)),
            Some("0".into()),
            None,
        )
        .unwrap();
    layout.containers.insert(
        "0".into(),
        FlowStyle {
            justify: JustifyContent::End,
            align_items: AlignItems::End,
            ..Default::default()
        },
    );
    item(&mut layout, "0").grow = 1.;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "0", 0., 0., 120., 60.);
    assert_slot(&scene, "leaf", 110., 50., 10., 10.);
    stable(&layout, &scene);
}

#[test]
fn hidden_keeps_ratio_slot_and_excluded_unsupported_text_is_untouched() {
    let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.), (20., 10.)]);
    scene
        .update_object(&"0".into(), |o| o.visible = false)
        .unwrap();
    scene
        .update_object(&"1".into(), |o| {
            o.shape = display::Text::new("x", Size::new(20., 10.)).into();
            o.transform = Affine::translation(300., 400.);
        })
        .unwrap();
    item(&mut layout, "0").grow = 1.;
    item(&mut layout, "1").in_flow = false;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "0", 0., 0., 100., 50.);
    assert_eq!(
        object(&scene, "1").transform,
        Affine::translation(300., 400.)
    );
    assert!(!object(&scene, "0").visible);
}

#[test]
fn auto_fit_spacing_and_grow_precedence_keep_existing_policy() {
    for mode in [GapMode::Fixed, GapMode::Auto, GapMode::Fit] {
        let (mut scene, mut layout) = fixture(200., 100., &[(20., 10.), (20., 10.)]);
        style(&mut layout).gap_mode = mode;
        item(&mut layout, "0").basis = Some(40.);
        layout.apply_to_scene(&mut scene).unwrap();
        assert_slot(&scene, "0", 0., 0., 40., 20.);
        assert_eq!(
            slot(&scene, "1").origin.x,
            if mode == GapMode::Fixed { 40. } else { 180. }
        );
        item(&mut layout, "0").grow = 1.;
        item(&mut layout, "0").max_width = Some(50.);
        layout.apply_to_scene(&mut scene).unwrap();
        assert_slot(&scene, "0", 0., 0., 50., 25.);
        assert_eq!(slot(&scene, "1").origin.x, 50.);
        stable(&layout, &scene);
    }
}

#[test]
fn position_only_wrap_reverse_preserves_geometry() {
    let (mut scene, mut layout) = fixture(35., 100., &[(20., 10.), (30., 15.), (25., 20.)]);
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::RowReverse,
        wrap: true,
        wrap_reverse: true,
        line_gap: 5.,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "0", 0., 0., 20., 10.);
    assert_slot(&scene, "1", 0., 15., 30., 15.);
    assert_slot(&scene, "2", 0., 35., 25., 20.);
    stable(&layout, &scene);
}

#[test]
fn wrapped_allocation_is_rejected_for_each_main_owner_even_if_current_size_matches() {
    for input in [
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
        FlowItem {
            basis: Some(20.),
            ..Default::default()
        },
        FlowItem {
            min_width: Some(20.),
            ..Default::default()
        },
        FlowItem {
            max_width: Some(20.),
            ..Default::default()
        },
    ] {
        let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.)]);
        style(&mut layout).wrap = true;
        *item(&mut layout, "0") = FlowItem {
            lock_ratio: true,
            ..input
        };
        rejected(&mut scene, &layout, "wrapping is unsupported");
    }
}

#[test]
fn cross_constraints_and_stretch_are_rejected_in_both_directions() {
    for direction in [FlowDirection::Row, FlowDirection::Column] {
        for cross in 0..3 {
            let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.)]);
            style(&mut layout).direction = direction;
            match cross {
                0 => style(&mut layout).align_items = AlignItems::Stretch,
                1 if direction == FlowDirection::Row => {
                    item(&mut layout, "0").min_height = Some(5.)
                }
                1 => item(&mut layout, "0").min_width = Some(5.),
                _ if direction == FlowDirection::Row => {
                    item(&mut layout, "0").max_height = Some(50.)
                }
                _ => item(&mut layout, "0").max_width = Some(50.),
            }
            rejected(&mut scene, &layout, "free cross axis");
        }
    }
}

#[test]
fn align_self_can_override_parent_stretch_without_disabling_ratio() {
    let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.)]);
    style(&mut layout).align_items = AlignItems::Stretch;
    item(&mut layout, "0").align_self = Some(AlignItems::End);
    item(&mut layout, "0").grow = 1.;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "0", 0., 50., 100., 50.);
}

#[test]
fn content_size_ownership_is_rejected_on_parent_or_locked_child() {
    for child in [false, true] {
        let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.)]);
        item(&mut layout, "0").basis = Some(40.);
        if child {
            scene
                .update_object(&"0".into(), |o| {
                    o.shape = display::Box::new(20., 10.).into()
                })
                .unwrap();
            layout.containers.insert(
                "0".into(),
                FlowStyle {
                    height_sizing: AxisSizing::content(),
                    ..Default::default()
                },
            );
        } else {
            style(&mut layout).height_sizing = AxisSizing::content();
        }
        rejected(
            &mut scene,
            &layout,
            if child {
                "content-sized container"
            } else {
                "fixed-size parent"
            },
        );
    }
}

#[test]
fn rotation_reflection_skew_and_zero_transforms_cannot_resize_a_locked_shape() {
    for transform in [
        Affine::rotation(30.),
        Affine::scale(-1., 1.),
        Affine {
            c: 0.2,
            ..Affine::IDENTITY
        },
        Affine::scale(0., 1.),
    ] {
        let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.)]);
        scene
            .update_object(&"0".into(), |o| o.transform = transform)
            .unwrap();
        item(&mut layout, "0").grow = 1.;
        rejected(&mut scene, &layout, "positive axis-aligned");
    }
}

#[test]
fn unsupported_text_and_zero_extent_line_do_not_invent_a_ratio() {
    for shape in [
        Shape::from(display::Text::new("x", Size::new(20., 10.))),
        Shape::from(display::Line::new(Point::ZERO, Point::new(20., 0.))),
    ] {
        let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.)]);
        scene
            .update_object(&"0".into(), |o| o.shape = shape.clone())
            .unwrap();
        item(&mut layout, "0").grow = 1.;
        let expected = if matches!(shape, Shape::Text(_)) {
            "text and custom"
        } else {
            "intrinsic finite positive"
        };
        rejected(&mut scene, &layout, expected);
    }
}

#[test]
fn stroke_box_resize_remains_rejected_but_position_only_stroke_is_allowed() {
    let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.)]);
    style(&mut layout).item_box = ItemBox::Stroke;
    scene
        .update_object(&"0".into(), |o| o.style = Style::stroke(Color::BLACK, 4.))
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(
        object(&scene, "0").shape.local_bounds().unwrap().size,
        Size::new(20., 10.)
    );
    item(&mut layout, "0").grow = 1.;
    rejected(&mut scene, &layout, "resizing visible stroke item boxes");
}

#[test]
fn exhausted_space_and_ratio_overflow_fail_before_scene_mutation() {
    let (mut scene, mut layout) = fixture(100., 100., &[(20., 10.), (100., 20.)]);
    item(&mut layout, "0").grow = 1.;
    rejected(&mut scene, &layout, "finite and positive on both axes");
    let (mut scene, mut layout) = fixture(2., 2., &[(1., f32::MAX)]);
    item(&mut layout, "0").grow = 1.;
    rejected(&mut scene, &layout, "finite and positive on both axes");
}

#[test]
fn huge_and_subnormal_inputs_keep_the_existing_f64_fallback() {
    for (main, w, h) in [(1e38, 1e30, 5e29), (8e-38, 2e-38, 1e-38)] {
        let (mut scene, mut layout) = fixture(main, main, &[(w, h)]);
        item(&mut layout, "0").grow = 1.;
        layout.apply_to_scene(&mut scene).unwrap();
        let size = slot(&scene, "0").size;
        assert_eq!(size.width, main);
        assert!((size.height / size.width - 0.5).abs() < 1e-6);
        stable(&layout, &scene);
    }
}

#[test]
fn fractional_transformed_ratio_is_stable_across_repeated_passes_and_width_reflows() {
    let (mut scene, mut layout) = fixture(103.7, 500., &[(13.2, 7.1)]);
    scene
        .update_object(&"0".into(), |o| o.transform = Affine::scale(1.3, 2.7))
        .unwrap();
    item(&mut layout, "0").grow = 1.;
    for width in [103.7, 219.3, 71.25, 103.7] {
        scene
            .update_object(&"root".into(), |o| {
                o.shape = display::Frame::new(width, 500.).into()
            })
            .unwrap();
        layout.apply_to_scene(&mut scene).unwrap();
        let size = object(&scene, "0").shape.local_bounds().unwrap().size;
        assert!((size.width / size.height - 13.2 / 7.1).abs() < 1e-5);
        for _ in 0..10 {
            stable(&layout, &scene);
        }
    }
}

#[test]
fn nonfinite_intrinsic_input_is_rejected_before_any_ratio_is_measured() {
    let (mut scene, layout) = fixture(100., 100., &[(20., 10.)]);
    let before = scene.clone();
    for invalid in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.] {
        assert!(
            scene
                .update_object(&"0".into(), |o| o.shape =
                    display::Rect::new(invalid, 10.).into())
                .is_err()
        );
        assert_eq!(scene, before);
    }
    assert!(layout.arranged(&scene).is_ok());
}

#[test]
fn taffy_aspect_ratio_with_two_definite_dimensions_is_not_a_uniform_resize_lock() {
    use taffy::prelude::{
        AvailableSpace, Dimension, FromLength, Size as TaffySize, Style as TaffyStyle, TaffyTree,
    };
    let mut tree: TaffyTree<()> = TaffyTree::new();
    tree.disable_rounding();
    let leaf = tree
        .new_leaf(TaffyStyle {
            size: TaffySize {
                width: Dimension::from_length(20.),
                height: Dimension::from_length(10.),
            },
            aspect_ratio: Some(2.),
            flex_grow: 1.,
            flex_shrink: 0.,
            ..Default::default()
        })
        .unwrap();
    let parent = tree
        .new_with_children(
            TaffyStyle {
                size: TaffySize {
                    width: Dimension::from_length(100.),
                    height: Dimension::from_length(100.),
                },
                ..Default::default()
            },
            &[leaf],
        )
        .unwrap();
    tree.compute_layout(
        parent,
        TaffySize {
            width: AvailableSpace::Definite(100.),
            height: AvailableSpace::Definite(100.),
        },
    )
    .unwrap();
    assert_eq!(
        tree.layout(leaf).unwrap().size,
        TaffySize {
            width: 100.,
            height: 10.
        }
    );
}
