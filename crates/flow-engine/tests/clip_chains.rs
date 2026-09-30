use gpui_flow_engine::SceneEngine;
use gpui_flow_scene::{
    Affine, DisplayObject, HitPolicy, HitTestOptions, Point, Scene, Shape, Size, display,
};
fn fixture(image: bool, text_x: f32, inner_x: Option<f32>) -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("frame", display::Frame::new(100., 100.)),
        None,
        None,
    )
    .unwrap();
    let parent = if let Some(x) = inner_x {
        s.insert(
            DisplayObject::new("inner", display::Frame::new(100., 100.))
                .with_transform(Affine::translation(x, 0.)),
            Some("frame".into()),
            None,
        )
        .unwrap();
        "inner"
    } else {
        "frame"
    };
    let shape: Shape = if image {
        display::Image::new("local.png", Size::new(100., 100.)).into()
    } else {
        display::Text::new("text", Size::new(100., 100.)).into()
    };
    let mut leaf =
        DisplayObject::new("leaf", shape).with_transform(Affine::translation(text_x, 0.));
    leaf.hit_policy = HitPolicy::Bounds;
    s.insert(leaf, Some(parent.into()), None).unwrap();
    s
}
fn check(s: Scene, p: Point, expected: bool) {
    let e = SceneEngine::new(s.clone()).unwrap();
    for options in [
        HitTestOptions::default(),
        HitTestOptions {
            tolerance: 10.,
            ..Default::default()
        },
    ] {
        for hits in [
            s.hit_test_all(p, options).unwrap(),
            e.hit_test_all(p, options).unwrap(),
        ] {
            assert_eq!(hits.iter().any(|h| h.id.as_str() == "leaf"), expected);
        }
    }
}
#[test]
fn edge_only_and_disjoint_clip_chains_never_pick_text_or_image() {
    for image in [false, true] {
        for inner in [100., 110.] {
            check(
                fixture(image, -50., Some(inner)),
                Point::new(100., 50.),
                false,
            );
        }
    }
}
#[test]
fn own_box_only_touching_parent_clip_has_no_visible_area_or_hit() {
    for image in [false, true] {
        check(fixture(image, 100., None), Point::new(100., 50.), false);
    }
}
#[test]
fn nonempty_intersection_preserves_inclusive_bounds_edge_convention() {
    for image in [false, true] {
        for p in [
            Point::new(50., 0.),
            Point::new(100., 100.),
            Point::new(75., 50.),
        ] {
            check(fixture(image, 50., None), p, true);
        }
    }
}
#[test]
fn parent_clip_edits_update_cached_picking_and_restoring_clip_restores_it() {
    let mut e = SceneEngine::new(fixture(false, 100., None)).unwrap();
    let p = Point::new(110., 50.);
    assert!(
        !e.hit_test_all(p, HitTestOptions::default())
            .unwrap()
            .iter()
            .any(|h| h.id.as_str() == "leaf")
    );
    e.transaction(|s| {
        s.update_object(&"frame".into(), |o| {
            let Shape::Frame(f) = &mut o.shape else {
                panic!()
            };
            f.clip_children = false;
        })
    })
    .unwrap();
    assert!(
        e.hit_test_all(p, HitTestOptions::default())
            .unwrap()
            .iter()
            .any(|h| h.id.as_str() == "leaf")
    );
    e.transaction(|s| {
        s.update_object(&"frame".into(), |o| {
            let Shape::Frame(f) = &mut o.shape else {
                panic!()
            };
            f.clip_children = true;
        })
    })
    .unwrap();
    assert!(
        !e.hit_test_all(p, HitTestOptions::default())
            .unwrap()
            .iter()
            .any(|h| h.id.as_str() == "leaf")
    );
}
