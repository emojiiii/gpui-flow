use gpui_flow_engine::*;
use gpui_flow_scene::{Affine, DisplayId, DisplayObject, Scene, display};
use serde_json::json;

fn fixture() -> Scene {
    let mut scene = Scene::new();
    for root in ["z-root", "a-root"] {
        scene
            .insert(
                DisplayObject::new(root, display::Group::new())
                    .with_name("group")
                    .with_class("shared"),
                None,
                None,
            )
            .unwrap();
    }
    for (id, parent, name, class, z) in [
        ("z-leaf", "z-root", "same", "hot", 4),
        ("a-leaf", "z-root", "same", "cold", -1),
        ("middle", "z-root", "other", "hot", -1),
        ("outside", "a-root", "same", "hot", 0),
    ] {
        let mut object = DisplayObject::new(id, display::Rect::new(10., 10.))
            .with_name(name)
            .with_class(class)
            .with_class("shared");
        object.z_index = z;
        scene.insert(object, Some(parent.into()), None).unwrap();
    }
    scene
}
fn ids(engine: &SceneEngine, query: ElementQuery<'_>) -> Vec<String> {
    engine
        .query(query)
        .unwrap()
        .into_iter()
        .map(|h| engine.instance(h).unwrap().id.0)
        .collect()
}
fn all() -> ElementQuery<'static> {
    ElementQuery::default()
}

/// Independent traversal and predicate oracle: no engine index or cached order.
fn scan(engine: &SceneEngine, query: ElementQuery<'_>) -> Vec<ElementHandle> {
    let scene = engine.scene();
    let roots = query
        .subtree
        .map(|h| vec![engine.instance(h).unwrap().id])
        .unwrap_or_else(|| scene.roots().to_vec());
    let mut out = vec![];
    fn walk(
        engine: &SceneEngine,
        mut ids: Vec<DisplayId>,
        query: ElementQuery<'_>,
        out: &mut Vec<ElementHandle>,
    ) {
        if query.order == QueryOrder::Paint {
            ids.sort_by_key(|id| engine.scene().get(id).unwrap().object.z_index);
        }
        for id in ids {
            let node = engine.scene().get(&id).unwrap();
            let object = &node.object;
            let root_excluded = !query.include_root
                && query
                    .subtree
                    .is_some_and(|h| engine.instance(h).unwrap().id == id);
            if !root_excluded
                && query.id.is_none_or(|v| v == &id)
                && query.name.is_none_or(|v| v == object.name)
                && query.tag.is_none_or(|v| v == object.shape.tag())
                && query.class.is_none_or(|v| object.classes.contains(v))
            {
                out.push(engine.handle(&id).unwrap());
            }
            walk(engine, node.children.clone(), query, out);
        }
    }
    walk(engine, roots, query, &mut out);
    out
}
fn compare(engine: &SceneEngine) {
    let attached: Vec<_> = engine
        .scene()
        .nodes()
        .map(|n| n.object.id.clone())
        .collect();
    for order in [QueryOrder::Tree, QueryOrder::Paint] {
        for name in [
            None,
            Some("same"),
            Some("changed"),
            Some(""),
            Some("absent"),
        ] {
            for tag in [None, Some("rect"), Some("group"), Some("ellipse")] {
                for class in [None, Some("shared"), Some("hot"), Some("cold")] {
                    let query = ElementQuery {
                        name,
                        tag,
                        class,
                        order,
                        ..all()
                    };
                    assert_eq!(engine.query(query).unwrap(), scan(engine, query));
                }
            }
        }
        for root in &attached {
            let subtree = engine.handle(root);
            for include_root in [false, true] {
                for id in attached.iter().map(Some).chain(std::iter::once(None)) {
                    let query = ElementQuery {
                        id,
                        subtree,
                        include_root,
                        order,
                        ..all()
                    };
                    assert_eq!(engine.query(query).unwrap(), scan(engine, query));
                }
            }
        }
    }
}

#[test]
fn exact_conditions_use_tree_or_stable_paint_order() {
    let engine = SceneEngine::new(fixture()).unwrap();
    assert_eq!(
        ids(&engine, all()),
        ["z-root", "z-leaf", "a-leaf", "middle", "a-root", "outside"]
    );
    assert_eq!(
        ids(
            &engine,
            ElementQuery {
                order: QueryOrder::Paint,
                ..all()
            }
        ),
        ["z-root", "a-leaf", "middle", "z-leaf", "a-root", "outside"]
    );
    assert_eq!(
        ids(
            &engine,
            ElementQuery {
                name: Some("same"),
                tag: Some("rect"),
                class: Some("hot"),
                ..all()
            }
        ),
        ["z-leaf", "outside"]
    );
    assert!(
        ids(
            &engine,
            ElementQuery {
                tag: Some("Rect"),
                ..all()
            }
        )
        .is_empty()
    );
    assert!(
        ids(
            &engine,
            ElementQuery {
                class: Some("hot shared"),
                ..all()
            }
        )
        .is_empty()
    );
    compare(&engine);
}

#[test]
fn subtree_is_inclusive_optional_root_and_scope_validated_before_empty_match() {
    let mut engine = SceneEngine::new(fixture()).unwrap();
    let root = engine.handle(&"z-root".into()).unwrap();
    assert_eq!(
        ids(
            &engine,
            ElementQuery {
                subtree: Some(root),
                include_root: false,
                ..all()
            }
        ),
        ["z-leaf", "a-leaf", "middle"]
    );
    assert_eq!(
        ids(
            &engine,
            ElementQuery {
                subtree: Some(root),
                tag: Some("group"),
                ..all()
            }
        ),
        ["z-root"]
    );
    assert!(
        ids(
            &engine,
            ElementQuery {
                subtree: Some(root),
                id: Some(&"outside".into()),
                ..all()
            }
        )
        .is_empty()
    );
    let foreign = SceneEngine::new(fixture())
        .unwrap()
        .handle(&"z-root".into())
        .unwrap();
    assert_eq!(
        engine.query(ElementQuery {
            subtree: Some(foreign),
            name: Some("absent"),
            ..all()
        }),
        Err(EngineError::InvalidHandle)
    );
    let (token, _) = engine.detach(root).unwrap();
    assert_eq!(
        engine.query(ElementQuery {
            subtree: Some(root),
            name: Some("absent"),
            ..all()
        }),
        Err(EngineError::DetachedHandle)
    );
    engine.destroy_detached(&token).unwrap();
    assert_eq!(
        engine.query(ElementQuery {
            subtree: Some(root),
            ..all()
        }),
        Err(EngineError::InvalidHandle)
    );
}

#[test]
fn indexes_update_metadata_kind_and_z_order_without_unrelated_rebuilds() {
    let mut engine = SceneEngine::new(fixture()).unwrap();
    let leaf = engine.handle(&"z-leaf".into()).unwrap();
    engine
        .set_properties(
            leaf,
            [
                (PropertyKey::Name, json!("changed")),
                (PropertyKey::Classes, json!(["new", "shared", "new"])),
            ],
        )
        .unwrap();
    assert_eq!(
        engine.query_index_stats(),
        QueryIndexStats {
            metadata_examined: 1,
            metadata_updated: 1,
            order_nodes_rebuilt: 0
        }
    );
    assert_eq!(
        ids(
            &engine,
            ElementQuery {
                name: Some("changed"),
                class: Some("new"),
                ..all()
            }
        ),
        ["z-leaf"]
    );
    assert_eq!(
        ids(
            &engine,
            ElementQuery {
                name: Some("same"),
                class: Some("hot"),
                ..all()
            }
        ),
        ["outside"]
    );
    engine
        .transaction(|s| {
            s.update_object(&"z-leaf".into(), |o| {
                o.shape = display::Ellipse::new(10., 10.).into()
            })
        })
        .unwrap();
    assert_eq!(engine.query_index_stats().metadata_updated, 1);
    assert_ne!(
        engine
            .query(ElementQuery {
                tag: Some("ellipse"),
                ..all()
            })
            .unwrap(),
        vec![leaf]
    );
    assert_eq!(
        ids(
            &engine,
            ElementQuery {
                tag: Some("ellipse"),
                ..all()
            }
        ),
        ["z-leaf"]
    );
    let fresh = engine.handle(&"z-leaf".into()).unwrap();
    engine
        .set_properties(
            fresh,
            [
                (PropertyKey::ZIndex, json!(-4)),
                (PropertyKey::Visible, json!(false)),
                (PropertyKey::Locked, json!(true)),
            ],
        )
        .unwrap();
    assert_eq!(engine.query_index_stats().metadata_updated, 0);
    assert_eq!(
        engine.query_index_stats().order_nodes_rebuilt,
        engine.scene().len()
    );
    assert_eq!(
        ids(
            &engine,
            ElementQuery {
                order: QueryOrder::Paint,
                ..all()
            }
        )[1],
        "z-leaf"
    );
    compare(&engine);
}

#[test]
fn lifecycle_reparent_detach_remount_destroy_clone_and_reused_id_never_alias() {
    let mut engine = SceneEngine::new(fixture()).unwrap();
    let root = engine.handle(&"z-root".into()).unwrap();
    let leaf = engine.handle(&"z-leaf".into()).unwrap();
    let other = engine.handle(&"a-root".into()).unwrap();
    engine.reparent(leaf, Some(other), Some(0), true).unwrap();
    compare(&engine);
    assert!(
        ids(
            &engine,
            ElementQuery {
                subtree: Some(root),
                id: Some(&"z-leaf".into()),
                ..all()
            }
        )
        .is_empty()
    );
    let (token, _) = engine.detach(other).unwrap();
    assert_eq!(engine.handle(&"z-leaf".into()), Some(leaf));
    assert!(
        ids(
            &engine,
            ElementQuery {
                id: Some(&"z-leaf".into()),
                ..all()
            }
        )
        .is_empty()
    );
    compare(&engine);
    engine.attach(&token, Some(root), Some(0)).unwrap();
    assert_eq!(
        engine
            .query(ElementQuery {
                id: Some(&"z-leaf".into()),
                ..all()
            })
            .unwrap(),
        vec![leaf]
    );
    compare(&engine);
    let clone = engine.clone_subtree(leaf, Some(root), None).unwrap();
    assert_ne!(clone.root, leaf);
    compare(&engine);
    engine.destroy(leaf).unwrap();
    engine
        .create("rect", "z-leaf", json!({}), Some(root), None)
        .unwrap();
    assert_ne!(
        engine
            .query(ElementQuery {
                id: Some(&"z-leaf".into()),
                ..all()
            })
            .unwrap(),
        vec![leaf]
    );
    compare(&engine);
}

#[test]
fn undo_redo_reconciliation_reset_and_detached_destruction_keep_indexes_current() {
    let mut engine = SceneEngine::new(fixture()).unwrap();
    let before = engine.scene().clone();
    let leaf = engine.handle(&"z-leaf".into()).unwrap();
    engine
        .set_property(leaf, PropertyKey::Name, json!("changed"))
        .unwrap();
    let after = engine.scene().clone();
    engine.replace_scene(before.clone()).unwrap();
    assert_eq!(
        engine
            .query(ElementQuery {
                id: Some(&"z-leaf".into()),
                ..all()
            })
            .unwrap(),
        vec![leaf]
    );
    compare(&engine);
    engine.replace_scene(after).unwrap();
    compare(&engine);
    let root = engine.handle(&"a-root".into()).unwrap();
    let (token, _) = engine.detach(root).unwrap();
    engine.reset_scene(before).unwrap();
    assert_eq!(
        engine.query(ElementQuery {
            subtree: Some(root),
            ..all()
        }),
        Err(EngineError::InvalidHandle)
    );
    assert_ne!(
        engine
            .query(ElementQuery {
                id: Some(&"z-leaf".into()),
                ..all()
            })
            .unwrap(),
        vec![leaf]
    );
    assert!(engine.destroy_detached(&token).is_err());
    compare(&engine);
    // Identical reset changes every identity even when metadata/order need no work.
    let old = engine.query(all()).unwrap();
    engine.reset_scene(engine.scene().clone()).unwrap();
    assert!(
        old.into_iter()
            .all(|h| !engine.query(all()).unwrap().contains(&h))
    );
    compare(&engine);
    let (token, _) = engine
        .detach(engine.handle(&"a-root".into()).unwrap())
        .unwrap();
    engine.destroy_detached(&token).unwrap();
    assert_eq!(engine.query_index_stats(), QueryIndexStats::default());
    compare(&engine);
    engine.reset_scene(Scene::new()).unwrap();
    assert!(engine.query(all()).unwrap().is_empty());
}

#[test]
fn late_invalid_batch_failed_tree_edit_and_noop_leave_indexes_untouched() {
    let mut engine = SceneEngine::new(fixture()).unwrap();
    let leaf = engine.handle(&"z-leaf".into()).unwrap();
    let root = engine.handle(&"z-root".into()).unwrap();
    let before = engine.scene().clone();
    let revision = engine.revision();
    let stats = engine.query_index_stats();
    let handles = engine.query(all()).unwrap();
    assert!(
        engine
            .set_properties_batch(
                ChangeOrigin::Edit,
                [
                    (leaf, PropertyKey::Name, json!("changed")),
                    (leaf, PropertyKey::Classes, json!(["new"])),
                    (root, PropertyKey::Opacity, json!(-1))
                ]
            )
            .is_err()
    );
    assert!(engine.reparent(root, Some(leaf), None, false).is_err());
    assert!(
        !engine
            .set_property(leaf, PropertyKey::Name, json!("same"))
            .unwrap()
            .changed()
    );
    assert_eq!(engine.scene(), &before);
    assert_eq!(engine.revision(), revision);
    assert_eq!(engine.query_index_stats(), stats);
    assert_eq!(engine.query(all()).unwrap(), handles);
    compare(&engine);
}

#[test]
fn smallest_index_bucket_and_subtree_limit_candidate_work() {
    let engine = SceneEngine::new(fixture()).unwrap();
    let (_, stats) = engine
        .query_with_stats(ElementQuery {
            id: Some(&"outside".into()),
            ..all()
        })
        .unwrap();
    assert_eq!(
        stats,
        QueryStats {
            candidates_tested: 1,
            matches: 1
        }
    );
    let (_, stats) = engine
        .query_with_stats(ElementQuery {
            class: Some("cold"),
            tag: Some("rect"),
            ..all()
        })
        .unwrap();
    assert_eq!(
        stats,
        QueryStats {
            candidates_tested: 1,
            matches: 1
        }
    );
    let (_, stats) = engine
        .query_with_stats(ElementQuery {
            subtree: engine.handle(&"a-root".into()),
            class: Some("shared"),
            ..all()
        })
        .unwrap();
    assert_eq!(
        stats,
        QueryStats {
            candidates_tested: 2,
            matches: 2
        }
    );
}

#[test]
fn seeded_index_mutations_match_independent_full_tree_scan() {
    for seed in [1_u64, 0x5eed, 0xdead_beef] {
        let mut rng = seed;
        let mut engine = SceneEngine::new(fixture()).unwrap();
        for step in 0..100 {
            rng = rng
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            let choice = rng >> 32;
            let leaf = ["z-leaf", "a-leaf", "middle", "outside"][choice as usize % 4];
            let handle = engine.handle(&leaf.into()).unwrap();
            match step % 8 {
                0 => {
                    engine
                        .set_properties(
                            handle,
                            [
                                (
                                    PropertyKey::Name,
                                    json!(if choice & 1 == 0 { "changed" } else { "same" }),
                                ),
                                (
                                    PropertyKey::Classes,
                                    json!(if choice & 2 == 0 {
                                        vec!["shared", "hot"]
                                    } else {
                                        vec!["cold"]
                                    }),
                                ),
                            ],
                        )
                        .unwrap();
                }
                1 => {
                    engine
                        .set_property(handle, PropertyKey::ZIndex, json!((choice % 9) as i32 - 4))
                        .unwrap();
                }
                2 => {
                    let parent =
                        engine.handle(&if choice & 1 == 0 { "z-root" } else { "a-root" }.into());
                    engine.reparent(handle, parent, Some(0), false).unwrap();
                }
                3 => {
                    let (token, _) = engine.detach(handle).unwrap();
                    compare(&engine);
                    engine
                        .attach(&token, engine.handle(&"z-root".into()), None)
                        .unwrap();
                }
                4 => {
                    let before = engine.scene().clone();
                    engine
                        .set_property(handle, PropertyKey::Name, json!("temporary"))
                        .unwrap();
                    engine.replace_scene(before).unwrap();
                }
                5 => {
                    engine
                        .transaction(|s| {
                            s.update_object(&leaf.into(), |o| {
                                o.shape = if choice & 1 == 0 {
                                    display::Ellipse::new(8., 12.).into()
                                } else {
                                    display::Rect::new(8., 12.).into()
                                }
                            })
                        })
                        .unwrap();
                }
                6 => {
                    engine
                        .set_property(
                            handle,
                            PropertyKey::Transform,
                            serde_json::to_value(
                                Affine::translation((choice % 90) as f32, step as f32)
                                    * Affine::rotation((choice % 360) as f32),
                            )
                            .unwrap(),
                        )
                        .unwrap();
                }
                _ => {
                    engine.reset_scene(engine.scene().clone()).unwrap();
                }
            }
            compare(&engine);
        }
    }
}
