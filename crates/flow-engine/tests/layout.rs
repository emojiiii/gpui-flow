use gpui_flow_engine::*;
use gpui_flow_scene::{Affine, Color, DisplayObject, HitTestOptions, Point, Scene, display};
use serde_json::json;
fn fixture() -> Scene {
    let mut s = Scene::new();
    s.insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    for (id, x) in [("a", 0.), ("b", 100.)] {
        s.insert(
            DisplayObject::new(id, display::Rect::new(20., 20.))
                .with_transform(Affine::translation(x, 0.)),
            Some("g".into()),
            None,
        )
        .unwrap();
    }
    s.insert(
        DisplayObject::new("other", display::Rect::new(5., 5.))
            .with_transform(Affine::translation(300., 0.)),
        None,
        None,
    )
    .unwrap();
    s
}
fn compare(engine: &SceneEngine) {
    for node in engine.scene().nodes() {
        let id = &node.object.id;
        assert_eq!(
            engine.world_transform(id).unwrap(),
            engine.scene().world_transform(id).unwrap()
        );
        assert_eq!(
            engine.local_bounds(id).unwrap(),
            engine.scene().local_bounds(id).unwrap()
        );
        assert_eq!(
            engine.world_bounds(id).unwrap(),
            engine.scene().world_bounds(id).unwrap()
        );
    }
    let a = engine.paint_list().unwrap();
    let b = engine.scene().paint_list().unwrap();
    assert_eq!(a.len(), b.len());
    for (a, b) in a.iter().zip(b) {
        assert_eq!(a.object.id, b.object.id);
        assert_eq!(a.world_transform, b.world_transform);
        assert_eq!(a.opacity, b.opacity);
        assert_eq!(a.clips, b.clips);
    }
}
#[test]
fn paint_only_change_skips_geometry_and_updates_damage() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    let a = e.handle(&"a".into()).unwrap();
    let c = e
        .set_property(
            a,
            PropertyKey::Fill,
            json!({"type":"solid","color":{"r":255,"g":0,"b":0,"a":255}}),
        )
        .unwrap();
    assert_eq!(c.layout.world_recomputed, 0);
    assert_eq!(c.layout.bounds_recomputed, 0);
    assert_eq!(c.layout.paint_invalidated, 1);
    assert!(!c.damage.is_empty());
    compare(&e);
}
#[test]
fn moving_child_refreshes_ancestors_and_covers_old_and_new() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    let before = e.world_bounds(&"a".into()).unwrap().unwrap();
    let a = e.handle(&"a".into()).unwrap();
    let c = e
        .set_property(
            a,
            PropertyKey::Transform,
            serde_json::to_value(Affine::translation(200., 30.)).unwrap(),
        )
        .unwrap();
    assert_eq!(c.layout.world_recomputed, 1);
    assert_eq!(c.layout.bounds_recomputed, 1);
    let damage = c.damage_bounds().unwrap();
    assert!(damage.contains(before.origin));
    assert!(damage.contains(Point::new(220., 50.)));
    compare(&e);
}
#[test]
fn parent_transform_and_opacity_update_descendants_not_unrelated_branch() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    let g = e.handle(&"g".into()).unwrap();
    let c = e
        .set_property(
            g,
            PropertyKey::Transform,
            serde_json::to_value(Affine::rotation(36.) * Affine::scale(-2., 3.)).unwrap(),
        )
        .unwrap();
    assert_eq!(c.layout.world_recomputed, 3);
    compare(&e);
    e.set_property(g, PropertyKey::Opacity, json!(0.4)).unwrap();
    assert_eq!(e.layout().get(&"a".into()).unwrap().opacity, 0.4);
    assert_eq!(e.layout().get(&"other".into()).unwrap().opacity, 1.);
    compare(&e);
}
#[test]
fn removal_damage_covers_vacated_area_and_cached_bounds_shrink() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    let b = e.handle(&"b".into()).unwrap();
    let c = e.destroy(b).unwrap();
    assert!(c.damage_bounds().unwrap().contains(Point::new(120., 20.)));
    assert_eq!(
        e.local_bounds(&"g".into()).unwrap().unwrap().size.width,
        20.
    );
    compare(&e);
}
#[test]
fn clipping_changes_invalidate_child_cache_and_picking() {
    let mut s = fixture();
    s.update_object(&"g".into(), |o| {
        o.shape = display::Frame::new(30., 30.).into()
    })
    .unwrap();
    let mut e = SceneEngine::new(s).unwrap();
    assert!(
        e.hit_test(Point::new(105., 5.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    e.transaction(|s| {
        s.update_object(&"g".into(), |o| {
            o.shape = display::Frame::new(150., 30.).into()
        })
    })
    .unwrap();
    assert_eq!(
        e.hit_test(Point::new(105., 5.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "b"
    );
    compare(&e);
}
#[test]
fn full_and_incremental_match_deterministic_many_edits() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    for i in 0..40 {
        let id = if i % 2 == 0 { "a" } else { "b" };
        e.transaction(|s| {
            s.update_object(&id.into(), |o| {
                o.transform = Affine::translation(i as f32 * 7., i as f32 * -3.)
                    * Affine::rotation(i as f32 * 11.);
                o.style.fill = Some(Color::rgb(i * 3, 20, 60).into());
            })
        })
        .unwrap();
        compare(&e);
    }
}
#[test]
fn z_order_is_stable_and_cache_queries_are_immediate() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    e.transaction(|s| {
        s.update_object(&"a".into(), |o| o.z_index = 10)?;
        s.update_object(&"b".into(), |o| o.transform = Affine::IDENTITY)
    })
    .unwrap();
    assert_eq!(
        e.hit_test(Point::new(5., 5.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "a"
    );
    assert_eq!(e.layout().revision, e.revision());
    compare(&e);
}
#[test]
fn serialized_snapshot_excludes_runtime_and_preserves_document() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    let a = e.handle(&"a".into()).unwrap();
    e.add_disposer(a, || {}).unwrap();
    e.set_property(a, PropertyKey::Data, json!({"app":42}))
        .unwrap();
    let json = e.scene().to_json().unwrap();
    for word in [
        "generation",
        "disposers",
        "listeners",
        "world_transform",
        "revision",
    ] {
        assert!(!json.contains(word));
    }
    assert_eq!(Scene::from_json(&json).unwrap(), *e.scene());
}

#[test]
fn singular_transform_skips_picking_and_lock_does_not_hide_geometry() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    let a = e.handle(&"a".into()).unwrap();
    e.set_property(a, PropertyKey::Locked, json!(true)).unwrap();
    assert!(
        e.hit_test(Point::new(5., 5.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert!(
        e.hit_test(
            Point::new(5., 5.),
            HitTestOptions {
                include_locked: true,
                ..Default::default()
            }
        )
        .unwrap()
        .is_some()
    );
    assert!(
        e.paint_list()
            .unwrap()
            .iter()
            .any(|p| p.object.id.as_str() == "a")
    );
    e.set_property(
        a,
        PropertyKey::Transform,
        serde_json::to_value(Affine::scale(0., 1.)).unwrap(),
    )
    .unwrap();
    assert!(e.world_to_local(&"a".into(), Point::ZERO).is_err());
    assert!(
        e.hit_test(
            Point::ZERO,
            HitTestOptions {
                include_locked: true,
                ..Default::default()
            }
        )
        .unwrap()
        .is_none()
    );
}
#[test]
fn changed_child_hit_policy_propagates_without_stale_descendant_cache() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    let g = e.handle(&"g".into()).unwrap();
    e.set_property(g, PropertyKey::HitChildren, json!(false))
        .unwrap();
    assert!(!e.layout().get(&"a".into()).unwrap().hit_enabled);
    assert!(
        e.hit_test(Point::new(5., 5.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    e.set_property(g, PropertyKey::HitChildren, json!(true))
        .unwrap();
    assert!(
        e.hit_test(Point::new(5., 5.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    compare(&e);
}
#[test]
fn removing_parent_clip_refreshes_descendant_paint_and_hit_cache() {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("frame", display::Frame::new(30., 30.)),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("child", display::Rect::new(80., 80.)),
        Some("frame".into()),
        None,
    )
    .unwrap();
    let mut e = SceneEngine::new(s).unwrap();
    assert!(
        e.hit_test(Point::new(60., 60.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    e.transaction(|s| s.update_object(&"frame".into(), |o| o.shape = display::Group::new().into()))
        .unwrap();
    compare(&e);
    assert_eq!(
        e.hit_test(Point::new(60., 60.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "child"
    );
}

fn next_random(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    *state >> 32
}
fn random_affine(state: &mut u64) -> Affine {
    let x = (next_random(state) % 601) as f32 - 300.;
    let y = (next_random(state) % 401) as f32 - 200.;
    let angle = (next_random(state) % 360) as f32;
    let skew = (next_random(state) % 41) as f32 - 20.;
    let sx = 0.3 + (next_random(state) % 170) as f32 / 100.;
    let sy = 0.3 + (next_random(state) % 170) as f32 / 100.;
    let sign = if next_random(state) & 1 == 0 { -1. } else { 1. };
    Affine::translation(x, y)
        * Affine::rotation(angle)
        * Affine::skew(skew, -skew / 2.)
        * Affine::scale(sign * sx, sy)
}
fn random_fixture(state: &mut u64) -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    for branch in 0..4 {
        let mut group = DisplayObject::new(format!("g{branch}"), display::Group::new())
            .with_transform(random_affine(state));
        if branch % 2 == 0 {
            group.shape = display::Frame::new(100., 80.).into();
        }
        scene.insert(group, Some("root".into()), None).unwrap();
        scene
            .insert(
                DisplayObject::new(format!("nested{branch}"), display::Group::new())
                    .with_transform(random_affine(state)),
                Some(format!("g{branch}").into()),
                None,
            )
            .unwrap();
        for child in 0..8 {
            scene
                .insert(
                    DisplayObject::new(
                        format!("leaf{}", branch * 8 + child),
                        display::Rect::new(10. + child as f32, 12.),
                    )
                    .with_transform(random_affine(state)),
                    Some(format!("nested{branch}").into()),
                    None,
                )
                .unwrap();
        }
    }
    scene
}
fn compare_full_rebuild(engine: &SceneEngine) {
    // Scene methods independently recompute matrices, geometry and traversal.
    compare(engine);
    // A cold engine discards every cached dependency, also checking effective
    // opacity, visibility, locks, hit inheritance, inverse, clips and effects.
    let fresh = SceneEngine::new(engine.scene().clone()).unwrap();
    assert_eq!(engine.layout().len(), fresh.layout().len());
    for (id, actual) in engine.layout().entries() {
        let mut expected = fresh.layout().get(id).unwrap().clone();
        expected.revision = actual.revision;
        assert_eq!(actual, &expected, "full cache mismatch at {id}");
    }
    for point in [
        Point::ZERO,
        Point::new(10., 15.),
        Point::new(-125., 92.),
        Point::new(275., -110.),
    ] {
        for include_locked in [false, true] {
            let options = HitTestOptions {
                include_locked,
                include_invisible: true,
                ..Default::default()
            };
            assert_eq!(
                engine.hit_test_all(point, options).unwrap(),
                engine.scene().hit_test_all(point, options).unwrap()
            );
        }
    }
}

#[test]
fn seeded_affine_reparent_remount_and_undo_match_full_reference() {
    for seed in [0x5eed_u64, 0xcafe_f00d, 0x1234_5678] {
        let mut state = seed;
        let mut engine = SceneEngine::new(random_fixture(&mut state)).unwrap();
        for step in 0..160 {
            let leaf = format!("leaf{}", next_random(&mut state) % 32);
            let handle = engine.handle(&leaf.as_str().into()).unwrap();
            let branch = next_random(&mut state) % 4;
            match step % 10 {
                0 | 1 => {
                    let transform = random_affine(&mut state);
                    engine
                        .set_property(
                            handle,
                            PropertyKey::Transform,
                            serde_json::to_value(transform).unwrap(),
                        )
                        .unwrap();
                }
                2 => {
                    let parent = engine.handle(&format!("nested{branch}").into());
                    engine
                        .reparent(handle, parent, Some(0), step % 20 == 2)
                        .unwrap();
                }
                3 => {
                    let nested = engine.handle(&format!("nested{branch}").into()).unwrap();
                    let (detached, _) = engine.detach(nested).unwrap();
                    compare_full_rebuild(&engine);
                    engine
                        .attach(
                            &detached,
                            engine.handle(&format!("g{}", (branch + 1) % 4).into()),
                            Some(0),
                        )
                        .unwrap();
                    assert_eq!(
                        engine.handle(&format!("nested{branch}").into()),
                        Some(nested)
                    );
                }
                4 => {
                    let group = engine.handle(&format!("g{branch}").into()).unwrap();
                    engine
                        .set_property(
                            group,
                            PropertyKey::Transform,
                            serde_json::to_value(random_affine(&mut state)).unwrap(),
                        )
                        .unwrap();
                }
                5 => {
                    engine
                        .transaction(|scene| {
                            scene.update_object(&leaf.as_str().into(), |o| {
                                o.shape = display::Rect::new(
                                    3. + (state % 40) as f32,
                                    7. + (state % 50) as f32,
                                )
                                .into()
                            })
                        })
                        .unwrap();
                }
                6 => {
                    let group = engine.handle(&format!("nested{branch}").into()).unwrap();
                    engine
                        .set_properties(
                            group,
                            [
                                (PropertyKey::Opacity, json!((state % 100) as f32 / 100.)),
                                (PropertyKey::Visible, json!(state & 1 == 0)),
                                (PropertyKey::Locked, json!(state & 2 == 0)),
                                (PropertyKey::HitChildren, json!(state & 4 == 0)),
                            ],
                        )
                        .unwrap();
                }
                7 => {
                    let before = engine.scene().clone();
                    engine
                        .set_property(
                            handle,
                            PropertyKey::Transform,
                            serde_json::to_value(random_affine(&mut state)).unwrap(),
                        )
                        .unwrap();
                    engine.replace_scene(before).unwrap();
                }
                8 => {
                    engine
                        .transaction(|scene| {
                            scene.update_object(&format!("g{branch}").into(), |o| {
                                o.shape = if state & 1 == 0 {
                                    display::Frame::new(65., 73.).into()
                                } else {
                                    display::Group::new().into()
                                }
                            })
                        })
                        .unwrap();
                }
                _ => {
                    engine.reset_scene(engine.scene().clone()).unwrap();
                }
            }
            compare_full_rebuild(&engine);
        }
    }
}

fn benchmark_fixture() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    for branch in 0..32 {
        let group = format!("g{branch}");
        scene
            .insert(
                DisplayObject::new(group.clone(), display::Group::new())
                    .with_transform(Affine::translation(branch as f32 * 30., 0.)),
                Some("root".into()),
                None,
            )
            .unwrap();
        for child in 0..16 {
            let index = branch * 16 + child;
            scene
                .insert(
                    DisplayObject::new(format!("leaf{index}"), display::Rect::new(10., 10.))
                        .with_name(format!("item{index}"))
                        .with_class(if child == 0 { "rare" } else { "common" })
                        .with_transform(Affine::translation(0., child as f32 * 12.)),
                    Some(group.clone().into()),
                    None,
                )
                .unwrap();
        }
    }
    scene
}

/// Reproducible CPU experiment. Times are observations, never pass thresholds.
/// Run explicitly; normal tests do not spend time measuring scheduler noise.
#[test]
#[ignore = "CPU benchmark: run --test layout benchmark_query_cache_cpu -- --ignored --nocapture"]
fn benchmark_query_cache_cpu() {
    use std::{hint::black_box, time::Instant};
    const QUERIES: usize = 10_000;
    const EDITS: usize = 100;
    let fixture = benchmark_fixture();
    let mut engine = SceneEngine::new(fixture.clone()).unwrap();
    let query = ElementQuery {
        name: Some("item271"),
        ..Default::default()
    };
    let expected = engine.query(query).unwrap();
    let (_, query_work) = engine.query_with_stats(query).unwrap();
    assert_eq!(
        query_work,
        QueryStats {
            candidates_tested: 1,
            matches: 1
        }
    );
    let started = Instant::now();
    for _ in 0..QUERIES {
        assert_eq!(black_box(engine.query(black_box(query)).unwrap()), expected);
    }
    let indexed_time = started.elapsed();
    let started = Instant::now();
    for _ in 0..QUERIES {
        let result: Vec<_> = engine
            .scene()
            .nodes()
            .filter(|node| node.object.name == black_box("item271"))
            .map(|node| engine.handle(&node.object.id).unwrap())
            .collect();
        assert_eq!(black_box(result), expected);
    }
    let scan_time = started.elapsed();
    let leaf = engine.handle(&"leaf271".into()).unwrap();
    let started = Instant::now();
    let mut work = LayoutStats::default();
    for step in 1..=EDITS {
        let change = engine
            .set_property(
                leaf,
                PropertyKey::Transform,
                serde_json::to_value(Affine::translation(step as f32, step as f32 / 2.)).unwrap(),
            )
            .unwrap();
        work.visited += change.layout.visited;
        work.world_recomputed += change.layout.world_recomputed;
        work.bounds_recomputed += change.layout.bounds_recomputed;
        assert_eq!(change.layout.world_recomputed, 1);
        assert_eq!(change.layout.bounds_recomputed, 2); // the two ancestors
        assert_eq!(engine.query_index_stats().order_nodes_rebuilt, 0);
        assert_eq!(engine.query_index_stats().metadata_updated, 0);
    }
    let commit_time = started.elapsed();
    let started = Instant::now();
    let mut cold_work = LayoutStats::default();
    for _ in 0..EDITS {
        let cold = black_box(SceneEngine::new(engine.scene().clone()).unwrap());
        cold_work.visited += cold.layout().stats.visited;
        cold_work.world_recomputed += cold.layout().stats.world_recomputed;
        cold_work.bounds_recomputed += cold.layout().stats.bounds_recomputed;
    }
    let cold_time = started.elapsed();
    compare_full_rebuild(&engine);
    assert_eq!(work.visited, fixture.len() * EDITS);
    let metadata = engine
        .set_property(leaf, PropertyKey::Name, json!("renamed"))
        .unwrap();
    assert_eq!(metadata.layout.world_recomputed, 0);
    assert_eq!(metadata.layout.bounds_recomputed, 0);
    assert_eq!(
        engine.query_index_stats(),
        QueryIndexStats {
            metadata_examined: 1,
            metadata_updated: 1,
            order_nodes_rebuilt: 0
        }
    );
    println!(
        "DAT-04 CPU fixture_nodes={} queries={} edits={} indexed_query_us={} scan_query_us={} indexed_candidates={} scan_candidates={} incremental_commit_us={} cold_engine_build_us={} incremental_visited={} incremental_world={} incremental_bounds={} cold_visited={} cold_world={} cold_bounds={}",
        fixture.len(),
        QUERIES,
        EDITS,
        indexed_time.as_micros(),
        scan_time.as_micros(),
        query_work.candidates_tested * QUERIES,
        fixture.len() * QUERIES,
        commit_time.as_micros(),
        cold_time.as_micros(),
        work.visited,
        work.world_recomputed,
        work.bounds_recomputed,
        cold_work.visited,
        cold_work.world_recomputed,
        cold_work.bounds_recomputed
    );
}

/// Same pre-index public API and workload on both snapshots. The measured region
/// excludes fixture construction, engine setup, and final oracle validation.
#[test]
#[ignore = "CPU commit baseline: run explicitly with --ignored --nocapture"]
fn benchmark_commit_pipeline_cpu() {
    use std::{hint::black_box, time::Instant};
    const EDITS: usize = 100;
    for operation in ["transform", "rename", "reparent"] {
        let mut engine = SceneEngine::new(benchmark_fixture()).unwrap();
        let leaf = engine.handle(&"leaf271".into()).unwrap();
        let parents = [
            engine.handle(&"g16".into()).unwrap(),
            engine.handle(&"g17".into()).unwrap(),
        ];
        let started = Instant::now();
        let mut work = LayoutStats::default();
        for step in 0..EDITS {
            let change = match operation {
                "transform" => engine
                    .set_property(
                        leaf,
                        PropertyKey::Transform,
                        serde_json::to_value(Affine::translation(
                            (step + 1) as f32,
                            (step + 1) as f32 / 2.,
                        ))
                        .unwrap(),
                    )
                    .unwrap(),
                "rename" => engine
                    .set_property(leaf, PropertyKey::Name, json!(format!("renamed{step}")))
                    .unwrap(),
                "reparent" => engine
                    .reparent(leaf, Some(parents[(step + 1) % 2]), Some(0), false)
                    .unwrap(),
                _ => unreachable!(),
            };
            assert!(change.changed());
            work.visited += change.layout.visited;
            work.world_recomputed += change.layout.world_recomputed;
            work.bounds_recomputed += change.layout.bounds_recomputed;
            work.paint_invalidated += change.layout.paint_invalidated;
            black_box(&engine);
        }
        let elapsed = started.elapsed();
        assert_eq!(engine.revision(), EDITS as u64);
        assert_eq!(work.visited, 545 * EDITS);
        assert_eq!(engine.handle(&"leaf271".into()), Some(leaf));
        for node in engine.scene().nodes() {
            let id = &node.object.id;
            assert_eq!(
                engine.world_transform(id).unwrap(),
                engine.scene().world_transform(id).unwrap()
            );
            assert_eq!(
                engine.local_bounds(id).unwrap(),
                engine.scene().local_bounds(id).unwrap()
            );
            assert_eq!(
                engine.world_bounds(id).unwrap(),
                engine.scene().world_bounds(id).unwrap()
            );
        }
        println!(
            "DAT-04 SAME-COMMIT operation={operation} nodes=545 edits={EDITS} elapsed_us={} visited={} world={} bounds={} paint={}",
            elapsed.as_micros(),
            work.visited,
            work.world_recomputed,
            work.bounds_recomputed,
            work.paint_invalidated
        );
    }
}
