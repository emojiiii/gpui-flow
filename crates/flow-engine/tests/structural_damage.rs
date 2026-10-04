use gpui_flow_engine::{DirtyFlags, LifecycleKind, SceneEngine, StructureKind};
use gpui_flow_scene::{Affine, Color, DisplayObject, Scene, Shadow, Style, display};

fn rect(id: &str, x: f32) -> DisplayObject {
    DisplayObject::new(id, display::Rect::new(20., 20.)).with_transform(Affine::translation(x, 0.))
}

fn roots(ids: &[&str]) -> Scene {
    let mut scene = Scene::new();
    for (index, id) in ids.iter().enumerate() {
        scene
            .insert(rect(id, index as f32 * 40.), None, None)
            .unwrap();
    }
    scene
        .insert(
            DisplayObject::new("far", display::Rect::new(1000., 1000.))
                .with_transform(Affine::translation(2000., 0.)),
            None,
            None,
        )
        .unwrap();
    scene
}

#[test]
fn removing_and_restoring_first_root_only_damages_that_root() {
    let scene = roots(&["local", "survivor"]);
    let mut engine = SceneEngine::new(scene.clone()).unwrap();
    let old_layout = engine.layout().clone();
    let local = engine.handle(&"local".into()).unwrap();
    let local_bounds = engine.effect_bounds(&"local".into()).unwrap().unwrap();

    let removed = engine.destroy(local).unwrap();
    assert_eq!(removed.structure.len(), 1);
    assert_eq!(removed.structure[0].id.as_str(), "local");
    assert_eq!(removed.structure[0].kind, StructureKind::Removed);
    assert_eq!(removed.structure[0].old_index, Some(0));
    assert_eq!(removed.structure[0].new_index, None);
    assert_eq!(removed.damage, vec![local_bounds]);
    assert_eq!(removed.layout.world_recomputed, 0);
    assert_eq!(removed.layout.bounds_recomputed, 0);
    assert_eq!(
        removed
            .lifecycle
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![LifecycleKind::Unmounted, LifecycleKind::Destroyed]
    );
    for id in ["survivor", "far"] {
        assert!(!removed.dirty.contains_key(&id.into()));
        assert_eq!(engine.layout().get(&id.into()), old_layout.get(&id.into()));
    }

    let added = engine.replace_scene(scene).unwrap();
    assert_eq!(added.structure.len(), 1);
    assert_eq!(added.structure[0].id.as_str(), "local");
    assert_eq!(added.structure[0].kind, StructureKind::Added);
    assert_eq!(added.structure[0].old_index, None);
    assert_eq!(added.structure[0].new_index, Some(0));
    assert_eq!(added.damage, vec![local_bounds]);
    assert_eq!(
        added
            .lifecycle
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![LifecycleKind::Created, LifecycleKind::Mounted]
    );
    assert_ne!(engine.handle(&"local".into()).unwrap(), local);
    for id in ["survivor", "far"] {
        assert!(!added.dirty.contains_key(&id.into()));
        assert_eq!(engine.layout().get(&id.into()), old_layout.get(&id.into()));
    }
}

#[test]
fn detach_and_reattach_preserve_survivor_caches_and_live_identity() {
    let mut engine = SceneEngine::new(roots(&["local", "survivor"])).unwrap();
    let old_layout = engine.layout().clone();
    let local = engine.handle(&"local".into()).unwrap();
    let local_bounds = engine.effect_bounds(&"local".into()).unwrap().unwrap();
    let (detached, removed) = engine.detach(local).unwrap();
    let added = engine.attach(&detached, None, Some(0)).unwrap();
    assert_eq!(engine.handle(&"local".into()).unwrap(), local);
    assert_eq!(
        removed
            .lifecycle
            .iter()
            .map(|event| event.kind)
            .collect::<Vec<_>>(),
        vec![LifecycleKind::Unmounted, LifecycleKind::Detached]
    );
    assert_eq!(added.lifecycle.len(), 1);
    assert_eq!(added.lifecycle[0].kind, LifecycleKind::Mounted);
    for change in [removed, added] {
        assert_eq!(change.structure.len(), 1);
        assert_eq!(change.structure[0].id.as_str(), "local");
        assert_eq!(change.damage, vec![local_bounds]);
        for id in ["survivor", "far"] {
            assert!(!change.dirty.contains_key(&id.into()));
            assert_eq!(engine.layout().get(&id.into()), old_layout.get(&id.into()));
        }
    }
}

#[test]
fn real_root_reorder_keeps_order_damage_and_absolute_positions() {
    let mut engine = SceneEngine::new(roots(&["a", "b"])).unwrap();
    let far = engine.layout().get(&"far".into()).unwrap().clone();
    let b = engine.handle(&"b".into()).unwrap();
    let change = engine.reparent(b, None, Some(0), false).unwrap();
    assert_eq!(change.structure.len(), 2);
    for (id, old_index, new_index) in [("a", 0, 1), ("b", 1, 0)] {
        let record = change
            .structure
            .iter()
            .find(|record| record.id.as_str() == id)
            .unwrap();
        assert_eq!(record.kind, StructureKind::Reordered);
        assert_eq!(record.old_index, Some(old_index));
        assert_eq!(record.new_index, Some(new_index));
        assert_eq!(
            change.dirty[&id.into()],
            DirtyFlags::ORDER | DirtyFlags::PAINT
        );
        assert!(
            change
                .damage
                .contains(&engine.effect_bounds(&id.into()).unwrap().unwrap())
        );
    }
    assert!(change.lifecycle.is_empty());
    assert!(!change.dirty.contains_key(&"far".into()));
    assert_eq!(engine.layout().get(&"far".into()), Some(&far));
    assert!(change.damage_bounds().unwrap().max_x() < 2000.);
    assert_eq!(engine.scene().roots()[0].as_str(), "b");
}

#[test]
fn insertion_or_removal_cannot_hide_reorder_at_unchanged_absolute_index() {
    for remove_prefix in [false, true] {
        let scene = if remove_prefix {
            roots(&["prefix", "a", "b"])
        } else {
            roots(&["a", "b"])
        };
        let mut engine = SceneEngine::new(scene).unwrap();
        let change = engine
            .transaction(|scene| {
                if remove_prefix {
                    scene.remove(&"prefix".into())?;
                    scene.reparent(&"b".into(), None, Some(0), false)?;
                } else {
                    scene.insert(rect("prefix", 120.), None, Some(0))?;
                    scene.reparent(&"b".into(), None, Some(1), false)?;
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(change.structure.len(), 3);
        for id in ["a", "b"] {
            let record = change
                .structure
                .iter()
                .find(|record| record.id.as_str() == id)
                .unwrap();
            assert_eq!(record.kind, StructureKind::Reordered);
            assert_eq!(
                change.dirty[&id.into()],
                DirtyFlags::ORDER | DirtyFlags::PAINT
            );
            assert!(
                change
                    .damage
                    .contains(&engine.effect_bounds(&id.into()).unwrap().unwrap())
            );
        }
        // Removal offsets a's move; insertion offsets b's move. Neither may
        // disappear merely because its numeric array position stayed at 1.
        let unchanged_index = if remove_prefix { "a" } else { "b" };
        let record = change
            .structure
            .iter()
            .find(|record| record.id.as_str() == unchanged_index)
            .unwrap();
        assert_eq!(record.old_index, Some(1));
        assert_eq!(record.new_index, Some(1));
        assert!(!change.dirty.contains_key(&"far".into()));
        assert!(change.damage_bounds().unwrap().max_x() < 2000.);
    }
}

#[test]
fn reparenting_preserves_sibling_order_and_refreshes_both_parent_effects() {
    let mut scene = Scene::new();
    for (id, x) in [("from", 0.), ("to", 500.)] {
        scene
            .insert(
                DisplayObject::new(id, display::Group::new())
                    .with_transform(Affine::translation(x, 0.))
                    .with_style(Style {
                        shadow: Some(Shadow::new(40., 0., 0., Color::BLACK)),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
    }
    for (id, parent, x) in [
        ("moving", "from", 0.),
        ("a", "from", 80.),
        ("b", "from", 160.),
        ("c", "to", 80.),
        ("d", "to", 160.),
    ] {
        scene
            .insert(rect(id, x), Some(parent.into()), None)
            .unwrap();
    }
    scene.insert(rect("far", 2000.), None, None).unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let old_layout = engine.layout().clone();
    let moving = engine.handle(&"moving".into()).unwrap();
    let to = engine.handle(&"to".into()).unwrap();
    let change = engine.reparent(moving, Some(to), Some(0), false).unwrap();
    assert_eq!(change.structure.len(), 1);
    let record = &change.structure[0];
    assert_eq!(record.id.as_str(), "moving");
    assert_eq!(record.kind, StructureKind::Reparented);
    assert_eq!(record.old_parent, Some("from".into()));
    assert_eq!(record.new_parent, Some("to".into()));
    assert_eq!(record.old_index, Some(0));
    assert_eq!(record.new_index, Some(0));
    assert_eq!(change.lifecycle.len(), 1);
    assert_eq!(change.lifecycle[0].kind, LifecycleKind::Reparented);
    assert_eq!(change.lifecycle[0].handle, moving);
    for id in ["a", "b", "c", "d", "far"] {
        assert!(!change.dirty.contains_key(&id.into()), "{id}");
    }
    assert_eq!(
        engine.layout().get(&"far".into()),
        old_layout.get(&"far".into())
    );
    for id in ["from", "to"] {
        assert_eq!(change.dirty[&id.into()], DirtyFlags::STRUCTURE);
        let old = old_layout.get(&id.into()).unwrap();
        let new = engine.layout().get(&id.into()).unwrap();
        assert_ne!(old.local_bounds, new.local_bounds);
        assert_ne!(old.effect_bounds, new.effect_bounds);
        assert!(change.damage.contains(&old.effect_bounds.unwrap()));
        assert!(change.damage.contains(&new.effect_bounds.unwrap()));
    }
    let reference = SceneEngine::new(engine.scene().clone()).unwrap();
    for (id, entry) in engine.layout().entries() {
        let full = reference.layout().get(id).unwrap();
        assert_eq!(entry.world_transform, full.world_transform);
        assert_eq!(entry.local_bounds, full.local_bounds);
        assert_eq!(entry.world_bounds, full.world_bounds);
        assert_eq!(entry.effect_bounds, full.effect_bounds);
    }
}

#[test]
fn child_insert_remove_refresh_parent_bounds_without_reordering_survivors() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("parent", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    for (id, x) in [("a", 80.), ("b", 160.)] {
        scene
            .insert(rect(id, x), Some("parent".into()), None)
            .unwrap();
    }
    let mut engine = SceneEngine::new(scene).unwrap();
    let initial = engine.local_bounds(&"parent".into()).unwrap();
    let added = engine
        .transaction(|scene| scene.insert(rect("local", 0.), Some("parent".into()), Some(0)))
        .unwrap();
    assert_eq!(
        engine
            .local_bounds(&"parent".into())
            .unwrap()
            .unwrap()
            .min_x(),
        0.
    );
    let local = engine.handle(&"local".into()).unwrap();
    let removed = engine.destroy(local).unwrap();
    assert_eq!(engine.local_bounds(&"parent".into()).unwrap(), initial);
    for change in [added, removed] {
        assert_eq!(change.structure.len(), 1);
        assert_eq!(change.structure[0].id.as_str(), "local");
        assert_eq!(change.dirty[&"parent".into()], DirtyFlags::STRUCTURE);
        for id in ["a", "b"] {
            assert!(!change.dirty.contains_key(&id.into()));
        }
    }
}
