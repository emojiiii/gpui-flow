use gpui_flow_scene::{
    Affine, Bounds, DisplayObject, EraserMode, HitPolicy, HitTestOptions, MaskMode, Point, Scene,
    Size, display,
};
use serde_json::json;

fn nested(mode: EraserMode) -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("parent", display::Group::new())
                .with_transform(Affine::translation(10., 20.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("positive", display::Rect::new(20., 20.)),
            Some("parent".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("eraser", display::Group::new())
                .with_transform(Affine::translation(100., 0.))
                .with_eraser(mode),
            Some("parent".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("cut", display::Rect::new(30., 20.))
                .with_transform(Affine::translation(5., 0.)),
            Some("eraser".into()),
            None,
        )
        .unwrap();
    scene
}

#[test]
fn eraser_defaults_and_canonical_modes_round_trip_without_shape_restrictions() {
    let legacy: DisplayObject = serde_json::from_str(
        r#"{"id":"old","shape":{"type":"rect","geometry":{"size":{"width":20,"height":20}}}}"#,
    )
    .unwrap();
    assert_eq!(legacy.eraser, EraserMode::None);
    assert_eq!(EraserMode::default(), EraserMode::None);
    assert_eq!(
        DisplayObject::new("new", display::Group::new()).eraser,
        EraserMode::None
    );
    for (mode, encoded) in [
        (EraserMode::None, "none"),
        (EraserMode::Pixel, "pixel"),
        (EraserMode::Path, "path"),
    ] {
        let scene = nested(mode);
        let object = &scene.get(&"eraser".into()).unwrap().object;
        assert_eq!(serde_json::to_value(object).unwrap()["eraser"], encoded);
        assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    }
}

#[test]
fn canonical_json_rejects_unknown_eraser_modes_and_leafer_boolean_aliases() {
    let scene = nested(EraserMode::None);
    for invalid in [json!("unknown"), json!("Pixel"), json!(true), json!(null)] {
        let mut value = serde_json::to_value(&scene).unwrap();
        value["nodes"]["eraser"]["object"]["eraser"] = invalid;
        assert!(Scene::from_json(&value.to_string()).is_err());
    }
}

#[test]
fn eraser_subtree_keeps_own_geometry_without_expanding_parent_or_visible_bounds() {
    for mode in [EraserMode::Pixel, EraserMode::Path] {
        let scene = nested(mode);
        let positive_local = Some(Bounds::new(Point::ZERO, Size::new(20., 20.)));
        let positive_world = Some(Bounds::new(Point::new(10., 20.), Size::new(20., 20.)));
        assert_eq!(
            scene.local_bounds(&"parent".into()).unwrap(),
            positive_local
        );
        assert_eq!(
            scene.world_bounds(&"parent".into()).unwrap(),
            positive_world
        );
        assert_eq!(scene.visible_bounds().unwrap(), positive_world);
        assert_eq!(
            scene.local_bounds(&"eraser".into()).unwrap(),
            Some(Bounds::new(Point::new(5., 0.), Size::new(30., 20.)))
        );
        assert_eq!(
            scene.world_bounds(&"eraser".into()).unwrap(),
            Some(Bounds::new(Point::new(115., 20.), Size::new(30., 20.)))
        );
        let hit = scene
            .hit_test(Point::new(120., 25.), HitTestOptions::default())
            .unwrap()
            .unwrap();
        assert_eq!(hit.id.as_str(), "cut");
        assert_eq!(
            hit.path,
            vec!["parent".into(), "eraser".into(), "cut".into()]
        );
        assert_eq!(scene.paint_list().unwrap().len(), 4);
    }
}

#[test]
fn hidden_and_zero_opacity_erasers_still_do_not_expand_parent_bounds() {
    for (visible, opacity) in [(false, 1.), (true, 0.)] {
        let mut scene = nested(EraserMode::Pixel);
        scene
            .update_object(&"eraser".into(), |object| {
                object.visible = visible;
                object.style.opacity = opacity;
            })
            .unwrap();
        assert_eq!(
            scene
                .local_bounds(&"parent".into())
                .unwrap()
                .unwrap()
                .max_x(),
            20.
        );
        assert_eq!(
            scene
                .local_bounds(&"eraser".into())
                .unwrap()
                .unwrap()
                .max_x(),
            35.
        );
        assert!(
            scene
                .hit_test(Point::new(120., 25.), Default::default())
                .unwrap()
                .is_none()
        );
    }
}

#[test]
fn geometric_hits_keep_eraser_and_erased_target_and_obey_existing_gates() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("parent", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    for id in ["target", "eraser"] {
        scene
            .insert(
                DisplayObject::new(id, display::Rect::new(20., 20.)).with_eraser(
                    if id == "eraser" {
                        EraserMode::Pixel
                    } else {
                        EraserMode::None
                    },
                ),
                Some("parent".into()),
                None,
            )
            .unwrap();
    }
    let point = Point::new(5., 5.);
    let ids: Vec<_> = scene
        .hit_test_all(point, Default::default())
        .unwrap()
        .into_iter()
        .map(|hit| hit.id)
        .collect();
    assert_eq!(ids, vec!["eraser".into(), "target".into()]);
    scene
        .update_object(&"eraser".into(), |object| {
            object.hit_policy = HitPolicy::None
        })
        .unwrap();
    assert_eq!(
        scene
            .hit_test(point, Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "target"
    );
    scene
        .update_object(&"eraser".into(), |object| {
            object.hit_policy = HitPolicy::Painted;
            object.locked = true;
        })
        .unwrap();
    assert_eq!(
        scene
            .hit_test(point, Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "target"
    );
    assert_eq!(
        scene
            .hit_test(
                point,
                HitTestOptions {
                    include_locked: true,
                    ..Default::default()
                }
            )
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "eraser"
    );
    scene
        .update_object(&"parent".into(), |object| object.hit_children = false)
        .unwrap();
    assert!(
        scene
            .hit_test(
                point,
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
fn active_masks_limit_layout_and_eraser_root_has_no_visible_contribution() {
    let mut scene = nested(EraserMode::Pixel);
    scene
        .insert(
            DisplayObject::new("mask", display::Rect::new(5., 5.))
                .with_transform(Affine::translation(-40., 0.))
                .with_mask(MaskMode::Alpha),
            Some("parent".into()),
            Some(0),
        )
        .unwrap();
    assert_eq!(
        scene.local_bounds(&"parent".into()).unwrap(),
        Some(Bounds::new(Point::new(-40., 0.), Size::new(5., 5.)))
    );
    assert_eq!(
        scene
            .hit_test(Point::new(15., 25.), Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "positive"
    );
    scene
        .update_object(&"parent".into(), |object| object.eraser = EraserMode::Pixel)
        .unwrap();
    assert!(scene.visible_bounds().unwrap().is_none());
    assert!(scene.world_bounds(&"parent".into()).unwrap().is_some());
    assert_eq!(
        scene
            .hit_test(Point::new(120., 25.), Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "cut"
    );
}

#[test]
fn strict_leafer_exchange_rejects_erasers_including_dormant_markers_atomically() {
    for mode in [EraserMode::Pixel, EraserMode::Path] {
        for visible in [true, false] {
            let mut scene = nested(mode);
            scene
                .update_object(&"eraser".into(), |object| object.visible = visible)
                .unwrap();
            let before = scene.clone();
            let error = scene.to_leafer_json().unwrap_err();
            assert_eq!(error.path, "eraser");
            assert_eq!(error.message, "erasers are unsupported");
            assert_eq!(scene, before);
        }
    }
    let mut scene = nested(EraserMode::None);
    let before = scene.clone();
    for mode in [json!(false), json!(true), json!("pixel"), json!("path")] {
        let input = json!({"tag":"Rect","id":"replacement","eraser":mode});
        let error = scene
            .replace_from_leafer_json(&input.to_string())
            .unwrap_err();
        assert!(error.path.contains("eraser"));
        assert_eq!(scene, before);
    }
}
