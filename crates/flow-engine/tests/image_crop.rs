use gpui_flow_engine::*;
use gpui_flow_scene::{DisplayObject, Scene, Size, display::Image};
use serde_json::json;
#[test]
fn crop_property_schema_dirty_identity_and_atomic_rejection() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("i", Image::new("test", Size::new(80., 60.))),
            None,
            None,
        )
        .unwrap();
    let mut e = SceneEngine::new(scene).unwrap();
    let h = e.handle(&"i".into()).unwrap();
    let d = e
        .property_descriptor(h, &PropertyKey::ImageSourceCrop)
        .unwrap();
    assert_eq!(d.name, "image.source_crop");
    assert_eq!(d.default, Some(json!(null)));
    let change = e
        .set_property(
            h,
            PropertyKey::ImageSourceCrop,
            json!({"origin":{"x":0.25,"y":0.},"size":{"width":0.5,"height":1.}}),
        )
        .unwrap();
    assert_eq!(change.properties.len(), 1);
    assert_eq!(change.properties[0].property, "image.source_crop");
    assert_eq!(change.properties[0].old, json!(null));
    assert!(
        change.dirty[&"i".into()]
            .contains(DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT)
    );
    let before = e.scene().clone();
    let revision = e.revision();
    for value in [
        json!({}),
        json!(-1),
        json!({"origin":{"x":0.75,"y":0.},"size":{"width":0.5,"height":1.}}),
    ] {
        assert!(
            e.set_property(h, PropertyKey::ImageSourceCrop, value)
                .is_err()
        );
        assert_eq!(e.scene(), &before);
        assert_eq!(e.revision(), revision);
    }
    e.set_property(h, PropertyKey::ImageSourceCrop, json!(null))
        .unwrap();
    assert_eq!(e.handle(&"i".into()), Some(h));
}
#[test]
fn crop_edits_retain_loaded_resource_ticket_and_source_generation() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("i", Image::new("test", Size::new(80., 60.))),
            None,
            None,
        )
        .unwrap();
    let mut e = SceneEngine::new(scene).unwrap();
    let mut resources = SceneResources::new(&mut e).unwrap();
    resources.synchronize(&mut e).unwrap();
    let h = e.handle(&"i".into()).unwrap();
    let ticket = resources.ticket(&e, h).unwrap();
    resources
        .complete(&mut e, ticket, ResourceOutcome::Loaded)
        .unwrap();
    e.set_property(
        h,
        PropertyKey::ImageSourceCrop,
        json!({"origin":{"x":0.25,"y":0.},"size":{"width":0.5,"height":1.}}),
    )
    .unwrap();
    assert!(resources.synchronize(&mut e).unwrap().is_empty());
    assert_eq!(resources.ticket(&e, h), Some(ticket));
    assert_eq!(resources.counts(&e).loaded, 1);
}

#[test]
fn decoded_image_candidates_are_explicit_rounded_ordered_and_keep_all_engine_guards() {
    use gpui_flow_scene::{Affine, HitPolicy, HitTestOptions, Point, Shape, display};
    use std::collections::BTreeSet;
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("back", display::Rect::new(100., 100.)),
            None,
            None,
        )
        .unwrap();
    let mut parent = DisplayObject::new("parent", display::Frame::new(80., 100.).rounded(25.));
    parent.hit_policy = HitPolicy::None;
    scene.insert(parent, None, None).unwrap();
    let mut image = Image::new("test", Size::new(100., 100.)).rounded(20.);
    image.natural_size = Some(Size::new(1., 100.));
    scene
        .insert(DisplayObject::new("i", image), Some("parent".into()), None)
        .unwrap();
    let explicit = BTreeSet::from(["i".into(), "back".into()]);
    let point = Point::new(10., 50.);
    let options = HitTestOptions::default();
    let engine = SceneEngine::new(scene.clone()).unwrap();
    assert_eq!(
        engine
            .hit_test_all(point, options)
            .unwrap()
            .iter()
            .map(|h| h.id.as_str())
            .collect::<Vec<_>>(),
        vec!["back"]
    );
    let ids = |engine: &SceneEngine, point, options| {
        engine
            .hit_test_all_with_image_candidates(point, options, &explicit)
            .unwrap()
            .into_iter()
            .map(|h| h.id)
            .collect::<Vec<_>>()
    };
    assert_eq!(
        ids(&engine, point, options),
        vec!["i".into(), "back".into()]
    );
    assert_eq!(
        engine
            .hit_test_all_with_image_candidates(point, options, &BTreeSet::new())
            .unwrap(),
        engine.hit_test_all(point, options).unwrap()
    );
    for point in [
        Point::new(1., 1.),
        Point::new(90., 50.),
        Point::new(-1., 50.),
    ] {
        assert!(!ids(&engine, point, options).contains(&"i".into()));
    }
    for action in 0..8 {
        let mut scene = scene.clone();
        scene
            .update_object(
                &if action < 4 { "parent" } else { "i" }.into(),
                |o| match action {
                    0 => o.visible = false,
                    1 => o.locked = true,
                    2 => o.style.opacity = 0.,
                    3 => o.hit_children = false,
                    4 => o.hit_policy = HitPolicy::None,
                    5 => o.transform = Affine::scale(0., 1.),
                    6 => o.transform = Affine::translation(200., 0.),
                    _ => o.shape = Shape::from(display::Rect::new(1., 100.)),
                },
            )
            .unwrap();
        let engine = SceneEngine::new(scene).unwrap();
        assert!(
            !ids(&engine, point, options).contains(&"i".into()),
            "guard {action}"
        );
        if action < 3 {
            assert!(
                ids(
                    &engine,
                    point,
                    HitTestOptions {
                        include_locked: true,
                        include_invisible: true,
                        ..options
                    }
                )
                .contains(&"i".into())
            );
        }
    }
    for (point, tolerance) in [
        (Point::new(f32::NAN, 0.), 0.),
        (Point::ZERO, -1.),
        (Point::ZERO, f32::NAN),
    ] {
        assert!(
            engine
                .hit_test_all_with_image_candidates(
                    point,
                    HitTestOptions {
                        tolerance,
                        ..options
                    },
                    &explicit
                )
                .is_err()
        );
    }
}
