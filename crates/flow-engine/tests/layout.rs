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
