use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{
    CustomDefinition, DirtyFlags, ElementRegistry, PropertyDescriptor, PropertySchema, SceneEngine,
};
use gpui_flow_layout::*;
use gpui_flow_scene::{
    Affine, Color, DisplayObject, HitTestOptions, Point, Scene, Shape, Size, Style, display,
};
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
    for (index, (width, height)) in sizes.iter().enumerate() {
        scene
            .insert(
                DisplayObject::new(format!("c{index}"), display::Rect::new(*width, *height))
                    .with_style(Style::fill(Color::WHITE)),
                Some("root".into()),
                None,
            )
            .unwrap();
    }
    let mut layout = FlowLayout::default();
    layout.containers.insert(
        "root".into(),
        FlowStyle {
            width_sizing: AxisSizing::content(),
            height_sizing: AxisSizing::content(),
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
fn root(layout: &mut FlowLayout) -> &mut FlowStyle {
    layout.containers.get_mut(&"root".into()).unwrap()
}
#[test]
fn row_measures_padding_gap_and_largest_cross_extent() {
    let (mut scene, mut layout) = fixture(900., 700., &[(40., 20.), (50., 30.)]);
    root(&mut layout).padding = Insets::all(5.);
    root(&mut layout).gap = 10.;
    let report = layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(110., 40.));
    assert_eq!(pos(&scene, "c0"), Point::new(5., 5.));
    assert_eq!(pos(&scene, "c1"), Point::new(55., 5.));
    assert!(report.passes >= 2);
    assert_eq!(report.containers, 1);
    assert_eq!(report.items, 2);
    assert!(
        layout
            .apply_to_scene(&mut scene)
            .unwrap()
            .changed
            .is_empty()
    );
}
#[test]
fn column_auto_size_uses_separate_intrinsic_axes_and_asymmetric_padding() {
    let (mut scene, mut layout) = fixture(900., 700., &[(40., 20.), (50., 30.)]);
    let style = root(&mut layout);
    style.direction = FlowDirection::Column;
    style.padding = Insets::symmetric(6., 8.);
    style.gap = 7.;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(62., 73.));
    assert_eq!(pos(&scene, "c1"), Point::new(6., 35.));
}
#[test]
fn empty_content_boxes_have_positive_minimum_and_include_padding() {
    let (mut scene, mut layout) = fixture(900., 700., &[]);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(1., 1.));
    root(&mut layout).padding = Insets::all(4.);
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(8., 8.));
    root(&mut layout).width_sizing = AxisSizing::content_between(20., Some(30.));
    root(&mut layout).height_sizing = AxisSizing::content_between(30., Some(40.));
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(20., 30.));
}
#[test]
fn row_fixed_width_wrap_measures_total_auto_height_including_line_gap() {
    let (mut scene, mut layout) = fixture(160., 900., &[(70., 20.), (70., 40.)]);
    let style = root(&mut layout);
    style.width_sizing = AxisSizing::Fixed;
    style.padding = Insets::all(10.);
    style.gap = 10.;
    style.line_gap = 7.;
    style.wrap = true;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(160., 87.));
    assert_eq!(pos(&scene, "c1"), Point::new(10., 37.));
}
#[test]
fn column_fixed_height_wrap_measures_total_auto_width() {
    let (mut scene, mut layout) = fixture(900., 100., &[(30., 50.), (40., 50.)]);
    let style = root(&mut layout);
    style.direction = FlowDirection::Column;
    style.height_sizing = AxisSizing::Fixed;
    style.padding = Insets::all(10.);
    style.gap = 5.;
    style.line_gap = 9.;
    style.wrap = true;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(99., 100.));
    assert_eq!(pos(&scene, "c1"), Point::new(49., 10.));
}
#[test]
fn content_main_axis_disables_wrap_even_when_a_maximum_caps_the_box() {
    let (mut scene, mut layout) = fixture(30., 900., &[(50., 10.), (50., 10.)]);
    let style = root(&mut layout);
    style.wrap = true;
    style.gap = 10.;
    style.width_sizing = AxisSizing::content_between(1., Some(80.));
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(80., 10.));
    assert_eq!(pos(&scene, "c1"), Point::new(60., 0.));
    assert_eq!(size(&scene, "c1"), Size::new(50., 10.));
}
#[test]
fn content_minimum_creates_alignment_space_without_scaling_children() {
    let (mut scene, mut layout) = fixture(30., 900., &[(50., 10.), (50., 10.)]);
    let style = root(&mut layout);
    style.gap = 10.;
    style.width_sizing = AxisSizing::content_between(200., Some(250.));
    style.height_sizing = AxisSizing::content_between(40., None);
    style.justify = JustifyContent::Center;
    style.align_items = AlignItems::Center;
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(200., 40.));
    assert_eq!(pos(&scene, "c0"), Point::new(45., 15.));
    assert_eq!(size(&scene, "c0"), Size::new(50., 10.));
}
#[test]
fn hidden_items_measure_but_excluded_items_do_not_affect_auto_size() {
    let (mut scene, mut layout) = fixture(900., 700., &[(40., 20.), (50., 30.), (500., 500.)]);
    root(&mut layout).gap = 10.;
    scene
        .update_object(&"c1".into(), |o| o.visible = false)
        .unwrap();
    scene
        .update_object(&"c2".into(), |o| {
            o.transform = Affine::translation(700., 800.)
        })
        .unwrap();
    layout.items.insert(
        "c2".into(),
        FlowItem {
            in_flow: false,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(100., 30.));
    assert_eq!(pos(&scene, "c2"), Point::new(700., 800.));
}
#[test]
fn nested_content_is_measured_bottom_up_independent_of_id_sorting() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("m-parent", display::Box::new(900., 700.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("z-child", display::Frame::new(600., 500.)),
            Some("m-parent".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("a-leaf", display::Rect::new(20., 30.)),
            Some("z-child".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("b-sibling", display::Rect::new(40., 10.)),
            Some("m-parent".into()),
            None,
        )
        .unwrap();
    let mut layout = FlowLayout::default();
    layout.containers.insert(
        "m-parent".into(),
        FlowStyle {
            width_sizing: AxisSizing::content(),
            height_sizing: AxisSizing::content(),
            padding: Insets::all(5.),
            gap: 7.,
            ..Default::default()
        },
    );
    layout.containers.insert(
        "z-child".into(),
        FlowStyle {
            width_sizing: AxisSizing::content(),
            height_sizing: AxisSizing::content(),
            padding: Insets::all(4.),
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "z-child"), Size::new(28., 38.));
    assert_eq!(size(&scene, "m-parent"), Size::new(85., 48.));
    assert_eq!(pos(&scene, "b-sibling"), Point::new(40., 5.));
}
fn nested_wrap() -> (Scene, FlowLayout) {
    let (mut scene, mut layout) = fixture(240., 900., &[]);
    let style = root(&mut layout);
    style.width_sizing = AxisSizing::Fixed;
    style.padding = Insets::all(10.);
    scene
        .insert(
            DisplayObject::new("nested", display::Frame::new(50., 900.)),
            Some("root".into()),
            None,
        )
        .unwrap();
    for index in 0..2 {
        scene
            .insert(
                DisplayObject::new(format!("leaf{index}"), display::Rect::new(100., 20.)),
                Some("nested".into()),
                None,
            )
            .unwrap();
    }
    layout.containers.insert(
        "nested".into(),
        FlowStyle {
            height_sizing: AxisSizing::content(),
            padding: Insets::all(5.),
            gap: 10.,
            line_gap: 7.,
            wrap: true,
            ..Default::default()
        },
    );
    layout.items.insert(
        "nested".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    (scene, layout)
}
#[test]
fn parent_width_allocation_propagates_down_then_wrapped_auto_height_propagates_up() {
    let (mut scene, layout) = nested_wrap();
    let report = layout.apply_to_scene(&mut scene).unwrap();
    assert!(report.passes <= MAX_FLOW_LAYOUT_PASSES);
    assert_eq!(size(&scene, "nested"), Size::new(220., 30.));
    assert_eq!(size(&scene, "root"), Size::new(240., 50.));
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    layout
        .transaction_editor(&mut editor, |s| {
            s.update_object(&"root".into(), |o| {
                let Shape::Frame(frame) = &mut o.shape else {
                    panic!("frame")
                };
                frame.size.width = 140.;
            })
        })
        .unwrap();
    assert_eq!(size(editor.scene(), "nested"), Size::new(120., 57.));
    assert_eq!(size(editor.scene(), "root"), Size::new(140., 77.));
    assert!(editor.undo());
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
}
#[test]
fn grow_and_stretch_feedback_are_rejected_atomically() {
    let (scene, mut layout) = fixture(900., 700., &[(40., 20.)]);
    layout.items.insert(
        "c0".into(),
        FlowItem {
            grow: 1.,
            ..Default::default()
        },
    );
    assert!(layout.arranged(&scene).is_err());
    layout.items.clear();
    root(&mut layout).align_items = AlignItems::Stretch;
    assert!(layout.arranged(&scene).is_err());
    layout.items.insert(
        "c0".into(),
        FlowItem {
            align_self: Some(AlignItems::Start),
            ..Default::default()
        },
    );
    assert!(layout.arranged(&scene).is_ok());
}
#[test]
fn a_nested_axis_cannot_be_owned_by_both_content_measurement_and_parent_allocation() {
    let (scene, mut layout) = nested_wrap();
    layout
        .containers
        .get_mut(&"nested".into())
        .unwrap()
        .width_sizing = AxisSizing::content();
    assert!(layout.arranged(&scene).is_err());
    layout
        .containers
        .get_mut(&"nested".into())
        .unwrap()
        .width_sizing = AxisSizing::Fixed;
    layout.items.get_mut(&"nested".into()).unwrap().max_height = Some(80.);
    assert!(layout.arranged(&scene).is_err());
}
#[test]
fn intrinsic_auto_sizes_ignore_container_world_scale_but_measure_child_affine_boxes() {
    let (mut scene, mut layout) = fixture(900., 700., &[(20., 30.)]);
    root(&mut layout).padding = Insets::all(8.);
    scene
        .update_object(&"root".into(), |o| o.transform = Affine::scale(3., 4.))
        .unwrap();
    scene
        .update_object(&"c0".into(), |o| o.transform = Affine::scale(2., 3.))
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    assert_eq!(size(&scene, "root"), Size::new(56., 106.));
    let engine = SceneEngine::new(scene).unwrap();
    assert_eq!(
        engine.world_bounds(&"root".into()).unwrap().unwrap().size,
        Size::new(168., 424.)
    );
}
#[test]
fn repeated_fractional_scaled_content_layout_is_idempotent() {
    let (mut scene, mut layout) = fixture(900., 700., &[(7.1, 3.7), (5.3, 9.1)]);
    root(&mut layout).gap = 0.7;
    scene
        .update_object(&"c0".into(), |o| o.transform = Affine::scale(1.7, 1.3))
        .unwrap();
    layout.apply_to_scene(&mut scene).unwrap();
    let first = scene.clone();
    for _ in 0..30 {
        assert!(
            layout
                .apply_to_scene(&mut scene)
                .unwrap()
                .changed
                .is_empty()
        );
        assert_eq!(scene, first);
    }
}
#[test]
fn custom_property_is_normalized_before_edit_transaction_measures_content() {
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
                    DirtyFlags::GEOMETRY | DirtyFlags::HIT | DirtyFlags::PAINT,
                )],
                allow_unknown: false,
            },
            |data: &serde_json::Value| {
                let shape = display::Ellipse::circle(data["radius"].as_f64().unwrap() as f32);
                Ok((shape.bounds(), shape.flatten(0.1)))
            },
        ))
        .unwrap();
    let (scene, mut layout) = fixture(900., 700., &[]);
    root(&mut layout).padding = Insets::all(10.);
    let mut engine = SceneEngine::with_registry(scene, registry).unwrap();
    engine
        .create(
            "disc",
            "disc",
            json!({}),
            Some(engine.handle(&"root".into()).unwrap()),
            None,
        )
        .unwrap();
    let mut editor = SceneEditor::from_engine(engine);
    layout.apply_to_editor(&mut editor).unwrap();
    assert_eq!(size(editor.scene(), "root"), Size::new(30., 30.));
    let before = editor.scene().clone();
    layout
        .transaction_editor(&mut editor, |s| {
            s.update_object(&"disc".into(), |o| {
                let Shape::Custom(custom) = &mut o.shape else {
                    panic!("custom")
                };
                custom.data["radius"] = json!(15.);
            })
        })
        .unwrap();
    assert_eq!(size(editor.scene(), "root"), Size::new(50., 50.));
    assert_eq!(size(editor.scene(), "disc"), Size::new(30., 30.));
    // Final engine commit normalizes definitions again. The committed geometry
    // must still be the same fixed point that the layout provider measured.
    let (repeated, report) = layout.arranged(editor.scene()).unwrap();
    assert!(report.changed.is_empty());
    assert_eq!(&repeated, editor.scene());
    assert_eq!(
        editor.engine.prepare_scene(editor.scene().clone()).unwrap(),
        repeated
    );
    assert_eq!(
        editor
            .engine
            .hit_test(Point::new(35., 25.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "disc"
    );
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
}
#[test]
fn edit_and_dependent_sizes_are_one_history_step_and_preserve_prior_edits() {
    let (mut scene, layout) = fixture(900., 700., &[(40., 20.)]);
    layout.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    editor
        .transaction(|s| s.update_object(&"c0".into(), |o| o.name = "saved edit".into()))
        .unwrap();
    let before = editor.scene().clone();
    layout
        .transaction_editor(&mut editor, |s| {
            s.update_object(&"c0".into(), |o| {
                let Shape::Rect(rect) = &mut o.shape else {
                    panic!("rect")
                };
                rect.size.width = 90.;
            })
        })
        .unwrap();
    assert_eq!(size(editor.scene(), "root").width, 90.);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &before);
    assert!(editor.undo());
    assert_eq!(editor.scene().get(&"c0".into()).unwrap().object.name, "");
    assert!(!editor.undo());
}
#[test]
fn invalid_layout_after_an_input_edit_rolls_back_the_input_and_history() {
    let (scene, mut layout) = fixture(900., 700., &[(40., 20.)]);
    root(&mut layout).height_sizing = AxisSizing::Content { min: 0., max: None };
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    let revision = editor.engine.revision();
    assert!(
        layout
            .transaction_editor(&mut editor, |s| s
                .update_object(&"c0".into(), |o| o.name = "not committed".into()))
            .is_err()
    );
    assert_eq!(editor.scene(), &scene);
    assert_eq!(editor.engine.revision(), revision);
    assert!(!editor.undo());
}
#[test]
fn invalid_content_limits_and_old_json_defaults_have_explicit_behavior() {
    let (scene, mut layout) = fixture(900., 700., &[(40., 20.)]);
    for sizing in [
        AxisSizing::Content {
            min: f32::NAN,
            max: None,
        },
        AxisSizing::Content { min: 0., max: None },
        AxisSizing::Content {
            min: 10.,
            max: Some(5.),
        },
        AxisSizing::Content {
            min: 1.,
            max: Some(f32::INFINITY),
        },
    ] {
        root(&mut layout).width_sizing = sizing;
        assert!(layout.validate(&scene).is_err());
    }
    let old: FlowStyle = serde_json::from_value(json!({"gap":10.})).unwrap();
    assert_eq!(old.width_sizing, AxisSizing::Fixed);
    assert_eq!(old.height_sizing, AxisSizing::Fixed);
    root(&mut layout).width_sizing = AxisSizing::content_between(10., Some(80.));
    let document = FlowDocument { scene, layout };
    assert_eq!(
        FlowDocument::from_json(&document.to_json().unwrap()).unwrap(),
        document
    );
}

fn start_playback(
    editor: &mut SceneEditor,
) -> (
    gpui_flow_animation::AnimationController,
    gpui_flow_editor::PlaybackToken,
) {
    use gpui_flow_animation::{AnimationController, AnimationSpec, Tween};
    let token = editor.begin_playback().unwrap();
    let mut controller = AnimationController::new(&editor.engine);
    controller
        .animate(
            &editor.engine,
            editor.engine.handle(&"c0".into()).unwrap(),
            AnimationSpec::new(1000, vec![Tween::translation(Point::new(100., 0.))]),
        )
        .unwrap();
    controller
        .tick_editor(editor, token, std::time::Duration::from_millis(500))
        .unwrap();
    (controller, token)
}
#[test]
fn layout_edit_finishes_playback_and_preserves_separate_undo_boundaries() {
    use gpui_flow_animation::AnimationError;
    let (mut scene, layout) = fixture(900., 700., &[(40., 20.)]);
    layout.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene.clone()).unwrap();
    editor
        .transaction(|scene| scene.update_object(&"c0".into(), |o| o.name = "prior".into()))
        .unwrap();
    let prior_edit = editor.scene().clone();
    let (mut controller, token) = start_playback(&mut editor);
    let playback_frame = editor.scene().clone();
    assert_eq!(pos(editor.scene(), "c0"), Point::new(50., 0.));
    layout
        .transaction_editor(&mut editor, |scene| {
            scene.update_object(&"c0".into(), |object| {
                let Shape::Rect(rect) = &mut object.shape else {
                    unreachable!()
                };
                rect.size.width = 90.;
            })
        })
        .unwrap();
    let laid_out = editor.scene().clone();
    assert_eq!(size(editor.scene(), "root").width, 90.);
    assert_eq!(pos(editor.scene(), "c0"), Point::new(0., 0.));
    assert!(!editor.is_playback_active(token));
    assert_eq!(
        controller
            .tick_editor(&mut editor, token, std::time::Duration::from_millis(100))
            .unwrap_err(),
        AnimationError::EditorSessionEnded
    );
    assert!(controller.cancel_editor(&mut editor, token).is_err());
    assert_eq!(editor.scene(), &laid_out);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &playback_frame);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &prior_edit);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &scene);
    assert!(!editor.undo());
    for expected in [&prior_edit, &playback_frame, &laid_out] {
        assert!(editor.redo());
        assert_eq!(editor.scene(), expected);
    }
    assert!(!editor.redo());
}
#[test]
fn invalid_layout_keeps_active_playback_and_existing_redo_intact() {
    let (mut scene, mut layout) = fixture(900., 700., &[(40., 20.)]);
    layout.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    editor
        .transaction(|scene| scene.update_object(&"c0".into(), |o| o.name = "prior".into()))
        .unwrap();
    let prior_edit = editor.scene().clone();
    editor
        .transaction(|scene| scene.update_object(&"c0".into(), |o| o.name = "redo branch".into()))
        .unwrap();
    let redo = editor.scene().clone();
    assert!(editor.undo());
    let (mut controller, token) = start_playback(&mut editor);
    let frame = editor.scene().clone();
    let revision = editor.engine.revision();
    root(&mut layout).width_sizing = AxisSizing::Content { min: 0., max: None };
    assert!(
        layout
            .transaction_editor(&mut editor, |scene| scene
                .update_object(&"c0".into(), |o| o.name = "invalid input".into()))
            .is_err()
    );
    assert_eq!(editor.scene(), &frame);
    assert_eq!(editor.engine.revision(), revision);
    assert!(editor.is_playback_active(token));
    controller
        .tick_editor(&mut editor, token, std::time::Duration::from_millis(100))
        .unwrap();
    assert_eq!(pos(editor.scene(), "c0"), Point::new(60., 0.));
    controller.cancel_editor(&mut editor, token).unwrap();
    assert_eq!(editor.scene(), &prior_edit);
    assert!(editor.redo());
    assert_eq!(editor.scene(), &redo);
    assert!(editor.undo());
    assert_eq!(editor.scene(), &prior_edit);
    assert!(editor.undo());
    assert!(!editor.undo());
}
