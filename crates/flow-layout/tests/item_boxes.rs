//! Source-derived golden slots: pinned leafer-in 3eb445bf common/bounds.ts
//! and flowX/flowY simple placement select stroke bounds and subtract origins.
//! These are contract fixtures, not execution of a combined upstream build.
use gpui_flow_editor::SceneEditor;
use gpui_flow_layout::*;
use gpui_flow_scene::{
    Affine, Bounds, Color, DisplayObject, Point, Scene, Shape, Size, Stroke, StrokeCap, StrokeJoin,
    Style, display,
};
use serde_json::json;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

fn fixture(width: f32, height: f32) -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Frame::new(width, height)),
            None,
            None,
        )
        .unwrap();
    let mut layout = FlowLayout::default();
    layout.containers.insert(
        "root".into(),
        FlowStyle {
            item_box: ItemBox::Stroke,
            ..Default::default()
        },
    );
    (scene, layout)
}
fn add(scene: &mut Scene, id: &str, width: f32, height: f32, stroke: f32) {
    scene
        .insert(
            DisplayObject::new(id, display::Rect::new(width, height))
                .with_style(Style::stroke(Color::BLACK, stroke)),
            Some("root".into()),
            None,
        )
        .unwrap();
}
fn style(layout: &mut FlowLayout) -> &mut FlowStyle {
    layout.containers.get_mut(&"root".into()).unwrap()
}
fn object<'a>(scene: &'a Scene, id: &str) -> &'a DisplayObject {
    &scene.get(&id.into()).unwrap().object
}
fn slot(scene: &Scene, id: &str) -> Bounds {
    let object = object(scene, id);
    object
        .transform
        .transform_bounds(object.paint_bounds().unwrap())
}
fn assert_slot(scene: &Scene, id: &str, x: f32, y: f32, w: f32, h: f32) {
    assert_eq!(
        slot(scene, id),
        Bounds::new(Point::new(x, y), Size::new(w, h)),
        "{id}"
    );
}
fn assert_stable(layout: &FlowLayout, scene: &Scene) {
    let (again, report) = layout.arranged(scene).unwrap();
    assert_eq!(again, *scene);
    assert!(report.changed.is_empty());
}

#[test]
fn default_box_mode_retains_old_stroke_agnostic_positions_and_json() {
    let old: FlowStyle = serde_json::from_value(json!({"gap": 5.})).unwrap();
    assert_eq!(old.item_box, ItemBox::Box);
    assert!(
        serde_json::to_value(&old)
            .unwrap()
            .get("item_box")
            .is_none()
    );
    let (mut scene, mut layout) = fixture(200., 80.);
    *style(&mut layout) = old;
    add(&mut scene, "a", 20., 10., 10.);
    add(&mut scene, "b", 30., 20., 4.);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(object(&scene, "a").transform.tx, 0.);
    assert_eq!(object(&scene, "b").transform.tx, 25.);
}

#[test]
fn stroke_mode_round_trips_and_rejects_unknown_literals() {
    let (scene, layout) = fixture(100., 80.);
    assert_eq!(
        serde_json::to_value(style(&mut layout.clone())).unwrap()["item_box"],
        "stroke"
    );
    let document = FlowDocument { scene, layout };
    assert_eq!(
        FlowDocument::from_json(&document.to_json().unwrap()).unwrap(),
        document
    );
    for invalid in [
        json!({"item_box":"render"}),
        json!({"item_box":"Stroke"}),
        json!({"item_box":true}),
    ] {
        assert!(serde_json::from_value::<FlowStyle>(invalid).is_err());
    }
}

#[test]
fn centered_rect_stroke_golden_places_outer_edges_and_preserves_content() {
    let (mut scene, mut layout) = fixture(200., 80.);
    style(&mut layout).padding = Insets::all(10.);
    style(&mut layout).gap = 5.;
    add(&mut scene, "a", 20., 10., 4.);
    add(&mut scene, "b", 30., 20., 10.);
    let before_a = object(&scene, "a").shape.clone();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 10., 10., 24., 14.);
    assert_slot(&scene, "b", 39., 10., 40., 30.);
    assert_eq!(object(&scene, "a").transform.tx, 12.);
    assert_eq!(object(&scene, "b").transform.tx, 44.);
    assert_eq!(object(&scene, "a").shape, before_a);
    assert_stable(&layout, &scene);
}

#[test]
fn column_stroke_golden_swaps_axes_and_aligns_outer_edges() {
    let (mut scene, mut layout) = fixture(100., 200.);
    *style(&mut layout) = FlowStyle {
        item_box: ItemBox::Stroke,
        direction: FlowDirection::Column,
        gap: 6.,
        padding: Insets::all(10.),
        align_items: AlignItems::End,
        ..Default::default()
    };
    add(&mut scene, "a", 20., 10., 4.);
    add(&mut scene, "b", 30., 20., 10.);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 66., 10., 24., 14.);
    assert_slot(&scene, "b", 50., 30., 40., 30.);
}

#[test]
fn layered_strokes_use_largest_visible_extent() {
    let (mut scene, layout) = fixture(200., 80.);
    add(&mut scene, "a", 20., 10., 2.);
    scene
        .update_object(&"a".into(), |o| {
            o.style.stroke_layers = vec![
                Stroke::new(Color::BLACK, 8.),
                Stroke::new(Color::TRANSPARENT, 80.),
            ];
        })
        .unwrap();
    add(&mut scene, "b", 20., 10., 0.);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 0., 0., 28., 18.);
    assert_slot(&scene, "b", 28., 0., 20., 10.);
}

#[test]
fn transparent_and_zero_strokes_do_not_expand_or_block_grow() {
    for (color, width) in [(Color::TRANSPARENT, 80.), (Color::BLACK, 0.)] {
        let (mut scene, mut layout) = fixture(100., 80.);
        add(&mut scene, "a", 20., 10., 0.);
        scene
            .update_object(&"a".into(), |o| o.style = Style::stroke(color, width))
            .unwrap();
        layout.items.insert(
            "a".into(),
            FlowItem {
                grow: 1.,
                ..Default::default()
            },
        );
        layout.apply_to_scene(&mut scene).unwrap();
        assert_slot(&scene, "a", 0., 0., 100., 10.);
        assert_stable(&layout, &scene);
    }
}

#[test]
fn opacity_and_visibility_do_not_remove_geometric_stroke_slots() {
    let (mut scene, layout) = fixture(200., 80.);
    add(&mut scene, "a", 20., 10., 4.);
    add(&mut scene, "b", 20., 10., 4.);
    scene
        .update_object(&"a".into(), |o| {
            o.visible = false;
            o.style.opacity = 0.;
        })
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "b", 24., 0., 24., 14.);
}

#[test]
fn advanced_caps_joins_and_dashes_reuse_conservative_scene_bounds() {
    for stroke in [
        Stroke::new(Color::BLACK, 4.).with_cap(StrokeCap::Butt),
        Stroke::new(Color::BLACK, 4.).with_cap(StrokeCap::Square),
        Stroke::new(Color::BLACK, 4.).with_join(StrokeJoin::Bevel),
        Stroke::new(Color::BLACK, 4.)
            .with_join(StrokeJoin::Miter)
            .with_miter_limit(2.),
        Stroke::new(Color::BLACK, 4.).with_dashes(vec![3., 9., 2.], -5.),
    ] {
        let (mut scene, layout) = fixture(500., 100.);
        let line = display::Line::new(Point::new(-10., -5.), Point::new(30., 20.));
        let styled = DisplayObject::new("a", line).with_style(Style {
            stroke: Some(stroke.clone()),
            ..Default::default()
        });
        let expected = styled.paint_bounds().unwrap();
        let mesh = styled.shape.stroke_mesh(&stroke).unwrap();
        assert!(
            mesh.triangles()
                .iter()
                .flatten()
                .all(|p| expected.contains(*p))
        );
        scene.insert(styled, Some("root".into()), None).unwrap();
        add(&mut scene, "b", 20., 10., 0.);
        layout.apply_to_scene(&mut scene).unwrap();
        assert_eq!(slot(&scene, "a").size, expected.size);
        assert_eq!(slot(&scene, "a").origin, Point::ZERO);
        assert_eq!(slot(&scene, "b").origin.x, expected.size.width);
        assert_stable(&layout, &scene);
    }
}

#[test]
fn dashed_gap_does_not_collapse_a_slot_to_the_painted_mesh() {
    let (mut scene, layout) = fixture(200., 100.);
    let stroke = Stroke::new(Color::BLACK, 4.)
        .with_cap(StrokeCap::Butt)
        .with_dashes(vec![1., 99.], 0.);
    let o = DisplayObject::new("a", display::Line::new(Point::ZERO, Point::new(100., 0.)))
        .with_style(Style {
            stroke: Some(stroke.clone()),
            ..Default::default()
        });
    assert!(
        o.shape
            .stroke_mesh(&stroke)
            .unwrap()
            .bounds()
            .unwrap()
            .size
            .width
            < 10.
    );
    scene.insert(o, Some("root".into()), None).unwrap();
    add(&mut scene, "b", 20., 10., 0.);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 0., 0., 104., 4.);
    assert_slot(&scene, "b", 104., 0., 20., 10.);
}

#[test]
fn negative_geometry_origins_and_affine_transforms_use_linear_stroke_aabb() {
    for transform in [
        Affine {
            a: 2.,
            d: 3.,
            ..Affine::IDENTITY
        },
        Affine {
            a: 0.,
            b: 1.,
            c: -1.,
            d: 0.,
            ..Affine::IDENTITY
        },
        Affine {
            a: -2.,
            d: 1.,
            ..Affine::IDENTITY
        },
        Affine {
            a: 1.,
            b: 0.5,
            c: 0.25,
            d: 1.,
            ..Affine::IDENTITY
        },
    ] {
        let (mut scene, mut layout) = fixture(500., 500.);
        style(&mut layout).padding = Insets::all(7.);
        let o = DisplayObject::new(
            "a",
            display::Line::new(Point::new(-10., -5.), Point::new(20., 10.)),
        )
        .with_style(Style::stroke(Color::BLACK, 4.))
        .with_transform(Affine {
            tx: 80.,
            ty: -90.,
            ..transform
        });
        let expected = transform.transform_bounds(o.paint_bounds().unwrap());
        scene.insert(o, Some("root".into()), None).unwrap();
        add(&mut scene, "b", 10., 10., 0.);
        layout.apply_to_scene(&mut scene).unwrap();
        assert_eq!(slot(&scene, "a").origin, Point::new(7., 7.));
        assert_eq!(slot(&scene, "a").size, expected.size);
        assert_eq!(slot(&scene, "b").origin.x, 7. + expected.size.width);
        let actual = object(&scene, "a").transform;
        assert_eq!(
            (actual.a, actual.b, actual.c, actual.d),
            (transform.a, transform.b, transform.c, transform.d)
        );
        assert_stable(&layout, &scene);
    }
}

#[test]
fn ancestor_transform_and_clipping_never_change_local_slots() {
    let (mut scene, layout) = fixture(10., 10.);
    add(&mut scene, "a", 40., 30., 8.);
    add(&mut scene, "b", 20., 10., 4.);
    let transform = Affine {
        a: -2.,
        b: 0.5,
        c: 0.25,
        d: 3.,
        tx: 50.,
        ty: 60.,
    };
    scene
        .update_object(&"root".into(), |o| o.transform = transform)
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 0., 0., 48., 38.);
    assert_slot(&scene, "b", 48., 0., 24., 14.);
    assert_eq!(object(&scene, "root").transform, transform);
}

#[test]
fn nested_box_and_frame_slots_ignore_descendant_overflow_and_child_clips() {
    for shape in [
        Shape::from(display::Box::new(20., 10.)),
        Shape::from(display::Frame::new(20., 10.)),
    ] {
        let (mut scene, layout) = fixture(400., 200.);
        scene
            .insert(
                DisplayObject::new("a", shape).with_style(Style::stroke(Color::BLACK, 4.)),
                Some("root".into()),
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new("overflow", display::Rect::new(200., 100.)).with_transform(
                    Affine {
                        tx: 50.,
                        ..Affine::IDENTITY
                    },
                ),
                Some("a".into()),
                None,
            )
            .unwrap();
        add(&mut scene, "b", 10., 10., 0.);
        layout.apply_to_scene(&mut scene).unwrap();
        assert_slot(&scene, "a", 0., 0., 24., 14.);
        assert_slot(&scene, "b", 24., 0., 10., 10.);
        assert_eq!(object(&scene, "overflow").transform.tx, 50.);
    }
}

#[test]
fn stroke_width_changes_wrap_threshold_and_reverse_preserves_tree_order() {
    let (mut scene, mut layout) = fixture(50., 100.);
    *style(&mut layout) = FlowStyle {
        item_box: ItemBox::Stroke,
        direction: FlowDirection::RowReverse,
        wrap: true,
        wrap_reverse: true,
        line_gap: 3.,
        ..Default::default()
    };
    for id in ["a", "b", "c"] {
        add(&mut scene, id, 20., 10., 8.);
    }
    let children = scene.get(&"root".into()).unwrap().children.clone();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 0., 0., 28., 18.);
    assert_slot(&scene, "b", 0., 21., 28., 18.);
    assert_slot(&scene, "c", 0., 42., 28., 18.);
    assert_eq!(scene.get(&"root".into()).unwrap().children, children);
}

#[test]
fn column_reverse_wrap_swaps_complete_stroke_lines() {
    let (mut scene, mut layout) = fixture(100., 50.);
    *style(&mut layout) = FlowStyle {
        item_box: ItemBox::Stroke,
        direction: FlowDirection::ColumnReverse,
        wrap: true,
        wrap_reverse: true,
        line_gap: 3.,
        ..Default::default()
    };
    for id in ["a", "b", "c"] {
        add(&mut scene, id, 10., 20., 8.);
    }
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 0., 0., 18., 28.);
    assert_slot(&scene, "b", 21., 0., 18., 28.);
    assert_slot(&scene, "c", 42., 0., 18., 28.);
}

#[test]
fn auto_and_fit_gaps_distribute_outer_stroke_boxes() {
    for (width, mode, next_x) in [(100., GapMode::Auto, 72.), (40., GapMode::Fit, 12.)] {
        let (mut scene, mut layout) = fixture(width, 50.);
        style(&mut layout).gap_mode = mode;
        for id in ["a", "b"] {
            add(&mut scene, id, 20., 10., 8.);
        }
        layout.apply_to_scene(&mut scene).unwrap();
        assert_slot(&scene, "a", 0., 0., 28., 18.);
        assert_slot(&scene, "b", next_x, 0., 28., 18.);
        assert_stable(&layout, &scene);
    }
}

#[test]
fn auto_line_gaps_use_stroke_line_heights() {
    let (mut scene, mut layout) = fixture(30., 100.);
    style(&mut layout).wrap = true;
    style(&mut layout).line_gap_mode = GapMode::Auto;
    for id in ["a", "b", "c"] {
        add(&mut scene, id, 20., 10., 8.);
    }
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 0., 0., 28., 18.);
    assert_slot(&scene, "b", 0., 41., 28., 18.);
    assert_slot(&scene, "c", 0., 82., 28., 18.);
}

#[test]
fn content_parent_includes_child_stroke_once_but_not_its_own_stroke() {
    let (mut scene, mut layout) = fixture(200., 80.);
    *style(&mut layout) = FlowStyle {
        item_box: ItemBox::Stroke,
        width_sizing: AxisSizing::content(),
        height_sizing: AxisSizing::content(),
        padding: Insets::all(3.),
        gap: 5.,
        ..Default::default()
    };
    scene
        .update_object(&"root".into(), |o| {
            o.style = Style::stroke(Color::BLACK, 10.)
        })
        .unwrap();
    add(&mut scene, "a", 20., 10., 4.);
    add(&mut scene, "b", 30., 20., 10.);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(
        object(&scene, "root").shape.local_bounds().unwrap().size,
        Size::new(75., 36.)
    );
    assert_eq!(
        object(&scene, "a").shape.local_bounds().unwrap().size,
        Size::new(20., 10.)
    );
    assert_stable(&layout, &scene);
}

#[test]
fn nested_content_measurement_keeps_parent_and_child_strokes_separate() {
    let (mut scene, mut layout) = fixture(100., 100.);
    style(&mut layout).width_sizing = AxisSizing::content();
    style(&mut layout).height_sizing = AxisSizing::content();
    scene
        .insert(
            DisplayObject::new("inner", display::Box::new(100., 100.))
                .with_style(Style::stroke(Color::BLACK, 8.)),
            Some("root".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("leaf", display::Rect::new(20., 10.))
                .with_style(Style::stroke(Color::BLACK, 4.)),
            Some("inner".into()),
            None,
        )
        .unwrap();
    layout.containers.insert(
        "inner".into(),
        FlowStyle {
            item_box: ItemBox::Stroke,
            width_sizing: AxisSizing::content(),
            height_sizing: AxisSizing::content(),
            padding: Insets::all(3.),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(
        object(&scene, "inner").shape.local_bounds().unwrap().size,
        Size::new(30., 20.)
    );
    assert_eq!(
        object(&scene, "root").shape.local_bounds().unwrap().size,
        Size::new(38., 28.)
    );
    assert_stable(&layout, &scene);
}

#[test]
fn excluded_stroked_native_items_keep_position_and_do_not_fail() {
    let (mut scene, mut layout) = fixture(100., 100.);
    let text = DisplayObject::new("text", display::Text::new("x", Size::new(20., 10.)))
        .with_style(Style::stroke(Color::BLACK, 4.))
        .with_transform(Affine {
            tx: 90.,
            ty: 80.,
            ..Affine::IDENTITY
        });
    scene
        .insert(text.clone(), Some("root".into()), None)
        .unwrap();
    layout.items.insert(
        "text".into(),
        FlowItem {
            in_flow: false,
            ..Default::default()
        },
    );
    add(&mut scene, "a", 20., 10., 4.);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(object(&scene, "text"), &text);
    assert_slot(&scene, "a", 0., 0., 24., 14.);
}

#[test]
fn native_text_and_image_visible_stroke_boxes_fail_before_mutation() {
    for shape in [
        Shape::from(display::Text::new("x", Size::new(20., 10.))),
        Shape::from(display::Image::new("local.png", Size::new(20., 10.))),
    ] {
        let (mut scene, layout) = fixture(100., 100.);
        scene
            .insert(
                DisplayObject::new("native", shape).with_style(Style::stroke(Color::BLACK, 4.)),
                Some("root".into()),
                None,
            )
            .unwrap();
        let before = scene.clone();
        assert!(
            layout
                .validate(&scene)
                .unwrap_err()
                .to_string()
                .contains("native renderer")
        );
        assert!(layout.apply_to_scene(&mut scene).is_err());
        assert_eq!(scene, before);
    }
}

#[test]
fn group_still_requires_explicit_exclusion() {
    let (mut scene, mut layout) = fixture(100., 100.);
    scene
        .insert(
            DisplayObject::new("group", display::Group::default()),
            Some("root".into()),
            None,
        )
        .unwrap();
    let before = scene.clone();
    assert!(
        layout
            .apply_to_scene(&mut scene)
            .unwrap_err()
            .to_string()
            .contains("no intrinsic box")
    );
    assert_eq!(scene, before);
    layout.items.insert(
        "group".into(),
        FlowItem {
            in_flow: false,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
}

#[test]
fn visible_stroke_resize_requests_fail_atomically_for_all_allocation_paths() {
    for input in [
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
        FlowItem {
            basis: Some(50.),
            ..Default::default()
        },
        FlowItem {
            min_width: Some(50.),
            ..Default::default()
        },
        FlowItem {
            max_width: Some(10.),
            ..Default::default()
        },
        FlowItem {
            min_height: Some(50.),
            ..Default::default()
        },
        FlowItem {
            max_height: Some(5.),
            ..Default::default()
        },
        FlowItem {
            align_self: Some(AlignItems::Stretch),
            ..Default::default()
        },
    ] {
        let (mut scene, mut layout) = fixture(100., 100.);
        add(&mut scene, "first", 10., 10., 0.);
        add(&mut scene, "stroke", 20., 10., 4.);
        layout.items.insert("stroke".into(), input);
        let before = scene.clone();
        let mut editor = SceneEditor::new(scene).unwrap();
        let revision = editor.engine.revision();
        let error = layout.apply_to_editor(&mut editor).unwrap_err();
        assert!(
            error.to_string().contains("resizing visible stroke"),
            "{error}"
        );
        assert_eq!(editor.scene(), &before);
        assert_eq!(editor.engine.revision(), revision);
        assert!(!editor.undo());
    }
}

#[test]
fn equal_size_constraints_and_stretch_do_not_need_stroke_resizing() {
    let (mut scene, mut layout) = fixture(24., 14.);
    add(&mut scene, "a", 20., 10., 4.);
    layout.items.insert(
        "a".into(),
        FlowItem {
            grow: 1.,
            basis: Some(24.),
            min_width: Some(24.),
            max_width: Some(24.),
            align_self: Some(AlignItems::Stretch),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 0., 0., 24., 14.);
    assert_stable(&layout, &scene);
}

#[test]
fn mixed_stroked_fixed_and_unstroked_grow_use_remaining_slot_space() {
    let (mut scene, mut layout) = fixture(100., 40.);
    add(&mut scene, "a", 20., 10., 4.);
    add(&mut scene, "b", 20., 10., 0.);
    style(&mut layout).gap = 6.;
    layout.items.insert(
        "b".into(),
        FlowItem {
            grow: 1.,
            align_self: Some(AlignItems::Stretch),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 0., 0., 24., 14.);
    assert_slot(&scene, "b", 30., 0., 70., 40.);
    assert_stable(&layout, &scene);
}

#[derive(Default)]
struct Metrics {
    requests: Mutex<Vec<TextMeasureRequest>>,
}
impl TextMeasurer for Metrics {
    fn measure(&self, request: &TextMeasureRequest) -> Result<TextMetrics, String> {
        self.requests.lock().unwrap().push(request.clone());
        let lines = if request.wrap_width.is_some_and(|width| width < 60.) {
            2
        } else {
            1
        };
        Ok(TextMetrics {
            width: request.wrap_width.unwrap_or(60.).min(60.),
            height: request.line_height * lines as f32,
            line_height: request.line_height,
            lines,
        })
    }
}

#[test]
fn natural_text_keeps_measured_content_axes_beside_stroke_slots() {
    let (mut scene, mut layout) = fixture(200., 100.);
    add(&mut scene, "a", 20., 10., 4.);
    let mut text = display::Text::new("fixture", Size::new(1., 1.));
    text.wrap = false;
    text.font_size = 10.;
    text.line_height = 2.;
    scene
        .insert(DisplayObject::new("text", text), Some("root".into()), None)
        .unwrap();
    layout.texts.insert("text".into(), TextSizing::Natural);
    style(&mut layout).width_sizing = AxisSizing::content();
    style(&mut layout).height_sizing = AxisSizing::content();
    let backend = Arc::new(Metrics::default());
    let measured = layout.with_text_measurer(backend.clone());
    measured.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "text", 24., 0., 60., 20.);
    assert_eq!(
        object(&scene, "root").shape.local_bounds().unwrap().size,
        Size::new(84., 20.)
    );
    assert!(
        backend
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|r| r.wrap_width.is_none())
    );
    assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn auto_height_text_wraps_at_content_width_remaining_after_sibling_stroke() {
    let (mut scene, mut layout) = fixture(80., 100.);
    add(&mut scene, "a", 20., 10., 4.);
    let mut text = display::Text::new("fixture", Size::new(80., 1.));
    text.font_size = 10.;
    text.line_height = 2.;
    scene
        .insert(DisplayObject::new("text", text), Some("root".into()), None)
        .unwrap();
    layout.texts.insert("text".into(), TextSizing::AutoHeight);
    layout.items.insert(
        "text".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    style(&mut layout).height_sizing = AxisSizing::content();
    let backend = Arc::new(Metrics::default());
    let measured = layout.with_text_measurer(backend.clone());
    measured.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "text", 24., 0., 56., 40.);
    assert_eq!(
        object(&scene, "root")
            .shape
            .local_bounds()
            .unwrap()
            .size
            .height,
        40.
    );
    assert_eq!(
        backend.requests.lock().unwrap().last().unwrap().wrap_width,
        Some(56.)
    );
    assert!(measured.arranged(&scene).unwrap().1.changed.is_empty());
}

#[test]
fn reactive_stroke_edit_reflows_once_and_undo_redo_keep_handles() {
    let (mut scene, layout) = fixture(200., 50.);
    add(&mut scene, "a", 20., 10., 4.);
    add(&mut scene, "b", 20., 10., 4.);
    layout.apply_to_scene(&mut scene).unwrap();
    let before = scene.clone();
    let mut editor = SceneEditor::new(scene).unwrap();
    layout.install_reactive(&mut editor.engine).unwrap();
    let handle = editor.engine.handle(&"b".into()).unwrap();
    let changes = Arc::new(AtomicUsize::new(0));
    let seen = changes.clone();
    editor
        .engine
        .on_change(None, move |_| {
            seen.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    editor
        .transaction(|s| {
            s.update_object(&"a".into(), |o| {
                o.style.stroke.as_mut().unwrap().width = 10.
            })
        })
        .unwrap();
    assert_slot(editor.scene(), "a", 0., 0., 30., 20.);
    assert_slot(editor.scene(), "b", 30., 0., 24., 14.);
    assert_eq!(changes.load(Ordering::SeqCst), 1);
    assert_eq!(editor.engine.handle(&"b".into()), Some(handle));
    let after = editor.scene().clone();
    let revision = editor.engine.revision();
    layout.apply_to_engine(&mut editor.engine).unwrap();
    assert_eq!(editor.engine.revision(), revision);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
}

#[test]
fn reactive_new_stroke_that_requires_resize_rolls_back_style_and_history() {
    let (mut scene, mut layout) = fixture(100., 50.);
    add(&mut scene, "a", 20., 10., 0.);
    layout.items.insert(
        "a".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    layout.install_reactive(&mut editor.engine).unwrap();
    editor
        .transaction(|s| s.update_object(&"a".into(), |o| o.name = "prior".into()))
        .unwrap();
    let before = editor.scene().clone();
    let revision = editor.engine.revision();
    assert!(
        editor
            .transaction(|s| s.update_object(&"a".into(), |o| o
                .style
                .stroke
                .as_mut()
                .unwrap()
                .width = 4.))
            .is_err()
    );
    assert_eq!(editor.scene(), &before);
    assert_eq!(editor.engine.revision(), revision);
    assert!(editor.undo());
    assert_eq!(object(editor.scene(), "a").name, "");
    assert!(!editor.undo());
}

#[test]
fn huge_stroke_allocation_failure_keeps_the_complete_scene_unchanged() {
    let (mut scene, layout) = fixture(100., 100.);
    add(&mut scene, "a", 10., 10., 0.);
    // Widened arithmetic does not exempt huge valid stroke bounds from the
    // position-only contract or permit an earlier sibling to partially commit.
    let o = DisplayObject::new("b", display::Rect::new(1e38, 1.))
        .with_style(Style::stroke(Color::BLACK, 1e38));
    scene.insert(o, Some("root".into()), None).unwrap();
    let before = scene.clone();
    let mut constrained = layout;
    constrained.items.insert(
        "b".into(),
        FlowItem {
            min_height: Some(2e38),
            ..Default::default()
        },
    );
    assert!(constrained.apply_to_scene(&mut scene).is_err());
    assert_eq!(scene, before);
}

#[test]
fn wrapped_content_axis_measures_stroke_line_extent() {
    let (mut scene, mut layout) = fixture(50., 100.);
    style(&mut layout).wrap = true;
    style(&mut layout).height_sizing = AxisSizing::content();
    style(&mut layout).line_gap = 3.;
    for id in ["a", "b", "c"] {
        add(&mut scene, id, 20., 10., 8.);
    }
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(
        object(&scene, "root").shape.local_bounds().unwrap().size,
        Size::new(50., 60.)
    );
    assert_slot(&scene, "c", 0., 42., 28., 18.);
    assert_stable(&layout, &scene);
}

#[test]
fn reactive_layers_and_stroke_paint_visibility_recompute_selected_slots() {
    let (mut scene, layout) = fixture(200., 50.);
    add(&mut scene, "a", 20., 10., 4.);
    add(&mut scene, "b", 20., 10., 0.);
    layout.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    layout.install_reactive(&mut editor.engine).unwrap();
    editor
        .transaction(|s| {
            s.update_object(&"a".into(), |o| {
                o.style.stroke_layers.push(Stroke::new(Color::BLACK, 10.));
            })
        })
        .unwrap();
    assert_slot(editor.scene(), "b", 30., 0., 20., 10.);
    editor
        .transaction(|s| {
            s.update_object(&"a".into(), |o| {
                o.style.stroke.as_mut().unwrap().paint = Color::TRANSPARENT.into();
                o.style.stroke_layers[0].paint = Color::TRANSPARENT.into();
            })
        })
        .unwrap();
    assert_slot(editor.scene(), "a", 0., 0., 20., 10.);
    assert_slot(editor.scene(), "b", 20., 0., 20., 10.);
    assert!(editor.undo());
    assert_slot(editor.scene(), "b", 30., 0., 20., 10.);
    assert!(editor.undo());
    assert_slot(editor.scene(), "b", 24., 0., 20., 10.);
    assert!(!editor.undo());
}

#[test]
fn aligned_stroke_boxes_reflow_on_alignment_width_and_restore_in_one_undo() {
    use gpui_flow_scene::{FillRule, StrokeAlignment};
    let (mut scene, layout) = fixture(200., 50.);
    add(&mut scene, "a", 20., 20., 4.);
    add(&mut scene, "b", 20., 20., 4.);
    scene
        .update_object(&"a".into(), |o| {
            o.style.stroke.as_mut().unwrap().alignment = StrokeAlignment::Inside
        })
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 0., 0., 20., 20.);
    assert_slot(&scene, "b", 20., 0., 24., 24.);
    let before = scene.clone();
    let mut editor = SceneEditor::new(scene).unwrap();
    layout.install_reactive(&mut editor.engine).unwrap();
    editor
        .transaction(|s| {
            s.update_object(&"a".into(), |o| {
                let stroke = o.style.stroke.as_mut().unwrap();
                stroke.alignment = StrokeAlignment::Outside;
                stroke.join = StrokeJoin::Miter;
                stroke.width = 6.;
                o.style.fill_rule = FillRule::EvenOdd;
            })
        })
        .unwrap();
    assert_slot(editor.scene(), "a", 0., 0., 32., 32.);
    assert_slot(editor.scene(), "b", 32., 0., 24., 24.);
    let after = editor.scene().clone();
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert_eq!(editor.scene(), &after);
    assert_stable(&layout, editor.scene());
}

#[test]
fn aligned_open_path_stroke_boxes_reflow_and_restore_closure_in_one_undo() {
    use gpui_flow_scene::StrokeAlignment;
    let (mut scene, mut layout) = fixture(200., 50.);
    style(&mut layout).gap = 3.;
    add(&mut scene, "a", 20., 20., 4.);
    add(&mut scene, "b", 20., 20., 4.);
    scene
        .update_object(&"a".into(), |o| {
            o.shape = display::Path::from_svg("M0 0H20V20H0Z").unwrap().into();
            o.style.stroke = Some(
                Stroke::new(Color::BLACK, 4.)
                    .with_alignment(StrokeAlignment::Outside)
                    .with_cap(StrokeCap::Butt)
                    .with_join(StrokeJoin::Miter),
            );
        })
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_slot(&scene, "a", 0., 0., 28., 28.);
    assert_slot(&scene, "b", 31., 0., 24., 24.);
    let before = scene.clone();
    let mut editor = SceneEditor::new(scene).unwrap();
    layout.install_reactive(&mut editor.engine).unwrap();
    editor
        .transaction(|s| {
            s.update_object(&"a".into(), |o| {
                o.shape = display::Path::from_svg("M0 0H20V20H0").unwrap().into();
            })
        })
        .unwrap();
    // Butt caps leave the missing left edge unexpanded; the following item
    // must move with the actual final stroke mesh in the same transaction.
    assert_slot(editor.scene(), "a", 0., 0., 24., 28.);
    assert_slot(editor.scene(), "b", 27., 0., 24., 24.);
    assert_stable(&layout, editor.scene());
    let open = editor.scene().clone();
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(!editor.undo());
    assert!(editor.redo());
    assert_eq!(editor.scene(), &open);
    editor
        .transaction(|s| {
            s.update_object(&"a".into(), |o| {
                o.style.stroke.as_mut().unwrap().alignment = StrokeAlignment::Inside;
            })
        })
        .unwrap();
    assert_slot(editor.scene(), "a", 0., 0., 20., 20.);
    assert_slot(editor.scene(), "b", 23., 0., 24., 24.);
    let inside = editor.scene().clone();
    assert!(editor.undo());
    assert_eq!(editor.scene(), &open);
    assert!(editor.redo());
    assert_eq!(editor.scene(), &inside);
    assert_stable(&layout, editor.scene());
}
