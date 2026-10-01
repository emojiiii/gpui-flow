use gpui_flow_scene::{display, *};
use serde_json::{Value, json};

fn order(shape: &Shape) -> ContainerPaintOrder {
    match shape {
        Shape::Box(shape) => shape.paint_order,
        Shape::Frame(shape) => shape.paint_order,
        _ => panic!("expected a painted container"),
    }
}

fn set_order(shape: &mut Shape, order: ContainerPaintOrder) {
    match shape {
        Shape::Box(shape) => shape.paint_order = order,
        Shape::Frame(shape) => shape.paint_order = order,
        _ => panic!("expected a painted container"),
    }
}

#[test]
fn constructors_and_legacy_geometry_keep_before_children_order() {
    assert_eq!(
        ContainerPaintOrder::default(),
        ContainerPaintOrder::BeforeChildren
    );
    let box_shape = display::Box::new(30.0, 20.0);
    let frame = display::Frame::new(30.0, 20.0);
    assert_eq!(box_shape.paint_order, ContainerPaintOrder::BeforeChildren);
    assert_eq!(frame.paint_order, ContainerPaintOrder::BeforeChildren);
    assert!(!box_shape.clip_children);
    assert!(frame.clip_children);

    let old_geometry = json!({"size": {"width": 30.0, "height": 20.0}});
    assert_eq!(
        serde_json::from_value::<display::Box>(old_geometry.clone()).unwrap(),
        box_shape
    );
    assert_eq!(
        serde_json::from_value::<display::Frame>(old_geometry).unwrap(),
        frame
    );
}

#[test]
fn omitted_order_in_a_versioned_scene_preserves_the_legacy_document() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("box", display::Box::new(40.0, 30.0).clipped(true)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("frame", display::Frame::new(20.0, 10.0).rounded(3.0)),
            Some("box".into()),
            None,
        )
        .unwrap();
    let mut old = serde_json::to_value(&scene).unwrap();
    fn omit_order(value: &mut Value) {
        match value {
            Value::Object(map) => {
                map.remove("paint_order");
                for value in map.values_mut() {
                    omit_order(value);
                }
            }
            Value::Array(values) => {
                for value in values {
                    omit_order(value);
                }
            }
            _ => {}
        }
    }
    omit_order(&mut old);
    let restored = Scene::from_json(&serde_json::to_string(&old).unwrap()).unwrap();
    assert_eq!(restored, scene);
    assert_eq!(
        restored.paint_order().unwrap(),
        vec!["box".into(), "frame".into()]
    );
}

#[test]
fn explicit_orders_roundtrip_with_stable_json_names() {
    for (mode, name) in [
        (ContainerPaintOrder::BeforeChildren, "before_children"),
        (
            ContainerPaintOrder::StrokeAfterChildren,
            "stroke_after_children",
        ),
    ] {
        assert_eq!(serde_json::to_value(mode).unwrap(), json!(name));
        assert_eq!(
            serde_json::from_value::<ContainerPaintOrder>(json!(name)).unwrap(),
            mode
        );
        let mut scene = Scene::new();
        for (id, shape) in [
            (
                "box",
                Shape::Box(display::Box::new(40.0, 30.0).with_paint_order(mode)),
            ),
            (
                "frame",
                Shape::Frame(display::Frame::new(20.0, 10.0).with_paint_order(mode)),
            ),
        ] {
            assert_eq!(order(&shape), mode);
            assert_eq!(
                serde_json::to_value(&shape).unwrap()["geometry"]["paint_order"],
                name
            );
            scene
                .insert(DisplayObject::new(id, shape), None, None)
                .unwrap();
        }
        assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
    }
}

#[test]
fn invalid_order_is_rejected_instead_of_falling_back_to_a_default() {
    for kind in ["box", "frame"] {
        for invalid in [
            json!("after_children"),
            json!("StrokeAfterChildren"),
            Value::Null,
            json!(1),
        ] {
            let shape = json!({
                "type": kind,
                "geometry": {
                    "size": {"width": 30.0, "height": 20.0},
                    "paint_order": invalid
                }
            });
            assert!(serde_json::from_value::<Shape>(shape).is_err());
        }
    }
}

#[test]
fn phase_schedule_is_explicit_and_does_not_change_other_shapes() {
    for shape in [
        Shape::Rect(display::Rect::new(20.0, 20.0)),
        Shape::Group(display::Group::new()),
        Shape::Image(display::Image::new("local.png", Size::new(20.0, 20.0))),
        Shape::Text(display::Text::new("text", Size::new(20.0, 20.0))),
        Shape::Box(display::Box::new(20.0, 20.0)),
        Shape::Frame(display::Frame::new(20.0, 20.0)),
    ] {
        assert_eq!(shape.paint_phases(), (PaintPhase::All, None));
    }
    for shape in [
        Shape::Box(
            display::Box::new(20.0, 20.0)
                .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
        ),
        Shape::Frame(
            display::Frame::new(20.0, 20.0)
                .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
        ),
    ] {
        assert_eq!(
            shape.paint_phases(),
            (PaintPhase::Fill, Some(PaintPhase::Stroke))
        );
    }
}

#[test]
fn stroke_after_children_preserves_structural_picking_clips_and_sibling_order() {
    for shape in [
        Shape::Box(display::Box::new(20.0, 20.0).rounded(4.0).clipped(true)),
        Shape::Frame(display::Frame::new(20.0, 20.0).rounded(4.0)),
    ] {
        let mut scene = Scene::new();
        let mut outer = DisplayObject::new("outer", display::Frame::new(40.0, 40.0))
            .with_transform(Affine::translation(40.0, 20.0) * Affine::scale(2.0, 1.0));
        outer.hit_policy = HitPolicy::None;
        scene.insert(outer, None, None).unwrap();
        let mut parent = DisplayObject::new("parent", shape)
            .with_transform(Affine::translation(5.0, 5.0))
            .with_style(Style {
                fill: Some(Color::WHITE.into()),
                stroke: Some(Stroke::new(Color::BLACK, 8.0)),
                ..Style::default()
            });
        parent.z_index = 100;
        scene.insert(parent, Some("outer".into()), None).unwrap();
        for id in ["first", "last"] {
            scene
                .insert(
                    DisplayObject::new(id, display::Rect::new(20.0, 20.0)),
                    Some("parent".into()),
                    None,
                )
                .unwrap();
        }
        let ids = scene.paint_order().unwrap();
        assert_eq!(
            ids,
            vec![
                "outer".into(),
                "parent".into(),
                "first".into(),
                "last".into()
            ]
        );
        let structural = |scene: &Scene| {
            scene
                .paint_list()
                .unwrap()
                .into_iter()
                .map(|item| {
                    (
                        item.object.id.clone(),
                        item.world_transform,
                        item.opacity,
                        item.clips,
                        item.hit_enabled,
                    )
                })
                .collect::<Vec<_>>()
        };
        let before = structural(&scene);
        let bounds = scene.visible_bounds().unwrap();
        let overlap = scene
            .local_to_world(&"parent".into(), Point::new(1.0, 10.0))
            .unwrap();
        let hits = scene
            .hit_test_all(overlap, HitTestOptions::default())
            .unwrap();
        assert_eq!(
            hits.iter().map(|hit| hit.id.as_str()).collect::<Vec<_>>(),
            vec!["last", "first", "parent"]
        );
        scene
            .update_object(&"parent".into(), |object| {
                set_order(&mut object.shape, ContainerPaintOrder::StrokeAfterChildren);
            })
            .unwrap();
        assert_eq!(scene.paint_order().unwrap(), ids);
        assert_eq!(structural(&scene), before);
        assert_eq!(scene.visible_bounds().unwrap(), bounds);
        // Picking intentionally prefers the child beneath the visually top stroke.
        assert_eq!(
            scene
                .hit_test_all(overlap, HitTestOptions::default())
                .unwrap(),
            hits
        );
        let items = scene.paint_list().unwrap();
        assert_eq!(items.len(), 4);
        assert_eq!(items[1].clips.len(), 1);
        assert_eq!(items[2].clips.len(), 2);
        let outside_child_clip = scene
            .local_to_world(&"parent".into(), Point::new(-2.0, 10.0))
            .unwrap();
        assert_eq!(
            scene
                .hit_test(outside_child_clip, HitTestOptions::default())
                .unwrap()
                .unwrap()
                .id
                .as_str(),
            "parent"
        );
        scene
            .update_object(&"parent".into(), |object| object.hit_children = false)
            .unwrap();
        assert_eq!(
            scene
                .hit_test(overlap, HitTestOptions::default())
                .unwrap()
                .unwrap()
                .id
                .as_str(),
            "parent"
        );
    }
}
