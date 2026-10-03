use gpui_flow_engine::{ChangeOrigin, DirtyFlags, PropertyKey, PropertyType, SceneEngine};
use gpui_flow_scene::{
    Affine, Bounds, Color, DisplayObject, EraserMode, HitPolicy, HitTestOptions, MaskMode, Point,
    Scene, Shadow, Size, Style, display,
};
use serde_json::json;

fn fixture(mode: EraserMode) -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("parent", display::Group::new())
                .with_transform(Affine::translation(10., 20.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("positive", display::Rect::new(20., 20.)),
            Some("parent".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("eraser", display::Group::new())
                .with_transform(Affine::translation(100., 0.))
                .with_eraser(mode),
            Some("parent".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("cut", display::Rect::new(30., 20.))
                .with_transform(Affine::translation(5., 0.))
                .with_style(Style {
                    shadow: Some(Shadow::new(50., 0., 0., Color::BLACK)),
                    ..Default::default()
                }),
            Some("eraser".into()),
            None,
        )
        .unwrap();
    scene
}

fn assert_consistent(engine: &SceneEngine) {
    let rebuilt = SceneEngine::new(engine.scene().clone()).unwrap();
    for node in engine.scene().nodes() {
        let id = &node.object.id;
        assert_eq!(
            engine.local_bounds(id).unwrap(),
            engine.scene().local_bounds(id).unwrap()
        );
        assert_eq!(
            engine.world_bounds(id).unwrap(),
            engine.scene().world_bounds(id).unwrap()
        );
        assert_eq!(
            engine.world_transform(id).unwrap(),
            rebuilt.world_transform(id).unwrap()
        );
        assert_eq!(
            engine.effect_bounds(id).unwrap(),
            rebuilt.effect_bounds(id).unwrap()
        );
    }
    assert_eq!(
        engine.visible_bounds().unwrap(),
        engine.scene().visible_bounds().unwrap()
    );
    for point in [
        Point::new(15., 25.),
        Point::new(120., 25.),
        Point::new(180., 25.),
    ] {
        assert_eq!(
            engine.hit_test_all(point, Default::default()).unwrap(),
            engine
                .scene()
                .hit_test_all(point, Default::default())
                .unwrap()
        );
    }
}

#[test]
fn eraser_property_schema_and_invalid_batch_preserve_all_committed_state() {
    let descriptor = PropertyKey::Eraser.descriptor().unwrap();
    assert_eq!(descriptor.name, "eraser");
    assert_eq!(descriptor.default, Some(json!("none")));
    assert_eq!(descriptor.value_type, PropertyType::String);
    assert_eq!(
        descriptor.dirty,
        DirtyFlags::PAINT | DirtyFlags::GEOMETRY | DirtyFlags::HIT
    );
    let mut engine = SceneEngine::new(fixture(EraserMode::None)).unwrap();
    let handle = engine.handle(&"eraser".into()).unwrap();
    let before = engine.scene().clone();
    let layout = engine.layout().clone();
    let revision = engine.revision();
    let query_stats = engine.query_index_stats();
    for invalid in [
        json!("destination_out"),
        json!("Pixel"),
        json!(true),
        json!(null),
    ] {
        assert!(
            engine
                .set_properties(
                    handle,
                    [
                        (PropertyKey::Name, json!("discarded")),
                        (PropertyKey::Eraser, json!("pixel")),
                        (PropertyKey::Eraser, invalid),
                    ]
                )
                .is_err()
        );
        assert_eq!(engine.scene(), &before);
        assert_eq!(engine.layout(), &layout);
        assert_eq!(engine.revision(), revision);
        assert_eq!(engine.query_index_stats(), query_stats);
        assert_eq!(engine.handle(&"eraser".into()), Some(handle));
    }
}

#[test]
fn eraser_mode_changes_and_snapshot_undo_redo_recompute_bounds_and_exact_dirty_bits() {
    let mut engine = SceneEngine::new(fixture(EraserMode::None)).unwrap();
    let handle = engine.handle(&"eraser".into()).unwrap();
    let before = engine.scene().clone();
    let own_geometry = engine.world_bounds(&"eraser".into()).unwrap();
    let own_effect = engine.effect_bounds(&"eraser".into()).unwrap();
    assert_eq!(
        engine
            .world_bounds(&"parent".into())
            .unwrap()
            .unwrap()
            .max_x(),
        145.
    );
    assert_eq!(
        engine
            .effect_bounds(&"parent".into())
            .unwrap()
            .unwrap()
            .max_x(),
        195.
    );
    let change = engine
        .set_property(handle, PropertyKey::Eraser, json!("pixel"))
        .unwrap();
    assert_eq!(change.properties.len(), 1);
    assert_eq!(change.properties[0].property, "eraser");
    assert_eq!(change.properties[0].old, json!("none"));
    assert_eq!(change.properties[0].new, json!("pixel"));
    assert_eq!(
        change.dirty[&"eraser".into()],
        DirtyFlags::PAINT | DirtyFlags::GEOMETRY | DirtyFlags::HIT
    );
    assert_eq!(
        engine.world_bounds(&"parent".into()).unwrap(),
        Some(Bounds::new(Point::new(10., 20.), Size::new(20., 20.)))
    );
    assert_eq!(
        engine.effect_bounds(&"parent".into()).unwrap(),
        engine.world_bounds(&"parent".into()).unwrap()
    );
    assert_eq!(engine.world_bounds(&"eraser".into()).unwrap(), own_geometry);
    assert_eq!(engine.effect_bounds(&"eraser".into()).unwrap(), own_effect);
    assert!(change.damage_bounds().unwrap().max_x() >= 195.);
    assert_consistent(&engine);
    let after = engine.scene().clone();
    let undo = engine.replace_scene(before.clone()).unwrap();
    assert_eq!(undo.origin, ChangeOrigin::UndoRedo);
    assert_eq!(undo.dirty[&"eraser".into()], descriptor_dirty());
    assert_eq!(engine.scene(), &before);
    assert_consistent(&engine);
    let redo = engine.replace_scene(after.clone()).unwrap();
    assert_eq!(redo.origin, ChangeOrigin::UndoRedo);
    assert_eq!(redo.dirty[&"eraser".into()], descriptor_dirty());
    assert_eq!(engine.scene(), &after);
    assert_eq!(engine.handle(&"eraser".into()), Some(handle));
    assert_consistent(&engine);
    let change = engine
        .set_property(handle, PropertyKey::Eraser, json!("path"))
        .unwrap();
    assert_eq!(change.dirty[&"eraser".into()], descriptor_dirty());
    assert_consistent(&engine);
    let revision = engine.revision();
    assert!(
        !engine
            .set_property(handle, PropertyKey::Eraser, json!("path"))
            .unwrap()
            .changed()
    );
    assert_eq!(engine.revision(), revision);
}

fn descriptor_dirty() -> DirtyFlags {
    DirtyFlags::PAINT | DirtyFlags::GEOMETRY | DirtyFlags::HIT
}

#[test]
fn erased_subtree_effect_changes_preserve_own_bounds_without_growing_parent() {
    let mut engine = SceneEngine::new(fixture(EraserMode::Pixel)).unwrap();
    let parent = engine.world_bounds(&"parent".into()).unwrap();
    let cut = engine.handle(&"cut".into()).unwrap();
    let change = engine
        .set_property(
            cut,
            PropertyKey::Shadow,
            serde_json::to_value(Shadow::new(100., 0., 0., Color::BLACK)).unwrap(),
        )
        .unwrap();
    assert_eq!(change.dirty[&"cut".into()], DirtyFlags::PAINT);
    assert_eq!(
        engine
            .effect_bounds(&"eraser".into())
            .unwrap()
            .unwrap()
            .max_x(),
        245.
    );
    assert_eq!(engine.world_bounds(&"parent".into()).unwrap(), parent);
    assert_eq!(engine.effect_bounds(&"parent".into()).unwrap(), parent);
    assert!(change.damage_bounds().unwrap().max_x() >= 245.);
    assert_consistent(&engine);
    let change = engine
        .set_property(
            cut,
            PropertyKey::Transform,
            serde_json::to_value(Affine::translation(300., 0.)).unwrap(),
        )
        .unwrap();
    assert_eq!(
        engine
            .world_bounds(&"eraser".into())
            .unwrap()
            .unwrap()
            .min_x(),
        410.
    );
    assert_eq!(engine.world_bounds(&"parent".into()).unwrap(), parent);
    assert!(change.damage_bounds().unwrap().max_x() >= 540.);
    assert_consistent(&engine);
}

#[test]
fn eraser_reparent_preserves_geometry_and_refreshes_both_parents_and_undo() {
    let mut scene = fixture(EraserMode::Pixel);
    scene
        .insert(
            DisplayObject::new("other", display::Group::new())
                .with_transform(Affine::translation(300., 20.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("other_ink", display::Rect::new(5., 5.)),
            Some("other".into()),
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let handle = engine.handle(&"eraser".into()).unwrap();
    let other = engine.handle(&"other".into()).unwrap();
    let before = engine.scene().clone();
    let own = engine.world_bounds(&"eraser".into()).unwrap();
    let first = engine.world_bounds(&"parent".into()).unwrap();
    let second = engine.world_bounds(&"other".into()).unwrap();
    let change = engine.reparent(handle, Some(other), None, true).unwrap();
    assert!(change.dirty[&"parent".into()].contains(DirtyFlags::STRUCTURE));
    assert!(change.dirty[&"other".into()].contains(DirtyFlags::STRUCTURE));
    assert_eq!(engine.world_bounds(&"eraser".into()).unwrap(), own);
    assert_eq!(engine.world_bounds(&"parent".into()).unwrap(), first);
    assert_eq!(engine.world_bounds(&"other".into()).unwrap(), second);
    assert_eq!(
        engine
            .hit_test(Point::new(120., 25.), Default::default())
            .unwrap()
            .unwrap()
            .path,
        vec!["other".into(), "eraser".into(), "cut".into()]
    );
    assert_consistent(&engine);
    let reparented = engine.scene().clone();
    engine
        .set_property(handle, PropertyKey::Eraser, json!("none"))
        .unwrap();
    assert_eq!(
        engine
            .world_bounds(&"other".into())
            .unwrap()
            .unwrap()
            .min_x(),
        115.
    );
    assert_consistent(&engine);
    engine.replace_scene(reparented).unwrap();
    assert_eq!(engine.world_bounds(&"other".into()).unwrap(), second);
    engine.replace_scene(before).unwrap();
    assert_eq!(engine.world_bounds(&"eraser".into()).unwrap(), own);
    assert_eq!(engine.handle(&"eraser".into()), Some(handle));
    assert_consistent(&engine);
}

#[test]
fn eraser_outside_parent_remains_pickable_and_respects_hit_children_locks_and_clips() {
    let mut engine = SceneEngine::new(fixture(EraserMode::Pixel)).unwrap();
    let point = Point::new(120., 25.);
    let parent = engine.handle(&"parent".into()).unwrap();
    let eraser = engine.handle(&"eraser".into()).unwrap();
    let cut = engine.handle(&"cut".into()).unwrap();
    assert!(
        !engine
            .world_bounds(&"parent".into())
            .unwrap()
            .unwrap()
            .contains(point)
    );
    assert_eq!(
        engine
            .hit_test(point, Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "cut"
    );
    engine
        .set_property(cut, PropertyKey::HitPolicy, json!("none"))
        .unwrap();
    assert!(
        engine
            .hit_test(point, Default::default())
            .unwrap()
            .is_none()
    );
    engine
        .set_property(
            cut,
            PropertyKey::HitPolicy,
            serde_json::to_value(HitPolicy::Painted).unwrap(),
        )
        .unwrap();
    engine
        .set_property(eraser, PropertyKey::Locked, json!(true))
        .unwrap();
    assert!(
        engine
            .hit_test(point, Default::default())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        engine
            .hit_test(
                point,
                HitTestOptions {
                    include_locked: true,
                    ..Default::default()
                }
            )
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "cut"
    );
    engine
        .set_property(eraser, PropertyKey::Locked, json!(false))
        .unwrap();
    engine
        .set_property(parent, PropertyKey::HitChildren, json!(false))
        .unwrap();
    assert!(
        engine
            .hit_test(point, Default::default())
            .unwrap()
            .is_none()
    );
    engine
        .set_property(parent, PropertyKey::HitChildren, json!(true))
        .unwrap();
    engine
        .transaction(|scene| {
            scene.update_object(&"parent".into(), |object| {
                object.shape = display::Frame::new(20., 20.).into()
            })
        })
        .unwrap();
    assert!(
        engine
            .hit_test(point, Default::default())
            .unwrap()
            .is_none()
    );
    assert_eq!(
        engine
            .world_bounds(&"eraser".into())
            .unwrap()
            .unwrap()
            .min_x(),
        115.
    );
    assert_consistent(&engine);
}

#[test]
fn dormant_erasers_masks_and_root_exclusion_match_scene_queries() {
    let mut scene = fixture(EraserMode::Pixel);
    scene
        .insert(
            DisplayObject::new("mask", display::Rect::new(5., 5.))
                .with_transform(Affine::translation(-40., 0.))
                .with_mask(MaskMode::Alpha),
            Some("parent".into()),
            Some(0),
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    assert_eq!(
        engine.local_bounds(&"parent".into()).unwrap(),
        Some(Bounds::new(Point::new(-40., 0.), Size::new(5., 5.)))
    );
    let parent_bounds = engine.world_bounds(&"parent".into()).unwrap();
    let eraser = engine.handle(&"eraser".into()).unwrap();
    for (visible, opacity) in [(false, 1.), (true, 0.), (true, 1.)] {
        engine
            .set_properties(
                eraser,
                [
                    (PropertyKey::Visible, json!(visible)),
                    (PropertyKey::Opacity, json!(opacity)),
                ],
            )
            .unwrap();
        assert_eq!(
            engine.world_bounds(&"parent".into()).unwrap(),
            parent_bounds
        );
        assert_consistent(&engine);
    }
    engine
        .set_property(
            engine.handle(&"parent".into()).unwrap(),
            PropertyKey::Eraser,
            json!("pixel"),
        )
        .unwrap();
    assert!(engine.visible_bounds().unwrap().is_none());
    assert!(engine.world_bounds(&"parent".into()).unwrap().is_some());
    assert_eq!(
        engine
            .hit_test(Point::new(120., 25.), Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "cut"
    );
    assert_consistent(&engine);
}
