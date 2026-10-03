use gpui_flow_engine::SceneEngine;
use gpui_flow_scene::{display, *};
fn fixture() -> Scene {
    let mut s = Scene::new();
    s.insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    s.insert(
        DisplayObject::new("other", display::Group::new()).with_transform(Affine::scale(4., 2.)),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("r", display::Rect::new(40., 30.)).with_style(Style {
            fill: None,
            stroke: Some(
                Stroke::new(Color::BLACK, 8.)
                    .with_scale_fixed(StrokeScaleFixed::Fixed)
                    .with_alignment(StrokeAlignment::Outside)
                    .with_join(StrokeJoin::Miter),
            ),
            ..Default::default()
        }),
        Some("g".into()),
        None,
    )
    .unwrap();
    s
}
fn parity(e: &SceneEngine) {
    for id in ["g", "other", "r"] {
        assert_eq!(
            e.local_bounds(&id.into()).unwrap(),
            e.scene().local_bounds(&id.into()).unwrap()
        );
        assert_eq!(
            e.world_bounds(&id.into()).unwrap(),
            e.scene().world_bounds(&id.into()).unwrap()
        );
    }
    assert_eq!(
        e.visible_bounds().unwrap(),
        e.scene().visible_bounds().unwrap()
    );
    for zoom in [0.5, 1., 2.] {
        assert_eq!(
            e.visible_bounds_for_view(zoom).unwrap(),
            e.scene().visible_bounds_for_view(zoom).unwrap()
        );
        for point in [
            Point::new(-3., 10.),
            Point::new(-6., 10.),
            Point::new(1., 1.),
        ] {
            assert_eq!(
                e.hit_test_for_view(point, HitTestOptions::default(), zoom)
                    .unwrap(),
                e.scene()
                    .hit_test_for_view(point, HitTestOptions::default(), zoom)
                    .unwrap()
            );
        }
    }
}
#[test]
fn fixed_descendant_local_cache_changes_with_ancestor_scale_and_reparent() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    parity(&e);
    let before = e.local_bounds(&"r".into()).unwrap();
    let change = e
        .transaction(|s| s.update_object(&"g".into(), |o| o.transform = Affine::scale(2., 3.)))
        .unwrap();
    assert!(change.layout.bounds_recomputed >= 2);
    assert_ne!(before, e.local_bounds(&"r".into()).unwrap());
    parity(&e);
    e.transaction(|s| s.reparent(&"r".into(), Some("other".into()), None, false))
        .unwrap();
    parity(&e);
    let before = e.scene().clone();
    let layout = e.layout().clone();
    let revision = e.revision();
    assert!(
        e.transaction(|s| s.update_object(&"other".into(), |o| o.transform =
            Affine::new(1., 0., 0.2, 1., 0., 0.)))
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.layout(), &layout);
    assert_eq!(e.revision(), revision);
}
#[test]
fn active_view_preflight_rejects_candidate_without_committing_or_changing_reference_queries() {
    let mut e = SceneEngine::new(fixture()).unwrap();
    e.set_stroke_view_scale(2.);
    let before = e.scene().clone();
    let layout = e.layout().clone();
    assert!(
        e.transaction(|s| s.update_object(&"r".into(), |o| o
            .style
            .stroke
            .as_mut()
            .unwrap()
            .width = MIN_ALIGNED_STROKE_WIDTH))
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.layout(), &layout);
    assert_eq!(e.revision(), 0);
    e.set_stroke_view_scale(0.5);
    e.transaction(|s| {
        s.update_object(&"r".into(), |o| {
            o.style.stroke.as_mut().unwrap().width = MIN_ALIGNED_STROKE_WIDTH
        })
    })
    .unwrap();
    assert_eq!(
        e.local_bounds(&"r".into()).unwrap(),
        e.scene().local_bounds(&"r".into()).unwrap()
    );
}

#[test]
fn context_sensitive_subtree_clone_move_detach_and_reinsert_validate_final_ancestry() {
    let mut scene = Scene::new();
    for (id, scale) in [("small", 0.5), ("small2", 0.5), ("large", 2.)] {
        scene
            .insert(
                DisplayObject::new(id, display::Group::new())
                    .with_transform(Affine::scale(scale, scale)),
                None,
                None,
            )
            .unwrap();
    }
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(40., 30.)).with_style(Style {
                fill: None,
                stroke: Some(
                    Stroke::new(Color::BLACK, MIN_ALIGNED_STROKE_WIDTH / 2.)
                        .with_scale_fixed(StrokeScaleFixed::Fixed)
                        .with_alignment(StrokeAlignment::Outside)
                        .with_join(StrokeJoin::Miter),
                ),
                ..Default::default()
            }),
            Some("small".into()),
            None,
        )
        .unwrap();
    let mut e = SceneEngine::new(scene).unwrap();
    let root = e.handle(&"r".into()).unwrap();
    let parent = e.handle(&"small".into()).unwrap();
    let other = e.handle(&"small2".into()).unwrap();
    let invalid = e.handle(&"large".into()).unwrap();
    let cloned = e.clone_subtree(root, Some(parent), None).unwrap();
    assert!(e.scene().get(&cloned.ids[&"r".into()]).is_some());
    e.reparent(root, Some(other), None, false).unwrap();
    let (detached, _) = e.detach(root).unwrap();
    assert!(e.detached_scene(&detached).unwrap().validate().is_ok());
    assert!(e.detached_scene(&detached).unwrap().to_json().is_err());
    let transfer = e.detached_scene(&detached).unwrap();
    let bounds = transfer.local_bounds(&"r".into()).unwrap();
    assert!(bounds.is_some());
    assert!(transfer.paint_list().is_err());
    let transferred_engine = SceneEngine::new(transfer.clone()).unwrap();
    assert_eq!(
        transferred_engine.local_bounds(&"r".into()).unwrap(),
        bounds
    );
    assert!(transferred_engine.paint_list().is_err());

    let before = e.scene().clone();
    let revision = e.revision();
    assert!(e.attach(&detached, Some(invalid), None).is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
    e.attach(&detached, Some(parent), None).unwrap();
    assert_eq!(e.handle(&"r".into()), Some(root));
    assert_eq!(
        e.scene()
            .get(&"r".into())
            .unwrap()
            .object
            .style
            .stroke
            .as_ref()
            .unwrap()
            .width,
        MIN_ALIGNED_STROKE_WIDTH / 2.
    );
    assert!(e.clone_subtree(root, Some(invalid), None).is_err());
}
