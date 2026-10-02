//! CUS-01: geometry ownership, transformed picking and live-instance isolation.
use gpui_flow_engine::*;
use gpui_flow_scene::{
    Affine, Bounds, Color, Contour, DisplayObject, HitTestOptions, Point, Scene, Shape, Size,
    Style, display,
};
use serde_json::{Value, json};

fn schema() -> PropertySchema {
    PropertySchema {
        properties: vec![PropertyDescriptor::number(
            "radius",
            20.,
            1.,
            100.,
            DirtyFlags::GEOMETRY | DirtyFlags::HIT | DirtyFlags::PAINT,
        )],
        allow_unknown: false,
    }
}
fn diamond(radius: f32) -> (Bounds, Vec<Contour>) {
    (
        Bounds::new(Point::ZERO, Size::new(radius * 2., radius * 2.)),
        vec![Contour {
            points: vec![
                Point::new(radius, 0.),
                Point::new(radius * 2., radius),
                Point::new(radius, radius * 2.),
                Point::new(0., radius),
            ],
            closed: true,
        }],
    )
}
fn engine() -> SceneEngine {
    let mut registry = ElementRegistry::default();
    registry
        .register(CustomDefinition::new(
            "acceptance.diamond",
            schema(),
            |input: &Value| {
                let radius = input["radius"].as_f64().unwrap() as f32;
                if radius == 95. {
                    panic!("deliberately failing pure geometry provider");
                }
                let (bounds, mut contours) = diamond(radius);
                if radius == 90. {
                    contours[0].points[0].x = 1000.;
                }
                Ok((bounds, contours))
            },
        ))
        .unwrap();
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("parent", display::Group::new())
                .with_transform(Affine::translation(20., 30.) * Affine::scale(2., 1.5)),
            None,
            None,
        )
        .unwrap();
    for (id, x) in [("a", 10.), ("b", 150.)] {
        scene
            .insert(
                registry
                    .create("acceptance.diamond", id, json!({}))
                    .unwrap()
                    .with_transform(Affine::translation(x, 20.))
                    .with_style(Style::fill(Color::WHITE)),
                Some("parent".into()),
                None,
            )
            .unwrap();
    }
    SceneEngine::with_registry(scene, registry).unwrap()
}
fn hit(engine: &SceneEngine, local: Point) -> Option<String> {
    let world = engine.local_to_world(&"a".into(), local).unwrap();
    engine
        .hit_test(world, HitTestOptions::default())
        .unwrap()
        .map(|hit| hit.id.to_string())
}

#[test]
fn dynamic_geometry_updates_transformed_bounds_precise_hit_and_damage_together() {
    let mut engine = engine();
    let id = "a".into();
    let handle = engine.handle(&id).unwrap();
    let old = engine.world_bounds(&id).unwrap().unwrap();
    assert_eq!(old, Bounds::new(Point::new(40., 60.), Size::new(80., 60.)));
    assert_eq!(hit(&engine, Point::new(60., 30.)), None);
    // Bounding-box membership alone is not a hit on the generated diamond.
    assert_eq!(hit(&engine, Point::new(2., 2.)), None);
    let sibling = engine.scene().get(&"b".into()).unwrap().clone();
    let change = engine
        .set_property(handle, PropertyKey::Custom("radius".into()), json!(40.))
        .unwrap();
    let new = engine.world_bounds(&id).unwrap().unwrap();
    assert_eq!(
        new,
        Bounds::new(Point::new(40., 60.), Size::new(160., 120.))
    );
    assert_eq!(hit(&engine, Point::new(60., 30.)).as_deref(), Some("a"));
    assert_eq!(hit(&engine, Point::new(2., 2.)), None);
    assert_eq!(engine.scene().get(&"b".into()).unwrap(), &sibling);
    assert!(change.dirty[&id].contains(DirtyFlags::GEOMETRY | DirtyFlags::HIT | DirtyFlags::PAINT));
    assert!(!change.dirty.contains_key(&"b".into()));
    assert_eq!(change.layout.world_recomputed, 0);
    assert!(change.damage_bounds().unwrap().contains(old.origin));
    assert!(
        change
            .damage_bounds()
            .unwrap()
            .contains(Point::new(new.max_x(), new.max_y()))
    );
    assert_eq!(
        engine
            .world_bounds(&"parent".into())
            .unwrap()
            .unwrap()
            .size
            .height,
        120.
    );
    let item = engine
        .paint_list()
        .unwrap()
        .into_iter()
        .find(|item| item.object.id == id)
        .unwrap();
    let Shape::Custom(custom) = &item.object.shape else {
        panic!("custom kind lost")
    };
    assert_eq!(custom.contours, diamond(40.).1);
    assert_eq!(item.world_transform.transform_bounds(custom.bounds), new);
    let revision = engine.revision();
    assert!(
        !engine
            .set_property(handle, PropertyKey::Custom("radius".into()), json!(40.))
            .unwrap()
            .changed()
    );
    assert_eq!(engine.revision(), revision);
}

#[test]
fn provider_failure_cannot_partially_change_geometry_or_another_instance() {
    let mut engine = engine();
    let a = engine.handle(&"a".into()).unwrap();
    let b = engine.handle(&"b".into()).unwrap();
    let before = engine.scene().clone();
    let revision = engine.revision();
    let last = engine.last_change().clone();
    let bounds = engine.world_bounds(&"parent".into()).unwrap();
    for failing_radius in [90., 95.] {
        assert!(
            engine
                .set_properties_batch(
                    ChangeOrigin::Edit,
                    [
                        (a, PropertyKey::Custom("radius".into()), json!(40.)),
                        (
                            b,
                            PropertyKey::Custom("radius".into()),
                            json!(failing_radius)
                        ),
                    ]
                )
                .is_err()
        );
        assert_eq!(engine.scene(), &before);
        assert_eq!(engine.revision(), revision);
        assert_eq!(engine.last_change(), &last);
        assert_eq!(engine.world_bounds(&"parent".into()).unwrap(), bounds);
        assert_eq!(hit(&engine, Point::new(60., 30.)), None);
    }
}

#[test]
fn same_named_custom_instances_in_distinct_engines_never_share_mutation_or_handles() {
    let mut left = engine();
    let mut right = engine();
    let left_handle = left.handle(&"a".into()).unwrap();
    let right_handle = right.handle(&"a".into()).unwrap();
    assert_ne!(left_handle, right_handle);
    let before = right.scene().clone();
    let revision = right.revision();
    left.set_property(
        left_handle,
        PropertyKey::Custom("radius".into()),
        json!(40.),
    )
    .unwrap();
    assert_eq!(right.scene(), &before);
    assert_eq!(right.revision(), revision);
    assert_eq!(hit(&left, Point::new(60., 30.)).as_deref(), Some("a"));
    assert_eq!(hit(&right, Point::new(60., 30.)), None);
    assert!(
        right
            .set_property(
                left_handle,
                PropertyKey::Custom("radius".into()),
                json!(40.)
            )
            .is_err()
    );
    assert_eq!(right.scene(), &before);
}

#[test]
fn explicit_custom_hit_provider_uses_resolved_dynamic_properties_and_local_coordinates() {
    struct HollowDiamond;
    impl ElementDefinition for HollowDiamond {
        fn kind(&self) -> &str {
            "acceptance.hollow"
        }
        fn schema(&self) -> PropertySchema {
            schema()
        }
        fn shape(&self, props: &Value) -> Result<Shape, String> {
            let (bounds, contours) = diamond(props["radius"].as_f64().unwrap() as f32);
            Ok(display::Custom::new(self.kind(), bounds, contours).into())
        }
        fn hit_test(&self, props: &Value, point: Point, _: f32) -> Option<bool> {
            let r = props["radius"].as_f64().unwrap() as f32;
            let distance = (point.x - r).abs() + (point.y - r).abs();
            Some(distance <= r && distance >= r / 2.)
        }
    }
    let mut registry = ElementRegistry::default();
    registry.register(HollowDiamond).unwrap();
    let mut scene = Scene::new();
    scene
        .insert(
            registry
                .create("acceptance.hollow", "a", json!({}))
                .unwrap()
                .with_transform(Affine::translation(100., 50.) * Affine::scale(2., 3.)),
            None,
            None,
        )
        .unwrap();
    let mut engine = SceneEngine::with_registry(scene, registry).unwrap();
    assert_eq!(hit(&engine, Point::new(20., 20.)), None);
    assert_eq!(hit(&engine, Point::new(20., 4.)).as_deref(), Some("a"));
    let handle = engine.handle(&"a".into()).unwrap();
    engine
        .set_property(handle, PropertyKey::Custom("radius".into()), json!(40.))
        .unwrap();
    assert_eq!(hit(&engine, Point::new(40., 40.)), None);
    assert_eq!(hit(&engine, Point::new(40., 8.)).as_deref(), Some("a"));
    assert_eq!(hit(&engine, Point::new(85., 40.)), None);
}
