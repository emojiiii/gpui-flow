//! Multi-resource receipt acceptance. These exercise the tracker contract;
//! the native example supplies actual GPUI paint callbacks separately.
use gpui_flow_engine::*;
use gpui_flow_scene::{DisplayObject, Scene, Size, display};

fn document() -> Scene {
    let mut scene = Scene::new();
    for id in ["loaded", "failed", "cancelled"] {
        scene
            .insert(
                DisplayObject::new(id, display::Image::new(id, Size::new(20., 20.))),
                None,
                None,
            )
            .unwrap();
    }
    scene
}
#[test]
fn mixed_terminal_batch_needs_fresh_receipt_and_emits_each_milestone_once() {
    let mut engine = SceneEngine::new(document()).unwrap();
    let mut tracker = SceneResources::new(&mut engine).unwrap();
    let events = tracker.synchronize(&mut engine).unwrap();
    assert_eq!(events.len(), 3);
    let ready = tracker.layout_ready(&engine).unwrap();
    assert_eq!(ready.milestone, ResourceViewMilestone::Ready);
    assert_eq!(ready.counts.pending, 3);
    let pending_receipt = tracker.frame(&engine).unwrap();
    for (id, outcome) in [
        ("loaded", ResourceOutcome::Loaded),
        ("failed", ResourceOutcome::Failed("producer error".into())),
        (
            "cancelled",
            ResourceOutcome::Cancelled(ResourceCancellation::NotAuthorized),
        ),
    ] {
        let ticket = tracker
            .ticket(&engine, engine.handle(&id.into()).unwrap())
            .unwrap();
        assert_eq!(
            tracker.complete(&mut engine, ticket, outcome).unwrap().0,
            ResourceCompletion::Accepted
        );
    }
    assert!(
        tracker.frame_drawn(&engine, pending_receipt).is_empty(),
        "an old pending paint must not infer terminal presentation"
    );
    assert!(tracker.layout_ready(&engine).is_none());
    let terminal_receipt = tracker.frame(&engine).unwrap();
    let milestones = tracker.frame_drawn(&engine, terminal_receipt);
    assert_eq!(
        milestones.iter().map(|e| e.milestone).collect::<Vec<_>>(),
        vec![
            ResourceViewMilestone::ViewReady,
            ResourceViewMilestone::Complete
        ]
    );
    for milestone in milestones {
        assert_eq!(
            milestone.counts,
            ResourceCounts {
                pending: 0,
                loaded: 1,
                failed: 1,
                cancelled: 1
            }
        );
    }
    for _ in 0..10 {
        assert!(tracker.frame_drawn(&engine, terminal_receipt).is_empty());
        assert!(tracker.synchronize(&mut engine).unwrap().is_empty());
        assert!(tracker.layout_ready(&engine).is_none());
    }
    tracker.dispose(&mut engine).unwrap();
}
#[test]
fn replaced_document_same_ids_rejects_old_tickets_and_paint_receipts() {
    let mut engine = SceneEngine::new(document()).unwrap();
    let mut tracker = SceneResources::new(&mut engine).unwrap();
    tracker.synchronize(&mut engine).unwrap();
    let old_ticket = tracker
        .ticket(&engine, engine.handle(&"loaded".into()).unwrap())
        .unwrap();
    let old_ready = tracker.layout_ready(&engine).unwrap();
    let old_frame = tracker.frame(&engine).unwrap();
    assert_eq!(tracker.frame_drawn(&engine, old_frame).len(), 1);
    engine.reset_scene(document()).unwrap();
    tracker.synchronize(&mut engine).unwrap();
    let new_ready = tracker.layout_ready(&engine).unwrap();
    assert_ne!(old_ready.document_epoch, new_ready.document_epoch);
    assert_eq!(
        tracker
            .complete(&mut engine, old_ticket, ResourceOutcome::Loaded)
            .unwrap(),
        (ResourceCompletion::Stale, None)
    );
    assert!(tracker.frame_drawn(&engine, old_frame).is_empty());
    assert_eq!(tracker.counts(&engine).pending, 3);
    let new_frame = tracker.frame(&engine).unwrap();
    let events = tracker.frame_drawn(&engine, new_frame);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].milestone, ResourceViewMilestone::ViewReady);
    assert_eq!(events[0].document_epoch, new_ready.document_epoch);
    tracker.dispose(&mut engine).unwrap();
}
