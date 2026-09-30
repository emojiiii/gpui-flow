use gpui_flow_engine::*;
use gpui_flow_scene::{
    Affine, DisplayId, DisplayObject, EventKind, ListenerOptions, Point, Scene, SceneError,
    SceneEvent, display,
};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
fn scene() -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("group", display::Group::new())
            .with_transform(Affine::translation(50., 20.)),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("a", display::Rect::new(20., 30.)),
        Some("group".into()),
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("b", display::Rect::new(20., 20.))
            .with_transform(Affine::translation(40., 0.)),
        Some("group".into()),
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("other", display::Rect::new(10., 10.))
            .with_transform(Affine::translation(200., 0.)),
        None,
        None,
    )
    .unwrap();
    s
}
fn h(e: &SceneEngine, id: &str) -> ElementHandle {
    e.handle(&id.into()).unwrap()
}
#[test]
fn atomic_changes_and_noops() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let before = e.scene().clone();
    let before_cache = e.layout().clone();
    let handle = h(&e, "a");
    let invalid = e.transaction(|s| s.update_object(&"a".into(), |o| o.style.opacity = 2.));
    assert!(invalid.is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.layout(), &before_cache);
    assert_eq!(h(&e, "a"), handle);
    let no = e.transaction(|_| Ok(())).unwrap();
    assert!(!no.changed());
    assert_eq!(e.revision(), 0);
    let change = e
        .set_property(handle, PropertyKey::Name, json!("renamed"))
        .unwrap();
    assert_eq!(change.revision, 1);
    assert_eq!(change.properties.len(), 1);
    assert_eq!(change.properties[0].old, json!(""));
    assert_eq!(change.properties[0].new, json!("renamed"));
    assert!(change.damage.is_empty());
    assert_eq!(change.layout.world_recomputed, 0);
    assert_eq!(change.layout.bounds_recomputed, 0);
}
#[test]
fn detached_tree_preserves_links_handles_and_scopes() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let group = h(&e, "group");
    let child = h(&e, "a");
    let calls = Rc::new(RefCell::new(0));
    let count = calls.clone();
    e.on(
        child,
        EventKind::Click,
        ListenerOptions::default(),
        move |_| *count.borrow_mut() += 1,
    )
    .unwrap();
    let (detached, change) = e.detach(group).unwrap();
    assert_eq!(e.scene().len(), 1);
    assert_eq!(e.detached_scene(&detached).unwrap().len(), 3);
    assert_eq!(
        e.detached_scene(&detached)
            .unwrap()
            .get(&"a".into())
            .unwrap()
            .parent,
        Some("group".into())
    );
    assert_eq!(e.instance(child).unwrap().state, InstanceState::Detached);
    assert_eq!(e.listener_count(), 1);
    assert!(
        !change
            .lifecycle
            .iter()
            .any(|e| e.kind == LifecycleKind::Destroyed)
    );
    assert!(
        e.transaction(|s| s.insert(
            DisplayObject::new("a", display::Rect::new(1., 1.)),
            None,
            None
        ))
        .is_err()
    );
    e.attach(&detached, None, Some(0)).unwrap();
    assert_eq!(h(&e, "a"), child);
    assert_eq!(h(&e, "group"), group);
    assert!(e.detached_scene(&detached).is_err());
    e.dispatch(SceneEvent::new(EventKind::Click, "a")).unwrap();
    assert_eq!(*calls.borrow(), 1);
}
#[test]
fn detach_and_failed_attach_are_atomic() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let group = h(&e, "group");
    let other = h(&e, "other");
    let (d, _) = e.detach(group).unwrap();
    let revision = e.revision();
    assert!(e.attach(&d, Some(other), None).is_err());
    assert_eq!(e.revision(), revision);
    assert_eq!(e.instance(group).unwrap().state, InstanceState::Detached);
    assert_eq!(e.detached_scene(&d).unwrap().len(), 3);
}
#[test]
fn destroy_is_terminal_scoped_and_exactly_once() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let a = h(&e, "a");
    let released = Rc::new(RefCell::new(0));
    let r = released.clone();
    e.add_disposer(a, move || *r.borrow_mut() += 1).unwrap();
    e.on(a, EventKind::Click, ListenerOptions::default(), |_| {
        panic!("stale callback")
    })
    .unwrap();
    let lifecycle = Rc::new(RefCell::new(vec![]));
    let l = lifecycle.clone();
    e.on_change(Some(a), move |c| {
        l.borrow_mut().extend(c.lifecycle.iter().map(|e| e.kind))
    })
    .unwrap();
    e.destroy(a).unwrap();
    assert_eq!(*released.borrow(), 1);
    assert_eq!(e.listener_count(), 0);
    assert!(lifecycle.borrow().contains(&LifecycleKind::Destroyed));
    assert!(e.object(a).is_err());
    assert!(!e.destroy(a).unwrap().changed());
    let (new, _) = e
        .create(
            "rect",
            "a",
            json!({"size":{"width":5,"height":5}}),
            None,
            None,
        )
        .unwrap();
    assert_ne!(a, new);
    assert!(new.generation() > a.generation());
    assert!(e.instance(a).is_err());
    e.dispatch(SceneEvent::new(EventKind::Click, "a")).unwrap();
    drop(e);
    assert_eq!(*released.borrow(), 1);
}
#[test]
fn foreign_engine_handles_and_listener_tokens_rejected() {
    let mut a = SceneEngine::new(scene()).unwrap();
    let mut b = SceneEngine::new(scene()).unwrap();
    let ha = h(&a, "a");
    let hb = h(&b, "a");
    assert_ne!(ha, hb);
    assert!(b.instance(ha).is_err());
    assert!(b.set_property(ha, PropertyKey::Name, json!("bad")).is_err());
    let token = a
        .on(ha, EventKind::Click, ListenerOptions::default(), |_| {})
        .unwrap();
    let _btoken = b
        .on(hb, EventKind::Click, ListenerOptions::default(), |_| {})
        .unwrap();
    assert!(!b.off(token));
    assert_eq!(b.listener_count(), 1);
    assert!(a.off(token));
}
#[test]
fn scene_reset_retires_same_ids_and_detached_resources() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let old_a = h(&e, "a");
    let other = h(&e, "other");
    let released = Rc::new(RefCell::new(0));
    for handle in [old_a, other] {
        let r = released.clone();
        e.add_disposer(handle, move || *r.borrow_mut() += 1)
            .unwrap();
    }
    let (d, _) = e.detach(other).unwrap();
    let before = e.scene().clone();
    e.reset_scene(before).unwrap();
    assert_ne!(h(&e, "a"), old_a);
    assert_eq!(*released.borrow(), 2);
    assert!(e.detached_scene(&d).is_err());
    assert_eq!(e.instance(old_a).unwrap().state, InstanceState::Destroyed);
}
#[test]
fn history_replace_preserves_survivors_but_does_not_revive_removed_identity() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let snapshot = e.scene().clone();
    let a = h(&e, "a");
    let b = h(&e, "b");
    e.set_property(a, PropertyKey::Name, json!("edited"))
        .unwrap();
    e.replace_scene(snapshot.clone()).unwrap();
    assert_eq!(h(&e, "a"), a);
    e.destroy(b).unwrap();
    e.replace_scene(snapshot).unwrap();
    assert_ne!(h(&e, "b"), b);
    assert_eq!(h(&e, "a"), a);
}
#[test]
fn kind_change_creates_new_instance() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let a = h(&e, "a");
    let calls = Rc::new(RefCell::new(0));
    let c = calls.clone();
    e.add_disposer(a, move || *c.borrow_mut() += 1).unwrap();
    let change = e
        .transaction(|s| {
            s.update_object(&"a".into(), |o| {
                o.shape = display::Ellipse::new(20., 30.).into()
            })
        })
        .unwrap();
    assert_ne!(h(&e, "a"), a);
    assert_eq!(*calls.borrow(), 1);
    assert!(
        change
            .lifecycle
            .iter()
            .any(|ev| ev.id == DisplayId::from("a") && ev.kind == LifecycleKind::Created)
    );
}
#[test]
fn clone_mapping_retains_hierarchy_not_runtime_listeners() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let group = h(&e, "group");
    let a = h(&e, "a");
    e.on(a, EventKind::Click, ListenerOptions::default(), |_| {})
        .unwrap();
    let clone = e.clone_subtree(group, None, None).unwrap();
    assert_eq!(clone.ids.len(), 3);
    assert_eq!(
        e.scene().get(&clone.ids[&"a".into()]).unwrap().parent,
        Some(clone.ids[&"group".into()].clone())
    );
    assert_eq!(e.listener_count(), 1);
    assert_ne!(clone.root, group);
    assert_eq!(e.scene().len(), 7);
}
#[test]
fn world_preserving_reparent_and_cycle_rejection() {
    let mut s = scene();
    s.insert(
        DisplayObject::new("dest", display::Group::new()).with_transform(
            Affine::translation(-40., 50.) * Affine::rotation(43.) * Affine::scale(-2., 3.),
        ),
        None,
        None,
    )
    .unwrap();
    let mut e = SceneEngine::new(s).unwrap();
    let a = h(&e, "a");
    let dest = h(&e, "dest");
    let before = e.world_transform(&"a".into()).unwrap();
    e.reparent(a, Some(dest), None, true).unwrap();
    let after = e.world_transform(&"a".into()).unwrap();
    for point in [Point::ZERO, Point::new(12., 7.)] {
        assert!(
            before
                .transform_point(point)
                .distance(after.transform_point(point))
                < 0.0001
        );
    }
    let snapshot = e.scene().clone();
    assert!(e.reparent(dest, Some(a), None, true).is_err());
    assert_eq!(e.scene(), &snapshot);
}
#[test]
fn disposal_continues_after_callback_panic_and_engine_drop() {
    let count = Rc::new(RefCell::new(0));
    {
        let mut e = SceneEngine::new(scene()).unwrap();
        let a = h(&e, "a");
        e.add_disposer(a, || panic!("application callback"))
            .unwrap();
        let c = count.clone();
        e.add_disposer(a, move || *c.borrow_mut() += 1).unwrap();
        e.destroy(a).unwrap();
        assert_eq!(e.callback_panics(), 1);
        let group = h(&e, "group");
        let c = count.clone();
        e.add_disposer(group, move || *c.borrow_mut() += 1).unwrap();
        let _ = e.detach(group).unwrap();
    }
    assert_eq!(*count.borrow(), 2);
}
#[test]
fn lifecycle_notifications_observe_committed_changes_and_once_listener() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let a = h(&e, "a");
    let count = Rc::new(RefCell::new(0));
    let c = count.clone();
    e.on(
        a,
        EventKind::Click,
        ListenerOptions {
            once: true,
            capture: false,
        },
        move |_| *c.borrow_mut() += 1,
    )
    .unwrap();
    e.dispatch(SceneEvent::new(EventKind::Click, "a")).unwrap();
    e.dispatch(SceneEvent::new(EventKind::Click, "a")).unwrap();
    assert_eq!(*count.borrow(), 1);
    assert_eq!(e.listener_count(), 0);
    assert_eq!(e.listener_scope_count(), 0);
    let events = Rc::new(RefCell::new(vec![]));
    let c = events.clone();
    e.on_change(None, move |change| c.borrow_mut().push(change.clone()))
        .unwrap();
    let snapshot = e.scene().clone();
    assert!(
        e.transaction(|s| {
            s.remove(&"a".into())?;
            Err(SceneError::Cycle)
        })
        .is_err()
    );
    assert_eq!(e.scene(), &snapshot);
    assert!(events.borrow().is_empty());
    e.set_property(a, PropertyKey::Visible, json!(false))
        .unwrap();
    assert_eq!(events.borrow()[0].revision, e.revision());
}

#[test]
fn property_batch_coalesces_first_old_last_new_and_validates_atomically() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let a = h(&e, "a");
    let c = e
        .set_properties(
            a,
            [
                (PropertyKey::Name, json!("one")),
                (PropertyKey::Name, json!("two")),
                (PropertyKey::Opacity, json!(0.5)),
            ],
        )
        .unwrap();
    assert_eq!(c.properties.len(), 2);
    assert_eq!(
        c.properties
            .iter()
            .find(|p| p.property == "name")
            .unwrap()
            .old,
        json!("")
    );
    assert_eq!(
        c.properties
            .iter()
            .find(|p| p.property == "name")
            .unwrap()
            .new,
        json!("two")
    );
    assert_eq!(c.revision, 1);
    let snapshot = e.scene().clone();
    let cache = e.layout().clone();
    assert!(
        e.set_properties(
            a,
            [
                (PropertyKey::Name, json!("three")),
                (PropertyKey::Opacity, json!(2.))
            ]
        )
        .is_err()
    );
    assert_eq!(e.scene(), &snapshot);
    assert_eq!(e.layout(), &cache);
}
#[test]
fn invalid_reset_preserves_identity_listeners_and_disposers() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let a = h(&e, "a");
    let count = Rc::new(RefCell::new(0));
    let c = count.clone();
    e.add_disposer(a, move || *c.borrow_mut() += 1).unwrap();
    e.on(a, EventKind::Click, ListenerOptions::default(), |_| {})
        .unwrap();
    let mut invalid = e.scene().clone();
    invalid.version = 999;
    assert!(e.reset_scene(invalid).is_err());
    assert_eq!(h(&e, "a"), a);
    assert_eq!(e.listener_count(), 1);
    assert_eq!(*count.borrow(), 0);
}
#[test]
fn detached_destroy_cleans_once_and_foreign_tokens_fail() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let a = h(&e, "a");
    let count = Rc::new(RefCell::new(0));
    let c = count.clone();
    e.add_disposer(a, move || *c.borrow_mut() += 1).unwrap();
    let (d, _) = e.detach(a).unwrap();
    let mut foreign = SceneEngine::new(Scene::new()).unwrap();
    assert!(foreign.attach(&d, None, None).is_err());
    assert!(foreign.destroy_detached(&d).is_err());
    e.destroy_detached(&d).unwrap();
    assert!(e.destroy_detached(&d).is_err());
    assert_eq!(*count.borrow(), 1);
    assert!(e.handle(&"a".into()).is_none());
}
#[test]
fn repeated_once_listeners_release_scope_metadata() {
    let mut e = SceneEngine::new(scene()).unwrap();
    let a = h(&e, "a");
    for _ in 0..64 {
        e.on(
            a,
            EventKind::Click,
            ListenerOptions {
                capture: false,
                once: true,
            },
            |_| {},
        )
        .unwrap();
        e.dispatch(SceneEvent::new(EventKind::Click, "a")).unwrap();
    }
    assert_eq!(e.listener_count(), 0);
    assert_eq!(e.listener_scope_count(), 0);
}
