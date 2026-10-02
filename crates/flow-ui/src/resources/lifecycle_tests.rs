//! Ownership and settlement regressions. Native paint evidence belongs to the
//! resource_lifecycle_trace example, not to these direct adapter unit calls.
use super::*;
use gpui_flow_scene::{DisplayObject, Scene, Size, display};

fn setup() -> (SceneEngine, SceneResourceAdapter, ResourceTicket) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("image", display::Image::new("A", Size::new(40., 30.))),
            None,
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::new(scene).unwrap();
    let mut adapter = SceneResourceAdapter::new(&mut engine).unwrap();
    adapter.tracker.synchronize(&mut engine).unwrap();
    let ticket = adapter
        .tracker
        .ticket(&engine, engine.handle(&"image".into()).unwrap())
        .unwrap();
    (engine, adapter, ticket)
}
fn decoded() -> Arc<RenderImage> {
    Arc::new(RenderImage::new(vec![image::Frame::new(
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 128, 255, 255])),
    )]))
}
fn settle(
    adapter: &mut SceneResourceAdapter,
    engine: &mut SceneEngine,
    ticket: ResourceTicket,
    image: Arc<RenderImage>,
) -> Vec<ResourceEvent> {
    let mut events = vec![];
    adapter
        .settle(
            engine,
            ticket,
            ResourceOutcome::Loaded,
            Some(SceneImagePresentation::Loaded(image)),
            &mut events,
        )
        .unwrap();
    adapter.synchronize_playbacks();
    events
}
fn source(engine: &mut SceneEngine, value: &str) {
    engine
        .transaction(|scene| {
            scene.update_object(&"image".into(), |o| {
                if let Shape::Image(image) = &mut o.shape {
                    image.source = value.into();
                }
            })
        })
        .unwrap();
}

#[test]
fn external_shared_cache_retains_its_image_after_each_flow_lease_is_released() {
    let (mut engine, mut first, first_ticket) = setup();
    let mut second = SceneResourceAdapter::new(&mut engine).unwrap();
    second.tracker.synchronize(&mut engine).unwrap();
    let second_ticket = second.tracker.ticket(&engine, first_ticket.handle).unwrap();
    // Model the host cache's strong owner separately from either adapter. No
    // loader eviction or backend task cancellation is implied by these counts.
    let external_cache = BTreeMap::from([("A", decoded())]);
    let weak = Arc::downgrade(&external_cache["A"]);
    assert_eq!(Arc::strong_count(&external_cache["A"]), 1);
    assert_eq!(
        settle(
            &mut first,
            &mut engine,
            first_ticket,
            external_cache["A"].clone()
        )
        .len(),
        1
    );
    assert_eq!(
        settle(
            &mut second,
            &mut engine,
            second_ticket,
            external_cache["A"].clone()
        )
        .len(),
        1
    );
    assert_eq!(Arc::strong_count(&external_cache["A"]), 3);
    first.dispose(&mut engine).unwrap();
    assert_eq!(Arc::strong_count(&external_cache["A"]), 2);
    assert!(matches!(
        second.image(&engine, second_ticket.handle),
        SceneImagePresentation::Loaded(_)
    ));
    second.dispose(&mut engine).unwrap();
    assert_eq!(Arc::strong_count(&external_cache["A"]), 1);
    assert!(
        weak.upgrade().is_some(),
        "Flow must not evict an externally owned result"
    );
    drop(external_cache);
    assert!(weak.upgrade().is_none());
}

#[test]
fn flow_owned_decoded_lease_and_playback_are_released_on_disposal() {
    let (mut engine, mut adapter, ticket) = setup();
    let image = decoded();
    let weak = Arc::downgrade(&image);
    settle(&mut adapter, &mut engine, ticket, image);
    assert!(weak.upgrade().is_some());
    assert_eq!(adapter.playbacks.len(), 1);
    assert_eq!(adapter.timelines.len(), 1);
    adapter.dispose(&mut engine).unwrap();
    assert!(
        weak.upgrade().is_none(),
        "no external cache owns this result"
    );
    assert!(adapter.images.is_empty());
    assert!(adapter.playbacks.is_empty());
    assert!(adapter.timelines.is_empty());
    assert!(adapter.dispose(&mut engine).unwrap().is_empty());
}

#[test]
fn source_remount_and_destroy_reject_and_drop_late_decoded_images() {
    for mutation in 0..3 {
        let (mut engine, mut adapter, old) = setup();
        match mutation {
            0 => {
                source(&mut engine, "B");
                source(&mut engine, "A");
            }
            1 => {
                let (subtree, _) = engine.detach(old.handle).unwrap();
                engine.attach(&subtree, None, None).unwrap();
            }
            _ => {
                engine.destroy(old.handle).unwrap();
                engine
                    .transaction(|scene| {
                        scene.insert(
                            DisplayObject::new(
                                "image",
                                display::Image::new("A", Size::new(40., 30.)),
                            ),
                            None,
                            None,
                        )
                    })
                    .unwrap();
            }
        }
        let events = adapter.tracker.synchronize(&mut engine).unwrap();
        assert_eq!(events.len(), 2);
        assert!(matches!(events[0].state, ResourceState::Cancelled(_)));
        assert_eq!(events[0].dispatched, mutation == 0);
        let handle = engine.handle(&"image".into()).unwrap();
        let current = adapter.tracker.ticket(&engine, handle).unwrap();
        assert_ne!(old, current);
        let image = decoded();
        let weak = Arc::downgrade(&image);
        assert!(settle(&mut adapter, &mut engine, old, image).is_empty());
        assert!(weak.upgrade().is_none(), "stale Arc must not be retained");
        assert!(matches!(
            adapter.image(&engine, handle),
            SceneImagePresentation::Pending
        ));
        assert_eq!(adapter.tracker.counts(&engine).pending, 1);
        assert_eq!(
            settle(&mut adapter, &mut engine, current, decoded()).len(),
            1
        );
        assert_eq!(adapter.tracker.counts(&engine).loaded, 1);
        adapter.dispose(&mut engine).unwrap();
    }
}

#[test]
fn terminal_failure_or_cancellation_cannot_be_overwritten_by_late_success() {
    for outcome in [
        ResourceOutcome::Failed("decode failure".into()),
        ResourceOutcome::Cancelled(ResourceCancellation::NotAuthorized),
    ] {
        let (mut engine, mut adapter, ticket) = setup();
        let expected = ResourceState::from(outcome.clone());
        let mut events = vec![];
        adapter
            .settle(&mut engine, ticket, outcome, None, &mut events)
            .unwrap();
        assert_eq!(events.len(), 1);
        let image = decoded();
        let weak = Arc::downgrade(&image);
        assert!(settle(&mut adapter, &mut engine, ticket, image).is_empty());
        assert!(weak.upgrade().is_none());
        assert_eq!(adapter.tracker.state(&engine, ticket), Some(expected));
        assert!(adapter.images.is_empty());
        adapter.dispose(&mut engine).unwrap();
    }
}

#[test]
fn cache_replacement_releases_flow_image_without_evicting_external_owner() {
    let (mut engine, mut adapter, ticket) = setup();
    let external = decoded();
    settle(&mut adapter, &mut engine, ticket, external.clone());
    assert_eq!(Arc::strong_count(&external), 2);
    adapter.set_image_cache(&engine, None).unwrap();
    assert_eq!(Arc::strong_count(&external), 1);
    assert!(adapter.playbacks.is_empty());
    assert!(adapter.timelines.is_empty());
    assert!(settle(&mut adapter, &mut engine, ticket, external.clone()).is_empty());
    assert_eq!(Arc::strong_count(&external), 1);
    adapter.tracker.synchronize(&mut engine).unwrap();
    let fresh = adapter.tracker.ticket(&engine, ticket.handle).unwrap();
    assert_ne!(fresh, ticket);
    assert_eq!(
        adapter.tracker.state(&engine, fresh),
        Some(ResourceState::Pending)
    );
    adapter.dispose(&mut engine).unwrap();
}
