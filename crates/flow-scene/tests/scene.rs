use gpui_flow_scene::{display as d, *};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};
fn id(value: &str) -> DisplayId {
    value.into()
}
fn rect(name: &str, w: f32, h: f32) -> DisplayObject {
    DisplayObject::new(name, d::Rect::new(w, h))
}
fn group(name: &str) -> DisplayObject {
    DisplayObject::new(name, d::Group::new())
}
fn add(scene: &mut Scene, object: DisplayObject, parent: Option<&str>) {
    scene.insert(object, parent.map(id), None).unwrap();
}
fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 0.002, "{a} != {b}");
}
fn close_point(a: Point, b: Point) {
    close(a.x, b.x);
    close(a.y, b.y);
}
fn nested() -> Scene {
    let mut scene = Scene::new();
    add(
        &mut scene,
        group("root").with_transform(Affine::translation(100.0, 50.0)),
        None,
    );
    add(
        &mut scene,
        group("middle").with_transform(Affine::rotation(90.0) * Affine::scale(2.0, 1.0)),
        Some("root"),
    );
    add(
        &mut scene,
        rect("leaf", 40.0, 20.0).with_transform(Affine::translation(10.0, 5.0)),
        Some("middle"),
    );
    scene
}
#[test]
fn affine_composition_inverse_and_nested_coordinates() {
    let transform = Affine::translation(40.0, 30.0)
        * Affine::rotation(37.0)
        * Affine::scale(2.0, 0.5)
        * Affine::skew(12.0, -4.0);
    let point = Point::new(12.0, -20.0);
    close_point(
        transform
            .inverse()
            .unwrap()
            .transform_point(transform.transform_point(point)),
        point,
    );
    let scene = nested();
    let world = scene
        .local_to_world(&id("leaf"), Point::new(4.0, 3.0))
        .unwrap();
    close_point(world, Point::new(92.0, 78.0));
    close_point(
        scene.world_to_local(&id("leaf"), world).unwrap(),
        Point::new(4.0, 3.0),
    );
    assert!(Affine::scale(0.0, 1.0).inverse().is_none());
}
#[test]
fn shape_specific_picking_uses_inverse_world_transform() {
    let scene = nested();
    let p = scene
        .local_to_world(&id("leaf"), Point::new(20.0, 10.0))
        .unwrap();
    let hit = scene
        .hit_test(p, HitTestOptions::default())
        .unwrap()
        .unwrap();
    assert_eq!(hit.id, id("leaf"));
    assert_eq!(hit.path, vec![id("root"), id("middle"), id("leaf")]);
    close_point(hit.local_point, Point::new(20.0, 10.0));
    let outside = scene
        .local_to_world(&id("leaf"), Point::new(41.0, 10.0))
        .unwrap();
    assert!(
        scene
            .hit_test(outside, HitTestOptions::default())
            .unwrap()
            .is_none()
    );
}
#[test]
fn sibling_z_order_is_stable_and_scoped() {
    let mut scene = Scene::new();
    add(&mut scene, rect("a", 100.0, 100.0), None);
    add(&mut scene, rect("b", 100.0, 100.0), None);
    assert_eq!(
        scene
            .hit_test(Point::new(10.0, 10.0), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id,
        id("b")
    );
    scene.update_object(&id("a"), |o| o.z_index = 2).unwrap();
    assert_eq!(scene.paint_order().unwrap(), vec![id("b"), id("a")]);
    let mut parent = group("container");
    parent.z_index = -1;
    add(&mut scene, parent, None);
    let mut child = rect("child", 100.0, 100.0);
    child.z_index = 100;
    add(&mut scene, child, Some("container"));
    assert_eq!(
        scene.paint_order().unwrap(),
        vec![id("container"), id("child"), id("b"), id("a")]
    );
}
#[test]
fn insertion_reorder_reparent_and_remove_preserve_tree() {
    let mut scene = nested();
    add(
        &mut scene,
        group("other").with_transform(Affine::translation(-30.0, 100.0) * Affine::rotation(-30.0)),
        None,
    );
    let before = scene.world_transform(&id("leaf")).unwrap();
    scene
        .reparent(&id("leaf"), Some(id("other")), Some(0), true)
        .unwrap();
    for point in [Point::ZERO, Point::new(5.0, 7.0)] {
        close_point(
            scene
                .world_transform(&id("leaf"))
                .unwrap()
                .transform_point(point),
            before.transform_point(point),
        );
    }
    assert!(scene.get(&id("middle")).unwrap().children.is_empty());
    assert_eq!(scene.get(&id("leaf")).unwrap().parent, Some(id("other")));
    assert!(matches!(
        scene.reparent(&id("other"), Some(id("leaf")), None, false),
        Err(SceneError::Cycle)
    ));
    let removed = scene.remove(&id("other")).unwrap();
    assert_eq!(
        removed.iter().map(|o| o.id.clone()).collect::<Vec<_>>(),
        vec![id("other"), id("leaf")]
    );
    scene.validate().unwrap();
}
#[test]
fn mutations_fail_atomically_and_panics_cannot_escape_candidate() {
    let mut scene = nested();
    let before = scene.clone();
    assert!(
        scene
            .insert(rect("illegal", 10.0, 10.0), Some(id("leaf")), None)
            .is_err()
    );
    assert_eq!(scene, before);
    assert!(
        scene
            .update_object(&id("root"), |o| o.shape =
                d::Ellipse::new(100.0, 100.0).into())
            .is_err()
    );
    assert_eq!(scene, before);
    assert!(
        scene
            .update_object(&id("leaf"), |o| o.id = id("new"))
            .is_err()
    );
    assert_eq!(scene, before);
    assert!(scene.reparent(&id("leaf"), None, Some(100), false).is_err());
    assert_eq!(scene, before);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = scene.update_object(&id("leaf"), |o| {
            o.visible = false;
            panic!("simulated callback panic");
        });
    }));
    assert!(panic.is_err());
    assert_eq!(scene, before);
}
#[test]
fn singular_parent_reparent_rejected_but_singular_scene_is_valid() {
    let mut scene = nested();
    add(
        &mut scene,
        group("collapsed").with_transform(Affine::scale(0.0, 1.0)),
        None,
    );
    let before = scene.clone();
    assert!(matches!(
        scene.reparent(&id("leaf"), Some(id("collapsed")), None, true),
        Err(SceneError::NonInvertibleTransform(_))
    ));
    assert_eq!(scene, before);
    scene
        .reparent(&id("leaf"), Some(id("collapsed")), None, false)
        .unwrap();
    scene.validate().unwrap();
    assert!(
        scene
            .hit_test(Point::new(0.0, 10.0), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
}
#[test]
fn transformed_frame_clips_children_and_provides_adapter_clip_stack() {
    let mut scene = Scene::new();
    let transform = Affine::translation(100.0, 100.0) * Affine::rotation(30.0);
    add(
        &mut scene,
        DisplayObject::new("frame", d::Frame::new(100.0, 100.0).rounded(20.0))
            .with_transform(transform),
        None,
    );
    add(
        &mut scene,
        rect("big", 200.0, 200.0).with_transform(Affine::translation(-50.0, -50.0)),
        Some("frame"),
    );
    let hit = scene
        .hit_test(
            transform.transform_point(Point::new(50.0, 50.0)),
            HitTestOptions::default(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(hit.id, id("big"));
    assert!(
        scene
            .hit_test(
                transform.transform_point(Point::new(1.0, 1.0)),
                HitTestOptions::default()
            )
            .unwrap()
            .is_none()
    );
    assert!(
        scene
            .hit_test(
                transform.transform_point(Point::new(101.0, 50.0)),
                HitTestOptions::default()
            )
            .unwrap()
            .is_none()
    );
    let items = scene.paint_list().unwrap();
    assert_eq!(items[1].clips.len(), 1);
    assert_eq!(items[1].clips[0].corner_radius, 20.0);
    assert_eq!(
        scene.local_bounds(&id("frame")).unwrap().unwrap(),
        Bounds::new(Point::ZERO, Size::new(100.0, 100.0))
    );
}
#[test]
fn nested_clips_are_intersected_for_picking() {
    let mut scene = Scene::new();
    add(
        &mut scene,
        DisplayObject::new("outer", d::Frame::new(100.0, 100.0)),
        None,
    );
    add(
        &mut scene,
        DisplayObject::new("inner", d::Frame::new(100.0, 100.0))
            .with_transform(Affine::translation(50.0, 0.0)),
        Some("outer"),
    );
    add(&mut scene, rect("child", 100.0, 100.0), Some("inner"));
    assert_eq!(
        scene
            .hit_test(Point::new(75.0, 50.0), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id,
        id("child")
    );
    assert!(
        scene
            .hit_test(Point::new(120.0, 50.0), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert_eq!(scene.paint_list().unwrap().last().unwrap().clips.len(), 2);
}
#[test]
fn visibility_opacity_locks_and_hit_children_are_inherited() {
    let mut scene = Scene::new();
    let mut root = group("root");
    root.style.opacity = 0.5;
    add(&mut scene, root, None);
    let mut leaf = rect("leaf", 100.0, 100.0);
    leaf.style.opacity = 0.4;
    add(&mut scene, leaf, Some("root"));
    close(scene.paint_list().unwrap()[1].opacity, 0.2);
    scene
        .update_object(&id("root"), |o| o.locked = true)
        .unwrap();
    assert!(
        scene
            .hit_test(Point::new(5.0, 5.0), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert!(
        scene
            .hit_test(
                Point::new(5.0, 5.0),
                HitTestOptions {
                    include_locked: true,
                    ..Default::default()
                }
            )
            .unwrap()
            .is_some()
    );
    scene
        .update_object(&id("root"), |o| {
            o.locked = false;
            o.visible = false;
        })
        .unwrap();
    assert!(scene.paint_list().unwrap().is_empty());
    assert!(
        scene
            .hit_test(
                Point::new(5.0, 5.0),
                HitTestOptions {
                    include_invisible: true,
                    ..Default::default()
                }
            )
            .unwrap()
            .is_some()
    );
    scene
        .update_object(&id("root"), |o| {
            o.visible = true;
            o.hit_children = false;
            o.hit_policy = HitPolicy::Bounds;
        })
        .unwrap();
    assert_eq!(
        scene
            .hit_test(Point::new(5.0, 5.0), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id,
        id("root")
    );
}
#[test]
fn local_and_world_bounds_include_strokes_and_transformed_descendants() {
    let mut scene = Scene::new();
    add(
        &mut scene,
        group("root").with_transform(Affine::translation(20.0, 30.0)),
        None,
    );
    add(
        &mut scene,
        rect("leaf", 100.0, 40.0)
            .with_transform(Affine::translation(10.0, 5.0))
            .with_style(Style::stroke(Color::BLACK, 10.0)),
        Some("root"),
    );
    assert_eq!(
        scene.local_bounds(&id("root")).unwrap().unwrap(),
        Bounds::new(Point::new(5.0, 0.0), Size::new(110.0, 50.0))
    );
    assert_eq!(
        scene.world_bounds(&id("root")).unwrap().unwrap(),
        Bounds::new(Point::new(25.0, 30.0), Size::new(110.0, 50.0))
    );
}
#[test]
fn rectangle_ellipse_polygon_and_star_hit_own_geometry() {
    let rounded = Shape::from(d::Rect::new(100.0, 100.0).rounded(30.0));
    assert!(!rounded.contains_geometry(Point::new(1.0, 1.0), FillRule::NonZero));
    assert!(rounded.contains_geometry(Point::new(50.0, 50.0), FillRule::NonZero));
    let ellipse = Shape::from(d::Ellipse::new(100.0, 50.0));
    assert!(!ellipse.contains_geometry(Point::new(1.0, 1.0), FillRule::NonZero));
    assert!(ellipse.contains_geometry(Point::new(50.0, 25.0), FillRule::NonZero));
    let triangle = Shape::from(d::Polygon::new(vec![
        Point::ZERO,
        Point::new(100.0, 0.0),
        Point::new(0.0, 100.0),
    ]));
    assert!(!triangle.contains_geometry(Point::new(80.0, 80.0), FillRule::NonZero));
    assert!(triangle.contains_geometry(Point::new(10.0, 10.0), FillRule::NonZero));
    let star = Shape::from(d::Star::new(5, 50.0, 20.0));
    assert_eq!(star.flatten(0.1)[0].points.len(), 10);
    assert!(star.contains_geometry(Point::new(50.0, 50.0), FillRule::NonZero));
}
#[test]
fn stroked_line_has_round_endcaps_and_no_implicit_rectangle_fill() {
    let shape = Shape::from(d::Line::new(Point::ZERO, Point::new(100.0, 0.0)));
    let style = Style::stroke(Color::BLACK, 10.0);
    assert!(shape.hit_test(Point::new(-4.0, 0.0), &style, 0.0));
    assert!(shape.hit_test(Point::new(50.0, 4.0), &style, 0.0));
    assert!(!shape.hit_test(Point::new(50.0, 6.0), &style, 0.0));
    assert!(!shape.hit_test(Point::new(50.0, 0.0), &Style::default(), 0.0));
}
#[test]
fn bezier_bounds_are_analytic_and_flattening_is_adaptive() {
    let path = d::Pen::new()
        .move_to(Point::ZERO)
        .cubic_to(
            Point::new(0.0, 100.0),
            Point::new(100.0, 100.0),
            Point::new(100.0, 0.0),
        )
        .into_path();
    let bounds = path.bounds().unwrap();
    close(bounds.max_y(), 75.0);
    close(bounds.size.width, 100.0);
    assert!(path.flatten(0.05)[0].points.len() > path.flatten(5.0)[0].points.len());
    let quadratic = d::Pen::new()
        .move_to(Point::ZERO)
        .quadratic_to(Point::new(50.0, 100.0), Point::new(100.0, 0.0));
    close(quadratic.bounds().unwrap().max_y(), 50.0);
    assert!(
        d::Path::new(vec![d::PathCommand::LineTo { to: Point::ZERO }])
            .validate()
            .is_err()
    );
}
#[test]
fn fill_rules_handle_holes_and_multiple_subpaths() {
    let path = d::Pen::new()
        .move_to(Point::ZERO)
        .line_to(Point::new(100.0, 0.0))
        .line_to(Point::new(100.0, 100.0))
        .line_to(Point::new(0.0, 100.0))
        .close()
        .move_to(Point::new(25.0, 25.0))
        .line_to(Point::new(75.0, 25.0))
        .line_to(Point::new(75.0, 75.0))
        .line_to(Point::new(25.0, 75.0))
        .close();
    let shape = Shape::from(path);
    assert!(shape.contains_geometry(Point::new(50.0, 50.0), FillRule::NonZero));
    assert!(!shape.contains_geometry(Point::new(50.0, 50.0), FillRule::EvenOdd));
    assert_eq!(shape.flatten(0.1).len(), 2);
}
#[test]
fn image_fit_uses_explicit_intrinsic_metrics_and_contains_letterbox() {
    let mut image = d::Image::new("asset://photo", Size::new(200.0, 200.0));
    image.natural_size = Some(Size::new(400.0, 200.0));
    assert_eq!(
        image.content_bounds(),
        Bounds::new(Point::new(0.0, 50.0), Size::new(200.0, 100.0))
    );
    let shape = Shape::from(image.clone());
    assert!(!shape.hit_test(Point::new(100.0, 10.0), &Style::default(), 0.0));
    assert!(shape.hit_test(Point::new(100.0, 100.0), &Style::default(), 0.0));
    image.fit = d::ImageFit::Cover;
    assert_eq!(
        image.content_bounds(),
        Bounds::new(Point::new(-100.0, 0.0), Size::new(400.0, 200.0))
    );
    image.fit = d::ImageFit::Fill;
    assert_eq!(image.content_bounds(), image.bounds());
}
#[test]
fn all_primitives_roundtrip_with_common_data() {
    let shapes: Vec<Shape> = vec![
        d::Rect::new(10.0, 20.0).into(),
        d::Ellipse::new(10.0, 20.0).into(),
        d::Line::new(Point::ZERO, Point::new(10.0, 10.0)).into(),
        d::Polygon::new(vec![
            Point::ZERO,
            Point::new(10.0, 0.0),
            Point::new(0.0, 10.0),
        ])
        .into(),
        d::Star::new(6, 20.0, 10.0).into(),
        d::Path::default().into(),
        d::Pen::new()
            .move_to(Point::ZERO)
            .line_to(Point::new(10.0, 10.0))
            .into(),
        d::Text::new("hello", Size::new(100.0, 30.0)).into(),
        d::Image::new("asset://one", Size::new(100.0, 100.0)).into(),
        d::Group::new().into(),
        d::Box::new(100.0, 100.0).into(),
        d::Frame::new(100.0, 100.0).into(),
    ];
    let mut scene = Scene::new();
    for (index, shape) in shapes.into_iter().enumerate() {
        add(
            &mut scene,
            DisplayObject::new(format!("shape-{index}"), shape)
                .with_name("example")
                .with_class("test")
                .with_data(json!({"index":index})),
            None,
        );
    }
    let restored = Scene::from_json(&scene.to_json().unwrap()).unwrap();
    assert_eq!(restored, scene);
    assert_eq!(restored.find_by_name("example").count(), 12);
    assert_eq!(restored.find_by_class("test").count(), 12);
}
#[test]
fn semantic_import_validation_rejects_bad_hierarchy_and_version() {
    let scene = nested();
    let mut value = serde_json::to_value(&scene).unwrap();
    value["version"] = json!(999);
    assert!(Scene::from_json(&value.to_string()).is_err());
    value = serde_json::to_value(&scene).unwrap();
    value["nodes"]["leaf"]["parent"] = json!("root");
    assert!(Scene::from_json(&value.to_string()).is_err());
    value = serde_json::to_value(&scene).unwrap();
    value["nodes"]["root"]["children"] = json!(["middle", "middle"]);
    assert!(Scene::from_json(&value.to_string()).is_err());
    value = serde_json::to_value(&scene).unwrap();
    value["roots"] = json!([]);
    assert!(Scene::from_json(&value.to_string()).is_err());
    value = serde_json::to_value(&scene).unwrap();
    value["nodes"]["leaf"]["object"]["id"] = json!("different");
    assert!(Scene::from_json(&value.to_string()).is_err());
}
#[test]
fn invalid_numeric_geometry_and_gradients_are_atomic() {
    let mut scene = Scene::new();
    add(&mut scene, rect("a", 100.0, 100.0), None);
    let before = scene.clone();
    assert!(
        scene
            .update_object(&id("a"), |o| o.transform.a = f32::INFINITY)
            .is_err()
    );
    assert_eq!(scene, before);
    assert!(
        scene
            .update_object(&id("a"), |o| o.style.opacity = -0.1)
            .is_err()
    );
    assert_eq!(scene, before);
    assert!(
        scene
            .update_object(&id("a"), |o| o.style.fill = Some(Paint::LinearGradient {
                start: Point::ZERO,
                end: Point::new(1.0, 0.0),
                stops: vec![
                    GradientStop {
                        offset: 0.8,
                        color: Color::BLACK
                    },
                    GradientStop {
                        offset: 0.2,
                        color: Color::WHITE
                    }
                ]
            }))
            .is_err()
    );
    assert_eq!(scene, before);
    assert!(
        scene
            .update_object(&id("a"), |o| o.transform = Affine::scale(f32::MAX, 1.0))
            .is_err()
    );
    assert_eq!(scene, before);
}
type EventLog = Rc<RefCell<Vec<(String, EventPhase, Option<Point>)>>>;

fn listen(dispatcher: &mut EventDispatcher, log: EventLog, node: &str, capture: bool) {
    let name = node.to_owned();
    dispatcher.on(
        node,
        EventKind::PointerDown,
        ListenerOptions {
            capture,
            once: false,
        },
        move |event| {
            log.borrow_mut()
                .push((name.clone(), event.phase, event.local_position));
        },
    );
}
#[test]
fn event_capture_target_bubble_order_and_local_coordinates() {
    let scene = nested();
    let log = Rc::new(RefCell::new(Vec::new()));
    let mut events = EventDispatcher::new();
    for node in ["root", "middle", "leaf"] {
        listen(&mut events, log.clone(), node, true);
        listen(&mut events, log.clone(), node, false);
    }
    let world = scene
        .local_to_world(&id("leaf"), Point::new(5.0, 6.0))
        .unwrap();
    let result = events
        .dispatch(
            &scene,
            SceneEvent::new(EventKind::PointerDown, "leaf").at(world),
        )
        .unwrap();
    assert_eq!(result.invoked, 6);
    let log = log.borrow();
    let order: Vec<_> = log
        .iter()
        .map(|(id, phase, _)| (id.as_str(), *phase))
        .collect();
    assert_eq!(
        order,
        vec![
            ("root", EventPhase::Capture),
            ("middle", EventPhase::Capture),
            ("leaf", EventPhase::Target),
            ("leaf", EventPhase::Target),
            ("middle", EventPhase::Bubble),
            ("root", EventPhase::Bubble)
        ]
    );
    close_point(log[2].2.unwrap(), Point::new(5.0, 6.0));
}
#[test]
fn once_and_off_listeners_and_default_prevention() {
    let scene = nested();
    let mut events = EventDispatcher::new();
    events.on(
        "leaf",
        EventKind::Click,
        ListenerOptions {
            once: true,
            capture: false,
        },
        |e| e.prevent_default(),
    );
    let removable = events.on("leaf", EventKind::Click, ListenerOptions::default(), |_| {
        panic!("removed listener must not run")
    });
    assert!(events.off(removable));
    assert!(!events.off(removable));
    let first = events
        .dispatch(&scene, SceneEvent::new(EventKind::Click, "leaf"))
        .unwrap();
    assert_eq!(first.invoked, 1);
    assert!(first.event.default_prevented());
    assert_eq!(
        events
            .dispatch(&scene, SceneEvent::new(EventKind::Click, "leaf"))
            .unwrap()
            .invoked,
        0
    );
}
#[test]
fn propagation_stop_keeps_same_target_listeners_but_not_ancestors() {
    let scene = nested();
    let mut events = EventDispatcher::new();
    let count = Rc::new(RefCell::new(0));
    events.on(
        "leaf",
        EventKind::Click,
        ListenerOptions {
            capture: true,
            once: false,
        },
        |e| e.stop_propagation(),
    );
    let count_copy = count.clone();
    events.on(
        "leaf",
        EventKind::Click,
        ListenerOptions::default(),
        move |_| *count_copy.borrow_mut() += 1,
    );
    events.on("root", EventKind::Click, ListenerOptions::default(), |_| {
        panic!("bubble should stop")
    });
    assert_eq!(
        events
            .dispatch(&scene, SceneEvent::new(EventKind::Click, "leaf"))
            .unwrap()
            .invoked,
        2
    );
    assert_eq!(*count.borrow(), 1);
}
#[test]
fn immediate_stop_suppresses_same_target_listeners() {
    let scene = nested();
    let mut events = EventDispatcher::new();
    events.on(
        "root",
        EventKind::Click,
        ListenerOptions {
            capture: true,
            once: false,
        },
        |e| e.stop_immediate_propagation(),
    );
    events.on(
        "root",
        EventKind::Click,
        ListenerOptions {
            capture: true,
            once: false,
        },
        |_| panic!("must not run"),
    );
    events.on("leaf", EventKind::Click, ListenerOptions::default(), |_| {
        panic!("target must not run")
    });
    assert_eq!(
        events
            .dispatch(&scene, SceneEvent::new(EventKind::Click, "leaf"))
            .unwrap()
            .invoked,
        1
    );
}
#[test]
fn nonbubbling_events_still_capture_and_cancelability_is_respected() {
    let scene = nested();
    let mut events = EventDispatcher::new();
    events.on(
        "root",
        EventKind::PointerEnter,
        ListenerOptions {
            capture: true,
            once: false,
        },
        |e| e.prevent_default(),
    );
    events.on(
        "root",
        EventKind::PointerEnter,
        ListenerOptions::default(),
        |_| panic!("pointerenter does not bubble"),
    );
    let mut event = SceneEvent::new(EventKind::PointerEnter, "leaf");
    event.cancelable = false;
    let result = events.dispatch(&scene, event).unwrap();
    assert_eq!(result.invoked, 1);
    assert!(!result.event.default_prevented());
}
#[test]
fn removing_scene_subtree_can_prune_event_listeners() {
    let mut scene = nested();
    let mut events = EventDispatcher::new();
    events.on("leaf", EventKind::Click, ListenerOptions::default(), |_| {});
    events.on("root", EventKind::Click, ListenerOptions::default(), |_| {});
    scene.remove(&id("middle")).unwrap();
    events.retain_existing(&scene);
    assert_eq!(events.len(), 1);
    assert!(
        events
            .dispatch(&scene, SceneEvent::new(EventKind::Click, "leaf"))
            .is_err()
    );
}

#[test]
fn custom_element_retains_identity_data_and_fallback_geometry() {
    let contour = Contour {
        points: vec![Point::ZERO, Point::new(100.0, 0.0), Point::new(50.0, 100.0)],
        closed: true,
    };
    let custom = d::Custom::new(
        "application/badge",
        Bounds::new(Point::ZERO, Size::new(100.0, 100.0)),
        vec![contour.clone()],
    )
    .with_data(json!({"label":"Ready"}));
    let mut scene = Scene::new();
    add(&mut scene, DisplayObject::new("badge", custom), None);
    let restored = Scene::from_json(&scene.to_json().unwrap()).unwrap();
    let Shape::Custom(custom) = &restored.get(&id("badge")).unwrap().object.shape else {
        panic!("custom type lost")
    };
    assert_eq!(custom.kind, "application/badge");
    assert_eq!(custom.data["label"], json!("Ready"));
    assert_eq!(custom.contours, vec![contour]);
    assert!(
        restored
            .hit_test(Point::new(50.0, 30.0), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    assert!(
        restored
            .hit_test(Point::new(90.0, 90.0), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    assert!(
        scene
            .update_object(&id("badge"), |o| {
                if let Shape::Custom(c) = &mut o.shape {
                    c.bounds.size.width = 10.0;
                }
            })
            .is_err()
    );
}
#[test]
fn max_depth_is_checked_without_unbounded_recursion() {
    let mut scene = Scene::new();
    let mut parent = None;
    for index in 0..MAX_SCENE_DEPTH {
        let object = group(&format!("group-{index}"));
        let id = object.id.clone();
        scene.insert(object, parent, None).unwrap();
        parent = Some(id);
    }
    let before = scene.clone();
    assert!(scene.insert(group("too-deep"), parent, None).is_err());
    assert_eq!(scene, before);
    scene.validate().unwrap();
}
#[test]
fn unreachable_cycle_import_is_rejected() {
    let mut scene = Scene::new();
    add(&mut scene, group("a"), None);
    add(&mut scene, group("b"), Some("a"));
    let mut value = serde_json::to_value(scene).unwrap();
    value["roots"] = json!([]);
    value["nodes"]["a"]["parent"] = json!("b");
    value["nodes"]["b"]["children"] = json!(["a"]);
    assert!(
        Scene::from_json(&value.to_string())
            .unwrap_err()
            .to_string()
            .contains("cyclic")
    );
}
#[test]
fn local_aggregate_overflow_cannot_hide_under_a_shrinking_parent() {
    let mut scene = Scene::new();
    add(
        &mut scene,
        group("root").with_transform(Affine::scale(0.01, 0.01)),
        None,
    );
    add(
        &mut scene,
        rect("left", 1.0, 1.0).with_transform(Affine::translation(-f32::MAX * 0.75, 0.0)),
        Some("root"),
    );
    let before = scene.clone();
    assert!(
        scene
            .insert(
                rect("right", 1.0, 1.0).with_transform(Affine::translation(f32::MAX * 0.75, 0.0)),
                Some(id("root")),
                None
            )
            .is_err()
    );
    assert_eq!(scene, before);
}
#[test]
fn affine_uses_wide_intermediates_and_rejects_nonfinite_bounds() {
    let transform = Affine::new(f32::MAX, 0.0, -f32::MAX, 1.0, 0.0, 0.0);
    assert_eq!(
        transform.transform_point(Point::new(2.0, 2.0)),
        Point::new(0.0, 2.0)
    );
    let overflow = Affine::scale(f32::MAX, 1.0)
        .transform_bounds(Bounds::new(Point::ZERO, Size::new(2.0, 2.0)));
    assert!(!overflow.is_valid());
}
#[test]
fn listener_cannot_retarget_an_inflight_event() {
    let scene = nested();
    let mut dispatcher = EventDispatcher::new();
    dispatcher.on(
        "root",
        EventKind::Click,
        ListenerOptions {
            capture: true,
            once: false,
        },
        |event| {
            event.target = id("missing");
            event.kind = EventKind::Wheel;
        },
    );
    dispatcher.on(
        "leaf",
        EventKind::Click,
        ListenerOptions::default(),
        |event| {
            assert_eq!(event.target, id("leaf"));
            assert_eq!(event.kind, EventKind::Click);
            event.target = id("changed-again");
        },
    );
    let result = dispatcher
        .dispatch(&scene, SceneEvent::new(EventKind::Click, "leaf"))
        .unwrap();
    assert_eq!(result.invoked, 2);
    assert_eq!(result.event.target, id("leaf"));
    assert_eq!(result.event.kind, EventKind::Click);
}
#[test]
fn valid_gradient_styles_roundtrip() {
    let mut scene = Scene::new();
    let stops = vec![
        GradientStop {
            offset: 0.0,
            color: Color::hex(0xff6600),
        },
        GradientStop {
            offset: 1.0,
            color: Color::rgba(0, 0, 255, 128),
        },
    ];
    let mut object = rect("linear", 100.0, 100.0);
    object.style.fill = Some(Paint::LinearGradient {
        start: Point::ZERO,
        end: Point::new(100.0, 100.0),
        stops: stops.clone(),
    });
    add(&mut scene, object, None);
    let mut object = rect("radial", 100.0, 100.0);
    object.style.fill = Some(Paint::RadialGradient {
        center: Point::new(50.0, 50.0),
        radius: 50.0,
        stops,
    });
    add(&mut scene, object, None);
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
}
