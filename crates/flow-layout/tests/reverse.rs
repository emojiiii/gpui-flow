use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{
    CustomDefinition, DirtyFlags, ElementRegistry, PropertyDescriptor, PropertyKey, PropertySchema,
    SceneEngine,
};
use gpui_flow_layout::*;
use gpui_flow_scene::{Affine, Color, DisplayObject, Point, Scene, Size, Style, display};
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
    for (i, (w, h)) in sizes.iter().enumerate() {
        scene
            .insert(
                DisplayObject::new(i.to_string(), display::Rect::new(*w, *h))
                    .with_style(Style::fill(Color::BLACK)),
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
fn xy(scene: &Scene, id: &str) -> (f32, f32) {
    let o = &scene.get(&id.into()).unwrap().object;
    (o.transform.tx, o.transform.ty)
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
#[test]
fn reverse_visits_last_child_first_from_normal_start_edge_not_a_mirrored_axis() {
    let (mut scene, mut layout) = fixture(200., 100., &[(20., 30.), (40., 50.)]);
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::RowReverse,
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
    assert_eq!(xy(&scene, "1"), (110., 35.));
    assert_eq!(xy(&scene, "0"), (160., 45.));
    let (mut scene, mut layout) = fixture(100., 200., &[(20., 30.), (40., 50.)]);
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::ColumnReverse,
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
    assert_eq!(xy(&scene, "1"), (25., 100.));
    assert_eq!(xy(&scene, "0"), (35., 160.));
}
#[test]
fn both_axes_and_all_direction_wrap_reverse_combinations_have_exact_line_geometry() {
    let expected = [
        (false, false, [(10., 47.5), (65., 42.5), (30., 87.5)]),
        (false, true, [(10., 72.5), (65., 67.5), (30., 42.5)]),
        (true, false, [(65., 72.5), (10., 67.5), (30., 42.5)]),
        (true, true, [(65., 47.5), (10., 42.5), (30., 87.5)]),
    ];
    for row in [true, false] {
        for (reverse, wrap_reverse, positions) in expected {
            let sizes = if row {
                vec![(45., 20.), (45., 30.), (80., 10.)]
            } else {
                vec![(20., 45.), (30., 45.), (10., 80.)]
            };
            let (mut scene, mut layout) = fixture(
                if row { 120. } else { 140. },
                if row { 140. } else { 120. },
                &sizes,
            );
            *style(&mut layout) = FlowStyle {
                direction: match (row, reverse) {
                    (true, false) => FlowDirection::Row,
                    (true, true) => FlowDirection::RowReverse,
                    (false, false) => FlowDirection::Column,
                    (false, true) => FlowDirection::ColumnReverse,
                },
                wrap: true,
                wrap_reverse,
                padding: Insets::all(10.),
                gap: 10.,
                line_gap: 15.,
                justify: JustifyContent::End,
                align_items: AlignItems::Center,
                align_content: AlignContent::Center,
                ..Default::default()
            };
            layout.apply_to_scene(&mut scene).unwrap();
            for (i, (x, y)) in positions.into_iter().enumerate() {
                assert_eq!(
                    xy(&scene, &i.to_string()),
                    if row { (x, y) } else { (y, x) },
                    "row={row},reverse={reverse},wrap_reverse={wrap_reverse},item={i}"
                );
            }
            assert!(
                layout
                    .apply_to_scene(&mut scene)
                    .unwrap()
                    .changed
                    .is_empty()
            );
        }
    }
}
#[test]
fn grow_constraints_keep_sizes_and_reverse_only_placement_without_tree_or_paint_reorder() {
    let (mut scene, mut layout) = fixture(300., 80., &[(40., 20.), (40., 20.), (40., 20.)]);
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::RowReverse,
        padding: Insets::all(10.),
        gap: 10.,
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
    let children = scene.get(&"root".into()).unwrap().children.clone();
    let painted = scene
        .paint_list()
        .unwrap()
        .iter()
        .map(|p| p.object.id.clone())
        .collect::<Vec<_>>();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "0").width, 70.);
    assert_eq!(size(&scene, "1").width, 150.);
    assert_eq!(xy(&scene, "2"), (10., 10.));
    assert_eq!(xy(&scene, "1"), (60., 10.));
    assert_eq!(xy(&scene, "0"), (220., 10.));
    assert_eq!(scene.get(&"root".into()).unwrap().children, children);
    assert_eq!(
        scene
            .paint_list()
            .unwrap()
            .iter()
            .map(|p| p.object.id.clone())
            .collect::<Vec<_>>(),
        painted
    );
    assert_eq!(
        scene
            .hit_test(Point::new(15., 15.), Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "2"
    );
}
#[test]
fn hidden_items_keep_slots_and_excluded_items_keep_their_original_geometry() {
    let (mut scene, mut layout) = fixture(
        200.,
        100.,
        &[(20., 20.), (30., 20.), (40., 20.), (50., 20.)],
    );
    style(&mut layout).direction = FlowDirection::RowReverse;
    style(&mut layout).gap = 5.;
    scene
        .update_object(&"1".into(), |o| o.visible = false)
        .unwrap();
    scene
        .update_object(&"2".into(), |o| {
            o.transform = Affine::translation(700., 800.)
        })
        .unwrap();
    layout.items.insert(
        "2".into(),
        FlowItem {
            in_flow: false,
            ..Default::default()
        },
    );
    let excluded = scene.get(&"2".into()).unwrap().object.clone();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(xy(&scene, "3"), (0., 0.));
    assert_eq!(xy(&scene, "1"), (55., 0.));
    assert_eq!(xy(&scene, "0"), (90., 0.));
    assert_eq!(scene.get(&"2".into()).unwrap().object, excluded);
}
#[test]
fn auto_cross_measurement_uses_reversed_wrapping_groups_and_wrap_reverse_keeps_size() {
    let (mut normal, mut layout) = fixture(100., 500., &[(60., 10.), (40., 100.), (40., 20.)]);
    *style(&mut layout) = FlowStyle {
        wrap: true,
        height_sizing: AxisSizing::content(),
        line_gap: 5.,
        ..Default::default()
    };
    let mut reverse = normal.clone();
    layout.apply_to_scene(&mut normal).unwrap();
    assert_eq!(size(&normal, "root").height, 125.);
    style(&mut layout).direction = FlowDirection::RowReverse;
    layout.apply_to_scene(&mut reverse).unwrap();
    assert_eq!(size(&reverse, "root").height, 115.);
    assert_eq!(xy(&reverse, "0"), (0., 105.));
    style(&mut layout).wrap_reverse = true;
    layout.apply_to_scene(&mut reverse).unwrap();
    assert_eq!(size(&reverse, "root").height, 115.);
    assert_eq!(xy(&reverse, "0"), (0., 0.));
    assert_eq!(xy(&reverse, "2"), (0., 15.));
    assert!(layout.arranged(&reverse).unwrap().1.changed.is_empty());
}
#[test]
fn auto_main_still_disables_wrapping_and_directional_ownership_checks_apply_to_reverse_axes() {
    for direction in [FlowDirection::RowReverse, FlowDirection::ColumnReverse] {
        let row = direction == FlowDirection::RowReverse;
        let (mut scene, mut layout) = fixture(40., 40., &[(20., 20.), (30., 30.)]);
        *style(&mut layout) = FlowStyle {
            direction,
            wrap: true,
            wrap_reverse: true,
            width_sizing: if row {
                AxisSizing::content()
            } else {
                AxisSizing::Fixed
            },
            height_sizing: if row {
                AxisSizing::Fixed
            } else {
                AxisSizing::content()
            },
            ..Default::default()
        };
        layout.apply_to_scene(&mut scene).unwrap();
        assert_eq!(xy(&scene, "1"), (0., 0.));
        assert_eq!(xy(&scene, "0"), if row { (30., 0.) } else { (0., 30.) });
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
        layout.items.clear();
        let s = style(&mut layout);
        s.width_sizing = if row {
            AxisSizing::Fixed
        } else {
            AxisSizing::content()
        };
        s.height_sizing = if row {
            AxisSizing::content()
        } else {
            AxisSizing::Fixed
        };
        s.align_items = AlignItems::Stretch;
        assert!(layout.apply_to_scene(&mut scene).is_err());
        assert_eq!(scene, before);
    }
}
#[test]
fn transformed_reverse_items_preserve_linear_transform_and_use_measured_boxes() {
    let (mut scene, mut layout) = fixture(300., 100., &[(20., 30.), (40., 10.)]);
    let rotation = Affine::rotation(90.);
    scene
        .update_object(&"1".into(), |o| o.transform = rotation)
        .unwrap();
    style(&mut layout).direction = FlowDirection::RowReverse;
    style(&mut layout).gap = 5.;
    layout.apply_to_scene(&mut scene).unwrap();
    let o = &scene.get(&"1".into()).unwrap().object;
    let bounds = o
        .transform
        .transform_bounds(o.shape.local_bounds().unwrap());
    assert!(bounds.origin.x.abs() < 1e-4);
    assert!(bounds.origin.y.abs() < 1e-4);
    assert!((xy(&scene, "0").0 - 15.).abs() < 1e-3);
    assert_eq!(
        (o.transform.a, o.transform.b, o.transform.c, o.transform.d),
        (rotation.a, rotation.b, rotation.c, rotation.d)
    );
}
#[test]
fn reverse_configuration_roundtrips_and_old_json_uses_forward_defaults() {
    let old: FlowStyle = serde_json::from_value(json!({"direction":"row","wrap":true})).unwrap();
    assert!(!old.wrap_reverse);
    for direction in [FlowDirection::RowReverse, FlowDirection::ColumnReverse] {
        let (scene, mut layout) = fixture(100., 100., &[(20., 20.)]);
        style(&mut layout).direction = direction;
        style(&mut layout).wrap_reverse = true;
        let doc = FlowDocument { scene, layout };
        assert_eq!(
            FlowDocument::from_json(&doc.to_json().unwrap()).unwrap(),
            doc
        );
    }
    assert!(serde_json::from_value::<FlowStyle>(json!({"direction":"x-backwards"})).is_err());
}
#[test]
fn reactive_reverse_tree_edits_are_one_undo_and_keep_provider_fixed_point() {
    let (mut scene, mut layout) = fixture(100., 80., &[(60., 10.), (40., 30.), (40., 20.)]);
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::RowReverse,
        wrap: true,
        wrap_reverse: true,
        height_sizing: AxisSizing::content(),
        line_gap: 5.,
        ..Default::default()
    };
    layout.apply_to_scene(&mut scene).unwrap();
    let mut e = SceneEditor::new(scene).unwrap();
    layout.install_reactive(&mut e.engine).unwrap();
    let before = e.scene().clone();
    let revision = e.engine.revision();
    e.transaction(|s| s.reparent(&"0".into(), Some("root".into()), Some(2), false))
        .unwrap();
    assert_eq!(e.engine.revision(), revision + 1);
    assert!(layout.arranged(e.scene()).unwrap().1.changed.is_empty());
    let after = e.scene().clone();
    assert_ne!(after, before);
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(e.scene(), &after);
}
#[test]
fn reverse_custom_geometry_changes_reflow_intrinsic_height_with_single_history_step() {
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
                    DirtyFlags::GEOMETRY | DirtyFlags::PAINT,
                )],
                allow_unknown: false,
            },
            |data: &serde_json::Value| {
                let shape = display::Ellipse::circle(data["radius"].as_f64().unwrap() as f32);
                Ok((shape.bounds(), shape.flatten(0.1)))
            },
        ))
        .unwrap();
    let (scene, mut layout) = fixture(80., 10., &[]);
    let mut engine = SceneEngine::with_registry(scene, registry).unwrap();
    let root = engine.handle(&"root".into()).unwrap();
    engine
        .create("disc", "a", json!({}), Some(root), None)
        .unwrap();
    engine
        .create("disc", "b", json!({"radius":10.}), Some(root), None)
        .unwrap();
    *style(&mut layout) = FlowStyle {
        direction: FlowDirection::RowReverse,
        height_sizing: AxisSizing::content(),
        wrap: true,
        wrap_reverse: true,
        ..Default::default()
    };
    layout.apply_to_engine(&mut engine).unwrap();
    layout.install_reactive(&mut engine).unwrap();
    let mut e = SceneEditor::from_engine(engine);
    let before = e.scene().clone();
    e.set_property(
        e.engine.handle(&"a".into()).unwrap(),
        PropertyKey::Custom("radius".into()),
        json!(15.),
    )
    .unwrap();
    assert_eq!(size(e.scene(), "root").height, 30.);
    assert_eq!(xy(e.scene(), "b"), (0., 0.));
    assert_eq!(xy(e.scene(), "a"), (20., 0.));
    assert!(layout.arranged(e.scene()).unwrap().1.changed.is_empty());
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
}

#[test]
fn nested_reverse_content_dimensions_preserve_parent_axis_ownership() {
    for row in [true, false] {
        let (mut scene, mut layout) = fixture(
            if row { 180. } else { 200. },
            if row { 200. } else { 180. },
            &[(40., 40.)],
        );
        scene
            .insert(
                DisplayObject::new(
                    "inner",
                    display::Frame::new(if row { 100. } else { 5. }, if row { 5. } else { 100. }),
                ),
                Some("root".into()),
                Some(0),
            )
            .unwrap();
        for (id, extent) in [("i0", 20.), ("i1", 50.)] {
            scene
                .insert(
                    DisplayObject::new(
                        id,
                        display::Rect::new(
                            if row { 30. } else { extent },
                            if row { extent } else { 30. },
                        ),
                    ),
                    Some("inner".into()),
                    None,
                )
                .unwrap();
        }
        *style(&mut layout) = FlowStyle {
            direction: if row {
                FlowDirection::RowReverse
            } else {
                FlowDirection::ColumnReverse
            },
            gap: 10.,
            width_sizing: if row {
                AxisSizing::Fixed
            } else {
                AxisSizing::content()
            },
            height_sizing: if row {
                AxisSizing::content()
            } else {
                AxisSizing::Fixed
            },
            ..Default::default()
        };
        layout.containers.insert(
            "inner".into(),
            FlowStyle {
                direction: if row {
                    FlowDirection::ColumnReverse
                } else {
                    FlowDirection::RowReverse
                },
                width_sizing: if row {
                    AxisSizing::Fixed
                } else {
                    AxisSizing::content()
                },
                height_sizing: if row {
                    AxisSizing::content()
                } else {
                    AxisSizing::Fixed
                },
                ..Default::default()
            },
        );
        layout.items.insert(
            "inner".into(),
            FlowItem {
                basis: Some(100.),
                ..Default::default()
            },
        );
        layout.apply_to_scene(&mut scene).unwrap();
        assert_eq!(
            size(&scene, "root"),
            if row {
                Size::new(180., 70.)
            } else {
                Size::new(70., 180.)
            }
        );
        assert_eq!(xy(&scene, "inner"), if row { (50., 0.) } else { (0., 50.) });
        assert_eq!(xy(&scene, "i0"), if row { (0., 50.) } else { (50., 0.) });
        assert!(layout.arranged(&scene).unwrap().1.changed.is_empty());
        let before = scene.clone();
        let inner = layout.containers.get_mut(&"inner".into()).unwrap();
        if row {
            inner.width_sizing = AxisSizing::content();
        } else {
            inner.height_sizing = AxisSizing::content();
        }
        assert!(layout.apply_to_scene(&mut scene).is_err());
        assert_eq!(scene, before);
    }
}
