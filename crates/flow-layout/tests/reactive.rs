use gpui_flow_animation::*;
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::*;
use gpui_flow_layout::*;
use gpui_flow_scene::{Affine, DisplayObject, Point, Scene, Shape, Size, display};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
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
fn layout() -> FlowLayout {
    let mut layout = FlowLayout::default();
    layout.containers.insert(
        "frame".into(),
        FlowStyle {
            height_sizing: AxisSizing::content(),
            padding: Insets::all(10.),
            gap: 10.,
            line_gap: 10.,
            wrap: true,
            ..Default::default()
        },
    );
    layout
}
fn setup() -> (SceneEditor, FlowLayout, LayoutProviderId) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("frame", display::Frame::new(220., 200.)),
            None,
            None,
        )
        .unwrap();
    for id in ["a", "b"] {
        scene
            .insert(
                DisplayObject::new(id, display::Rect::new(80., 30.)),
                Some("frame".into()),
                None,
            )
            .unwrap();
    }
    let layout = layout();
    layout.apply_to_scene(&mut scene).unwrap();
    let mut editor = SceneEditor::new(scene).unwrap();
    let token = layout.install_reactive(&mut editor.engine).unwrap();
    (editor, layout, token)
}
fn width(editor: &mut SceneEditor, value: f32) -> Result<(), gpui_flow_scene::SceneError> {
    let mut shape = editor
        .scene()
        .get(&"frame".into())
        .unwrap()
        .object
        .shape
        .clone();
    let Shape::Frame(frame) = &mut shape else {
        panic!()
    };
    frame.size.width = value;
    editor.set_property(
        editor.engine.handle(&"frame".into()).unwrap(),
        PropertyKey::Shape,
        serde_json::to_value(shape).unwrap(),
    )
}
#[test]
fn ordinary_property_edit_rewraps_before_one_notification_and_one_undo() {
    let (mut e, _, _) = setup();
    e.transaction(|s| s.update_object(&"a".into(), |o| o.name = "prior".into()))
        .unwrap();
    let before = e.scene().clone();
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    e.engine
        .on_change(None, move |changes| {
            assert!(
                changes
                    .properties
                    .iter()
                    .any(|p| p.id.as_ref().is_some_and(|id| id.as_str() == "frame"))
            );
            seen.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    width(&mut e, 140.).unwrap();
    assert_eq!(size(e.scene(), "frame"), Size::new(140., 90.));
    assert_eq!(
        e.engine
            .world_bounds(&"frame".into())
            .unwrap()
            .unwrap()
            .size
            .height,
        90.
    );
    assert_eq!(count.load(Ordering::SeqCst), 1);
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(e.undo());
    assert_eq!(e.scene().get(&"a".into()).unwrap().object.name, "");
    assert!(!e.undo());
}
#[test]
fn insertion_removal_and_reorder_derive_in_the_same_input_transaction() {
    let (mut e, _, _) = setup();
    let before = e.scene().clone();
    e.transaction(|s| {
        s.insert(
            DisplayObject::new("c", display::Rect::new(80., 30.)),
            Some("frame".into()),
            None,
        )
    })
    .unwrap();
    assert_eq!(size(e.scene(), "frame").height, 90.);
    e.transaction(|s| s.reparent(&"c".into(), Some("frame".into()), Some(0), false))
        .unwrap();
    assert_eq!(e.scene().get(&"c".into()).unwrap().object.transform.ty, 10.);
    assert_eq!(e.scene().get(&"b".into()).unwrap().object.transform.ty, 50.);
    e.transaction(|s| {
        s.remove(&"a".into())?;
        Ok(())
    })
    .unwrap();
    assert_eq!(size(e.scene(), "frame").height, 50.);
    for _ in 0..3 {
        assert!(e.undo());
    }
    assert_eq!(e.scene(), &before);
}
#[test]
fn removal_and_replacement_configuration_leave_existing_history_exact() {
    let (mut e, _, token) = setup();
    let before = e.scene().clone();
    width(&mut e, 140.).unwrap();
    let narrow = e.scene().clone();
    assert!(e.engine.remove_layout_provider(token));
    let empty = FlowLayout::default()
        .install_reactive(&mut e.engine)
        .unwrap();
    assert!(!e.engine.remove_layout_provider(token));
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.engine.layout_provider(), Some(empty));
    assert!(e.redo());
    assert_eq!(e.scene(), &narrow);
    width(&mut e, 500.).unwrap();
    assert_eq!(size(e.scene(), "frame").height, 90.);
}
#[test]
fn detach_reattach_retains_configuration_but_destroy_recreate_does_not() {
    let (mut e, _, _) = setup();
    let frame = e.engine.handle(&"frame".into()).unwrap();
    let (subtree, _) = e.engine.detach(frame).unwrap();
    assert_eq!(e.engine.scene().len(), 0);
    e.engine.attach(&subtree, None, None).unwrap();
    e.engine
        .transaction(|s| {
            s.update_object(&"frame".into(), |o| {
                let Shape::Frame(frame) = &mut o.shape else {
                    panic!()
                };
                frame.size.width = 140.;
            })
        })
        .unwrap();
    assert_eq!(size(e.scene(), "frame").height, 90.);
    e.engine.destroy(frame).unwrap();
    e.engine
        .transaction(|s| {
            s.insert(
                DisplayObject::new("frame", display::Frame::new(140., 300.)),
                None,
                None,
            )?;
            s.insert(
                DisplayObject::new("a", display::Rect::new(80., 30.)),
                Some("frame".into()),
                None,
            )
        })
        .unwrap();
    assert_eq!(size(e.scene(), "frame").height, 300.);
}
#[test]
fn replaced_item_does_not_inherit_old_exclusion_but_new_child_uses_defaults() {
    let (mut e, mut l, token) = setup();
    assert!(e.engine.remove_layout_provider(token));
    l.items.insert(
        "b".into(),
        FlowItem {
            in_flow: false,
            ..Default::default()
        },
    );
    l.apply_to_editor(&mut e).unwrap();
    l.install_reactive(&mut e.engine).unwrap();
    let b = e.engine.handle(&"b".into()).unwrap();
    e.engine.destroy(b).unwrap();
    e.engine
        .transaction(|s| {
            s.insert(
                DisplayObject::new("b", display::Rect::new(80., 70.)),
                Some("frame".into()),
                None,
            )
        })
        .unwrap();
    assert_eq!(size(e.scene(), "frame").height, 90.);
}
#[test]
fn same_document_reset_removes_runtime_configuration() {
    let (mut e, _, token) = setup();
    e.load_scene(e.scene().clone()).unwrap();
    assert!(e.engine.layout_provider().is_none());
    assert!(!e.engine.remove_layout_provider(token));
    width(&mut e, 140.).unwrap();
    assert_eq!(size(e.scene(), "frame").height, 50.);
}
#[test]
fn layout_owned_translation_animation_is_rejected_before_records_or_playback_change() {
    let (mut e, _, _) = setup();
    let token = e.begin_playback().unwrap();
    let mut controller = AnimationController::new(&e.engine);
    let before = e.scene().clone();
    assert!(
        controller
            .animate(
                &e.engine,
                e.engine.handle(&"a".into()).unwrap(),
                AnimationSpec::new(1000, vec![Tween::translation(Point::new(100., 0.))])
            )
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert!(e.is_playback_active(token));
    assert!(e.cancel_playback(token));
    assert!(!e.undo());
}
#[test]
fn failed_layout_transaction_and_property_keep_playback_and_redo_branch() {
    let (mut e, _, _) = setup();
    let a = e.engine.handle(&"a".into()).unwrap();
    e.set_property(a, PropertyKey::Name, json!("first"))
        .unwrap();
    e.set_property(a, PropertyKey::Name, json!("redo")).unwrap();
    assert!(e.undo());
    let baseline = e.scene().clone();
    let token = e.begin_playback().unwrap();
    e.apply_playback_properties(token, [(a, PropertyKey::Opacity, json!(0.5))])
        .unwrap();
    let frame = e.scene().clone();
    let rev = e.engine.revision();
    assert!(
        e.transaction(|s| s.insert(
            DisplayObject::new("bad", display::Group::new()),
            Some("frame".into()),
            None
        ))
        .is_err()
    );
    assert!(
        e.set_property(
            a,
            PropertyKey::Shape,
            serde_json::to_value(Shape::Group(display::Group::new())).unwrap()
        )
        .is_err()
    );
    assert_eq!(e.scene(), &frame);
    assert_eq!(e.engine.revision(), rev);
    assert!(e.is_playback_active(token));
    assert!(e.cancel_playback(token));
    assert_eq!(e.scene(), &baseline);
    assert!(e.redo());
    assert_eq!(e.scene().get(&"a".into()).unwrap().object.name, "redo");
}
#[test]
fn successful_ordinary_edit_interrupts_playback_into_separate_undo_steps() {
    let (mut e, _, _) = setup();
    let a = e.engine.handle(&"a".into()).unwrap();
    e.set_property(a, PropertyKey::Name, json!("prior"))
        .unwrap();
    let before = e.scene().clone();
    let token = e.begin_playback().unwrap();
    e.apply_playback_properties(token, [(a, PropertyKey::Opacity, json!(0.5))])
        .unwrap();
    let frame = e.scene().clone();
    width(&mut e, 140.).unwrap();
    let final_scene = e.scene().clone();
    assert!(!e.is_playback_active(token));
    assert!(!e.cancel_playback(token));
    assert_eq!(e.scene(), &final_scene);
    assert!(e.undo());
    assert_eq!(e.scene(), &frame);
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(e.undo());
    assert!(!e.undo());
}
#[test]
fn reactive_custom_geometry_animation_updates_parent_and_cancel_restores_exact_baseline() {
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
                    DirtyFlags::GEOMETRY | DirtyFlags::HIT,
                )],
                allow_unknown: false,
            },
            |p: &serde_json::Value| {
                let shape = display::Ellipse::circle(p["radius"].as_f64().unwrap() as f32);
                Ok((shape.bounds(), vec![shape.flatten(0.1)]))
            },
        ))
        .unwrap();
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("frame", display::Frame::new(100., 100.)),
            None,
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::with_registry(scene, registry).unwrap();
    let parent = engine.handle(&"frame".into()).unwrap();
    let (disc, _) = engine
        .create("disc", "disc", json!({}), Some(parent), None)
        .unwrap();
    let mut l = layout();
    l.containers.get_mut(&"frame".into()).unwrap().width_sizing = AxisSizing::content();
    l.apply_to_engine(&mut engine).unwrap();
    l.install_reactive(&mut engine).unwrap();
    let mut e = SceneEditor::from_engine(engine);
    e.set_property(disc, PropertyKey::Name, json!("prior"))
        .unwrap();
    let baseline = e.scene().clone();
    let token = e.begin_playback().unwrap();
    let mut c = AnimationController::new(&e.engine);
    c.animate(
        &e.engine,
        disc,
        AnimationSpec::new(1000, vec![Tween::custom_number("radius", 15.)]),
    )
    .unwrap();
    c.tick_editor(&mut e, token, Duration::from_millis(500))
        .unwrap();
    assert_eq!(size(e.scene(), "frame"), Size::new(40., 40.));
    assert_eq!(size(e.scene(), "disc"), Size::new(20., 20.));
    c.cancel_editor(&mut e, token).unwrap();
    assert_eq!(e.scene(), &baseline);
    assert!(e.undo());
    assert!(!e.undo());
}

#[test]
fn reparent_between_bound_frames_updates_both_parents_atomically() {
    let (mut e, mut l, old) = setup();
    e.engine.remove_layout_provider(old);
    e.transaction(|s| {
        s.insert(
            DisplayObject::new("other", display::Frame::new(100., 100.)),
            None,
            None,
        )
    })
    .unwrap();
    l.containers.insert(
        "other".into(),
        FlowStyle {
            width_sizing: AxisSizing::content(),
            height_sizing: AxisSizing::content(),
            ..Default::default()
        },
    );
    l.apply_to_editor(&mut e).unwrap();
    l.install_reactive(&mut e.engine).unwrap();
    let before = e.scene().clone();
    e.transaction(|s| s.reparent(&"b".into(), Some("other".into()), None, false))
        .unwrap();
    assert_eq!(size(e.scene(), "other"), Size::new(80., 30.));
    assert_eq!(
        e.scene().get(&"b".into()).unwrap().object.transform,
        Affine::IDENTITY
    );
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
}
#[test]
fn failed_initial_install_leaves_old_undo_and_scene_unchanged() {
    let (mut e, mut l, token) = setup();
    e.engine.remove_layout_provider(token);
    e.transaction(|s| s.update_object(&"a".into(), |o| o.name = "prior".into()))
        .unwrap();
    let before = e.scene().clone();
    let rev = e.engine.revision();
    l.containers.get_mut(&"frame".into()).unwrap().gap = 40.;
    assert!(l.install_reactive(&mut e.engine).is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.engine.revision(), rev);
    assert!(e.engine.layout_provider().is_none());
    assert!(e.undo());
    assert!(!e.undo());
}
#[test]
fn custom_definition_panic_during_tick_preserves_every_scene_value_clock_and_playback() {
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
            |p: &serde_json::Value| {
                let r = p["radius"].as_f64().unwrap() as f32;
                if r == 13. {
                    panic!("geometry implementation failed");
                }
                let shape = display::Ellipse::circle(r);
                Ok((shape.bounds(), vec![shape.flatten(0.1)]))
            },
        ))
        .unwrap();
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("frame", display::Frame::new(100., 100.)),
            None,
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::with_registry(scene, registry).unwrap();
    let parent = engine.handle(&"frame".into()).unwrap();
    let (disc, _) = engine
        .create("disc", "disc", json!({}), Some(parent), None)
        .unwrap();
    let mut l = layout();
    l.containers.get_mut(&"frame".into()).unwrap().width_sizing = AxisSizing::content();
    l.apply_to_engine(&mut engine).unwrap();
    l.install_reactive(&mut engine).unwrap();
    let mut e = SceneEditor::from_engine(engine);
    e.set_property(disc, PropertyKey::Name, json!("prior"))
        .unwrap();
    let before = e.scene().clone();
    let revision = e.engine.revision();
    let token = e.begin_playback().unwrap();
    let mut c = AnimationController::new(&e.engine);
    let id = c
        .animate(
            &e.engine,
            disc,
            AnimationSpec::new(1000, vec![Tween::custom_number("radius", 21.)]),
        )
        .unwrap();
    assert!(
        c.tick_editor(&mut e, token, Duration::from_millis(500))
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.engine.revision(), revision);
    assert!(e.is_playback_active(token));
    assert_eq!(c.info(&e.engine, id).unwrap().elapsed, Duration::ZERO);
    c.tick_editor(&mut e, token, Duration::from_millis(750))
        .unwrap();
    assert_eq!(size(e.scene(), "frame"), Size::new(54., 54.));
    c.cancel_editor(&mut e, token).unwrap();
    assert_eq!(e.scene(), &before);
    assert!(e.undo());
    assert!(!e.undo());
}

#[test]
fn later_keyframe_cannot_take_layout_owned_geometry_even_when_first_frame_is_paint_only() {
    let (e, _, _) = setup();
    let original = e.scene().clone();
    let revision = e.engine.revision();
    let mut controller = AnimationController::new(&e.engine);
    let result = controller.animate_sequence(
        &e.engine,
        e.engine.handle(&"a".into()).unwrap(),
        AnimationSequence::new(vec![
            AnimationSpec::new(100, vec![Tween::opacity(0.5)]),
            AnimationSpec::new(100, vec![Tween::translation(Point::new(999., 999.))]),
        ]),
    );
    assert!(result.is_err());
    assert!(controller.is_empty());
    assert_eq!(e.scene(), &original);
    assert_eq!(e.engine.revision(), revision);
}
