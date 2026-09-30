//! Authored regression cases. This source slice was not compiled or executed.
use gpui_flow_engine::*;
use gpui_flow_scene::{DisplayObject, EventKind, ListenerOptions, Scene, Shape, Size, display};
use std::{cell::RefCell, rc::Rc};

fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("root", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("image", display::Image::new("a.png", Size::new(40., 30.))),
            Some("root".into()),
            None,
        )
        .unwrap();
    SceneEngine::new(scene).unwrap()
}
fn handle(engine: &SceneEngine) -> ElementHandle {
    engine.handle(&"image".into()).unwrap()
}
fn source(engine: &mut SceneEngine, source: &str) {
    engine
        .transaction(|scene| {
            scene.update_object(&"image".into(), |object| {
                let Shape::Image(image) = &mut object.shape else {
                    panic!("image");
                };
                image.source = source.into();
            })
        })
        .unwrap();
}
fn setup() -> (SceneEngine, SceneResources, ResourceTicket) {
    let mut engine = engine();
    let mut resources = SceneResources::new(&mut engine).unwrap();
    resources.synchronize(&mut engine).unwrap();
    let ticket = resources.ticket(&engine, handle(&engine)).unwrap();
    (engine, resources, ticket)
}
#[test]
fn real_results_dispatch_to_live_element_and_bubble_exactly_once() {
    let mut engine = engine();
    let calls = Rc::new(RefCell::new(vec![]));
    for id in ["root", "image"] {
        let observed = calls.clone();
        engine
            .on(
                engine.handle(&id.into()).unwrap(),
                EventKind::Custom("resource.loaded".into()),
                ListenerOptions::default(),
                move |event| {
                    assert!(!event.cancelable);
                    assert_eq!(event.data["resource"], "image");
                    assert_eq!(event.data["state"], "loaded");
                    observed.borrow_mut().push(event.current_target.to_string());
                },
            )
            .unwrap();
    }
    let mut resources = SceneResources::new(&mut engine).unwrap();
    let changes = resources.synchronize(&mut engine).unwrap();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].state, ResourceState::Pending);
    assert!(changes[0].dispatched);
    let ticket = resources.ticket(&engine, handle(&engine)).unwrap();
    let (accepted, event) = resources
        .complete(&mut engine, ticket, ResourceOutcome::Loaded)
        .unwrap();
    assert_eq!(accepted, ResourceCompletion::Accepted);
    assert!(event.unwrap().dispatched);
    assert_eq!(&*calls.borrow(), &["image", "root"]);
    assert_eq!(
        resources
            .complete(&mut engine, ticket, ResourceOutcome::Failed("late".into()))
            .unwrap(),
        (ResourceCompletion::AlreadySettled, None)
    );
    assert_eq!(calls.borrow().len(), 2);
    assert_eq!(
        resources.counts(&engine),
        ResourceCounts {
            loaded: 1,
            ..Default::default()
        }
    );
}
#[test]
fn detach_before_render_discards_late_completion_and_remount_uses_new_epoch() {
    let (mut engine, mut resources, old) = setup();
    let (detached, _) = engine.detach(old.handle).unwrap();
    assert_eq!(
        resources
            .complete(&mut engine, old, ResourceOutcome::Loaded)
            .unwrap(),
        (ResourceCompletion::Stale, None)
    );
    let cancelled = resources.synchronize(&mut engine).unwrap();
    assert_eq!(cancelled.len(), 1);
    assert_eq!(
        cancelled[0].state,
        ResourceState::Cancelled(ResourceCancellation::Detached)
    );
    assert!(!cancelled[0].dispatched);
    assert_eq!(resources.counts(&engine).total(), 0);
    let root = engine.handle(&"root".into()).unwrap();
    engine.attach(&detached, Some(root), None).unwrap();
    resources.synchronize(&mut engine).unwrap();
    let new = resources.ticket(&engine, old.handle).unwrap();
    assert_ne!(new.attachment_epoch, old.attachment_epoch);
    assert_ne!(new.generation, old.generation);
    assert_eq!(
        resources
            .complete(&mut engine, old, ResourceOutcome::Loaded)
            .unwrap()
            .0,
        ResourceCompletion::Stale
    );
    assert_eq!(resources.counts(&engine).pending, 1);
}
#[test]
fn detach_and_remount_between_observations_never_resume_old_loading() {
    let (mut engine, mut resources, old) = setup();
    let (detached, _) = engine.detach(old.handle).unwrap();
    let root = engine.handle(&"root".into()).unwrap();
    engine.attach(&detached, Some(root), None).unwrap();
    assert_eq!(
        resources
            .complete(&mut engine, old, ResourceOutcome::Loaded)
            .unwrap()
            .0,
        ResourceCompletion::Stale
    );
    let events = resources.synchronize(&mut engine).unwrap();
    assert_eq!(events.len(), 2);
    assert!(
        !events[0].dispatched,
        "old cancellation cannot address a new attachment"
    );
    assert!(events[1].dispatched);
}
#[test]
fn a_b_a_commits_invalidate_source_even_without_intermediate_render() {
    let (mut engine, mut resources, old) = setup();
    source(&mut engine, "b.png");
    source(&mut engine, "a.png");
    assert_eq!(
        resources
            .complete(&mut engine, old, ResourceOutcome::Loaded)
            .unwrap(),
        (ResourceCompletion::Stale, None)
    );
    let events = resources.synchronize(&mut engine).unwrap();
    assert_eq!(
        events[0].state,
        ResourceState::Cancelled(ResourceCancellation::Superseded)
    );
    assert_eq!(events[1].state, ResourceState::Pending);
    assert!(events.iter().all(|event| event.dispatched));
    let new = resources.ticket(&engine, old.handle).unwrap();
    assert_ne!(new.generation, old.generation);
    assert_eq!(new.attachment_epoch, old.attachment_epoch);
}
#[test]
fn destroyed_id_reuse_does_not_receive_old_resource_result() {
    let (mut engine, mut resources, old) = setup();
    engine.destroy(old.handle).unwrap();
    engine
        .transaction(|scene| {
            scene.insert(
                DisplayObject::new("image", display::Image::new("a.png", Size::new(40., 30.))),
                Some("root".into()),
                None,
            )
        })
        .unwrap();
    resources.synchronize(&mut engine).unwrap();
    let new = resources.ticket(&engine, handle(&engine)).unwrap();
    assert_ne!(new.handle, old.handle);
    assert_eq!(
        resources
            .complete(&mut engine, old, ResourceOutcome::Loaded)
            .unwrap()
            .0,
        ResourceCompletion::Stale
    );
    assert_eq!(resources.counts(&engine).pending, 1);
}
#[test]
fn foreign_engine_cannot_consume_tickets_or_retarget_tracker() {
    let (engine, mut resources, ticket) = setup();
    let mut other = self::engine();
    assert!(
        resources
            .complete(&mut other, ticket, ResourceOutcome::Loaded)
            .is_err()
    );
    assert!(resources.synchronize(&mut other).is_err());
    assert!(resources.frame(&other).is_none());
    assert_eq!(resources.counts(&engine).pending, 1);
}
#[test]
fn ready_does_not_mean_loaded_and_completion_includes_failure_and_cancellation() {
    let (mut engine, mut resources, ticket) = setup();
    let ready = resources.layout_ready(&engine).unwrap();
    assert_eq!(ready.milestone, ResourceViewMilestone::Ready);
    assert_eq!(ready.counts.pending, 1);
    assert!(resources.layout_ready(&engine).is_none());
    let first_paint = resources.frame(&engine).unwrap();
    let events = resources.frame_drawn(&engine, first_paint);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].milestone, ResourceViewMilestone::ViewReady);
    assert!(resources.frame_drawn(&engine, first_paint).is_empty());
    resources
        .complete(
            &mut engine,
            ticket,
            ResourceOutcome::Failed("decode error".into()),
        )
        .unwrap();
    assert!(
        resources.frame_drawn(&engine, first_paint).is_empty(),
        "pending frame is stale"
    );
    let painted_failure = resources.frame(&engine).unwrap();
    let complete = resources.frame_drawn(&engine, painted_failure);
    assert_eq!(complete.len(), 1);
    assert_eq!(complete[0].milestone, ResourceViewMilestone::Complete);
    assert_eq!(complete[0].counts.failed, 1);
    assert!(resources.frame_drawn(&engine, painted_failure).is_empty());
    source(&mut engine, "https://unapproved.example/image.png");
    resources.synchronize(&mut engine).unwrap();
    let ticket = resources.ticket(&engine, ticket.handle).unwrap();
    resources
        .complete(
            &mut engine,
            ticket,
            ResourceOutcome::Cancelled(ResourceCancellation::NotAuthorized),
        )
        .unwrap();
    let frame = resources.frame(&engine).unwrap();
    let complete = resources.frame_drawn(&engine, frame);
    assert_eq!(complete.len(), 1);
    assert_eq!(complete[0].counts.cancelled, 1);
    assert!(
        resources.layout_ready(&engine).is_none(),
        "Ready is per document"
    );
}
#[test]
fn stale_paint_receipt_cannot_mark_new_scene_ready() {
    let (mut engine, mut resources, ticket) = setup();
    resources
        .complete(&mut engine, ticket, ResourceOutcome::Loaded)
        .unwrap();
    let old_frame = resources.frame(&engine).unwrap();
    source(&mut engine, "b.png");
    assert!(
        resources.frame(&engine).is_none(),
        "must synchronize a new resource set"
    );
    assert!(resources.frame_drawn(&engine, old_frame).is_empty());
    resources.synchronize(&mut engine).unwrap();
    assert!(resources.frame_drawn(&engine, old_frame).is_empty());
    let new_frame = resources.frame(&engine).unwrap();
    let events = resources.frame_drawn(&engine, new_frame);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].milestone, ResourceViewMilestone::ViewReady);
}
#[test]
fn no_resource_scene_completes_only_after_paint_receipt() {
    let mut engine = SceneEngine::new(Scene::new()).unwrap();
    let mut resources = SceneResources::new(&mut engine).unwrap();
    assert!(resources.synchronize(&mut engine).unwrap().is_empty());
    assert_eq!(resources.layout_ready(&engine).unwrap().counts.total(), 0);
    let frame = resources.frame(&engine).unwrap();
    let events = resources.frame_drawn(&engine, frame);
    assert_eq!(
        events
            .iter()
            .map(|event| event.milestone)
            .collect::<Vec<_>>(),
        vec![
            ResourceViewMilestone::ViewReady,
            ResourceViewMilestone::Complete
        ]
    );
    assert!(resources.frame_drawn(&engine, frame).is_empty());
}
#[test]
fn capacity_rejection_has_no_partial_pending_entries_or_events() {
    let mut engine = engine();
    engine
        .transaction(|scene| {
            scene.insert(
                DisplayObject::new("text", display::Text::new("hello", Size::new(40., 30.))),
                None,
                None,
            )
        })
        .unwrap();
    let mut resources = SceneResources::with_limit(&mut engine, 1).unwrap();
    assert_eq!(
        resources.synchronize(&mut engine).unwrap_err(),
        ResourceError::Capacity {
            limit: 1,
            requested: 2
        }
    );
    assert_eq!(resources.counts(&engine).total(), 0);
    assert!(resources.frame(&engine).is_none());
}
#[test]
fn repeated_source_changes_keep_pending_counts_bounded_and_disposal_is_terminal() {
    let (mut engine, mut resources, first) = setup();
    for index in 0..100 {
        source(&mut engine, &format!("{index}.png"));
        resources.synchronize(&mut engine).unwrap();
        assert_eq!(resources.counts(&engine).pending, 1);
        assert_eq!(resources.counts(&engine).total(), 1);
        assert_eq!(
            resources
                .complete(&mut engine, first, ResourceOutcome::Loaded)
                .unwrap()
                .0,
            ResourceCompletion::Stale
        );
    }
    let ticket = resources.ticket(&engine, first.handle).unwrap();
    let events = resources.dispose(&mut engine).unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].state,
        ResourceState::Cancelled(ResourceCancellation::Disposed)
    );
    assert_eq!(resources.counts(&engine).total(), 0);
    assert!(resources.dispose(&mut engine).unwrap().is_empty());
    assert_eq!(
        resources
            .complete(&mut engine, ticket, ResourceOutcome::Loaded)
            .unwrap_err(),
        ResourceError::Disposed
    );
    assert!(resources.frame(&engine).is_none());
}
#[test]
fn resource_observations_do_not_change_document_layout_or_revision() {
    let (mut engine, mut resources, ticket) = setup();
    let scene = engine.scene().clone();
    let revision = engine.revision();
    let layout = engine.layout().clone();
    resources
        .complete(&mut engine, ticket, ResourceOutcome::Loaded)
        .unwrap();
    resources.layout_ready(&engine);
    let frame = resources.frame(&engine).unwrap();
    resources.frame_drawn(&engine, frame);
    assert_eq!(engine.scene(), &scene);
    assert_eq!(engine.layout(), &layout);
    assert_eq!(engine.revision(), revision);
}
#[test]
fn image_fit_and_size_changes_do_not_reload_bytes() {
    let (mut engine, mut resources, ticket) = setup();
    engine
        .transaction(|scene| {
            scene.update_object(&"image".into(), |object| {
                let Shape::Image(image) = &mut object.shape else {
                    panic!("image");
                };
                image.fit = display::ImageFit::Cover;
                image.size = Size::new(80., 90.);
            })
        })
        .unwrap();
    assert!(resources.synchronize(&mut engine).unwrap().is_empty());
    assert_eq!(resources.ticket(&engine, ticket.handle), Some(ticket));
}
#[test]
fn canonical_text_change_and_host_font_invalidation_create_new_generations() {
    let mut engine = SceneEngine::new(Scene::new()).unwrap();
    engine
        .transaction(|scene| {
            scene.insert(
                DisplayObject::new("text", display::Text::new("", Size::new(80., 30.))),
                None,
                None,
            )
        })
        .unwrap();
    let mut resources = SceneResources::new(&mut engine).unwrap();
    resources.synchronize(&mut engine).unwrap();
    let handle = engine.handle(&"text".into()).unwrap();
    let old = resources.ticket(&engine, handle).unwrap();
    resources
        .complete(&mut engine, old, ResourceOutcome::Loaded)
        .unwrap();
    resources.invalidate(&engine, ResourceKind::Text).unwrap();
    assert!(resources.frame(&engine).is_none());
    resources.synchronize(&mut engine).unwrap();
    let new = resources.ticket(&engine, handle).unwrap();
    assert_ne!(old.generation, new.generation);
    engine
        .transaction(|scene| {
            scene.update_object(&"text".into(), |object| {
                let Shape::Text(text) = &mut object.shape else {
                    panic!("text");
                };
                text.content = "now present".into();
            })
        })
        .unwrap();
    assert_eq!(
        resources
            .complete(&mut engine, new, ResourceOutcome::Loaded)
            .unwrap()
            .0,
        ResourceCompletion::Stale
    );
    assert_eq!(
        engine.object(handle).unwrap().shape,
        Shape::Text(display::Text::new("now present", Size::new(80., 30.)))
    );
}
#[test]
fn reparent_and_ordinary_properties_do_not_replace_resource_identity() {
    let (mut engine, mut resources, ticket) = setup();
    engine
        .set_property(
            ticket.handle,
            PropertyKey::Name,
            serde_json::json!("renamed"),
        )
        .unwrap();
    assert!(resources.synchronize(&mut engine).unwrap().is_empty());
    assert_eq!(resources.ticket(&engine, ticket.handle), Some(ticket));
}

#[test]
fn replacement_or_second_tracker_cannot_accept_another_owners_ticket_or_frame() {
    let (mut engine, mut old, ticket) = setup();
    let old_frame = old.frame(&engine).unwrap();
    let mut second = SceneResources::new(&mut engine).unwrap();
    second.synchronize(&mut engine).unwrap();
    let second_ticket = second.ticket(&engine, ticket.handle).unwrap();
    assert_ne!(second_ticket, ticket);
    assert_eq!(
        second
            .complete(&mut engine, ticket, ResourceOutcome::Loaded)
            .unwrap()
            .0,
        ResourceCompletion::Stale
    );
    assert!(second.frame_drawn(&engine, old_frame).is_empty());
    old.dispose(&mut engine).unwrap();
    let mut replacement = SceneResources::new(&mut engine).unwrap();
    replacement.synchronize(&mut engine).unwrap();
    assert_ne!(replacement.ticket(&engine, ticket.handle), Some(ticket));
    assert_eq!(
        replacement
            .complete(&mut engine, ticket, ResourceOutcome::Loaded)
            .unwrap()
            .0,
        ResourceCompletion::Stale
    );
}
