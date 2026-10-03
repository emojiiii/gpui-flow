use gpui_flow_engine::*;
use gpui_flow_scene::{
    Bounds, Contour, DisplayObject, HitTestOptions, Point, Scene, Shape, display,
};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
fn registry(counter: Arc<AtomicUsize>) -> ElementRegistry {
    let mut registry = ElementRegistry::default();
    registry
        .register(CustomDefinition::new(
            "badge",
            PropertySchema {
                properties: vec![
                    PropertyDescriptor::number(
                        "radius",
                        10.,
                        1.,
                        100.,
                        DirtyFlags::GEOMETRY | DirtyFlags::HIT | DirtyFlags::PAINT,
                    ),
                    PropertyDescriptor {
                        name: "label".into(),
                        value_type: PropertyType::String,
                        default: Some(json!("")),
                        required: false,
                        dirty: DirtyFlags::DATA,
                    },
                ],
                allow_unknown: false,
            },
            move |props: &Value| {
                counter.fetch_add(1, Ordering::Relaxed);
                let r = props["radius"].as_f64().unwrap() as f32;
                let ellipse = display::Ellipse::circle(r);
                Ok((ellipse.bounds(), ellipse.flatten(0.1)))
            },
        ))
        .unwrap();
    registry
}
#[test]
fn custom_factory_updates_geometry_bounds_and_hit_from_properties() {
    let registry = registry(Arc::new(AtomicUsize::new(0)));
    let mut e = SceneEngine::with_registry(Scene::new(), registry).unwrap();
    let (badge, _) = e.create("badge", "badge", json!({}), None, None).unwrap();
    assert_eq!(
        e.world_bounds(&"badge".into()).unwrap().unwrap().size.width,
        20.
    );
    assert!(
        e.hit_test(Point::new(30., 10.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    let change = e
        .set_property(badge, PropertyKey::Custom("radius".into()), json!(30))
        .unwrap();
    assert_eq!(
        e.world_bounds(&"badge".into()).unwrap().unwrap().size.width,
        60.
    );
    assert!(
        e.hit_test(Point::new(30., 10.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    assert!(
        change
            .properties
            .iter()
            .any(|p| p.property == "custom.radius")
    );
    assert!(change.dirty[&"badge".into()].contains(DirtyFlags::GEOMETRY));
}
#[test]
fn schema_validation_failure_is_atomic_and_defaults_are_not_serialized() {
    let registry = registry(Arc::new(AtomicUsize::new(0)));
    let mut e = SceneEngine::with_registry(Scene::new(), registry).unwrap();
    let (badge, _) = e.create("badge", "badge", json!({}), None, None).unwrap();
    let before = e.scene().clone();
    let revision = e.revision();
    for value in [json!(0), json!(200), json!("bad")] {
        assert!(
            e.set_property(badge, PropertyKey::Custom("radius".into()), value)
                .is_err()
        );
        assert_eq!(e.scene(), &before);
        assert_eq!(e.revision(), revision);
    }
    let Shape::Custom(custom) = &e.object(badge).unwrap().shape else {
        panic!()
    };
    assert!(custom.data.get("radius").is_none());
    assert!(
        e.set_property(badge, PropertyKey::Custom("unknown".into()), json!(1))
            .is_err()
    );
}
#[test]
fn unknown_custom_preserves_inert_fallback_but_factory_unknown_errors() {
    let mut s = Scene::new();
    let custom = display::Custom::new(
        "legacy-diamond",
        Bounds::from_points(Point::ZERO, Point::new(10., 10.)),
        vec![Contour {
            points: vec![
                Point::new(5., 0.),
                Point::new(10., 5.),
                Point::new(5., 10.),
                Point::new(0., 5.),
            ],
            closed: true,
        }],
    );
    s.insert(DisplayObject::new("legacy", custom), None, None)
        .unwrap();
    let mut e = SceneEngine::new(s.clone()).unwrap();
    assert_eq!(e.scene(), &s);
    assert!(
        e.create("unregistered", "no", json!({}), None, None)
            .is_err()
    );
    assert!(
        e.hit_test(Point::new(5., 5.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
}
#[test]
fn metadata_only_edit_does_not_recompute_geometry_caches() {
    let registry = registry(Arc::new(AtomicUsize::new(0)));
    let mut e = SceneEngine::with_registry(Scene::new(), registry).unwrap();
    let (badge, _) = e.create("badge", "badge", json!({}), None, None).unwrap();
    let change = e
        .set_property(badge, PropertyKey::Custom("label".into()), json!("hello"))
        .unwrap();
    assert_eq!(change.layout.world_recomputed, 0);
    assert_eq!(change.layout.bounds_recomputed, 0);
    assert!(change.damage.is_empty());
    assert_eq!(change.dirty[&"badge".into()], DirtyFlags::DATA);
}
#[test]
fn unrelated_edits_do_not_reinvoke_registered_geometry() {
    let counter = Arc::new(AtomicUsize::new(0));
    let registry = registry(counter.clone());
    let mut e = SceneEngine::with_registry(Scene::new(), registry).unwrap();
    let (badge, _) = e.create("badge", "badge", json!({}), None, None).unwrap();
    let before = counter.load(Ordering::Relaxed);
    e.set_property(badge, PropertyKey::Name, json!("name"))
        .unwrap();
    assert_eq!(counter.load(Ordering::Relaxed), before);
}
#[test]
fn builtins_are_real_validated_factories() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (group, _) = e.create("group", "g", json!({}), None, None).unwrap();
    let (rect, _) = e
        .create(
            "rect",
            "r",
            json!({"size":{"width":20,"height":10}}),
            Some(group),
            None,
        )
        .unwrap();
    assert!(matches!(e.object(rect).unwrap().shape, Shape::Rect(_)));
    assert!(
        e.create(
            "rect",
            "invalid",
            json!({"size":{"width":-1,"height":10}}),
            None,
            None
        )
        .is_err()
    );
}
#[test]
fn schema_versions_and_duplicate_definitions_are_rejected() {
    let mut registry = registry(Arc::new(AtomicUsize::new(0)));
    assert!(
        registry
            .create("badge", "bad", json!({"$version":99}))
            .is_err()
    );
    assert!(
        registry
            .register(CustomDefinition::new(
                "badge",
                PropertySchema::default(),
                |_: &Value| Ok((Bounds::from_points(Point::ZERO, Point::new(1., 1.)), vec![]))
            ))
            .is_err()
    );
    assert!(
        registry
            .register(CustomDefinition::new(
                "bad",
                PropertySchema {
                    properties: vec![PropertyDescriptor::number(
                        "r",
                        500.,
                        1.,
                        10.,
                        DirtyFlags::GEOMETRY
                    )],
                    allow_unknown: false
                },
                |_: &Value| Ok((Bounds::from_points(Point::ZERO, Point::new(1., 1.)), vec![]))
            ))
            .is_err()
    );
}
#[test]
fn reset_preserves_registered_factory() {
    let mut e =
        SceneEngine::with_registry(Scene::new(), registry(Arc::new(AtomicUsize::new(0)))).unwrap();
    e.reset_scene(Scene::new()).unwrap();
    assert!(e.create("badge", "badge", json!({}), None, None).is_ok());
}

#[test]
fn explicit_null_change_retains_input_presence_information() {
    let mut registry = ElementRegistry::default();
    registry
        .register(CustomDefinition::new(
            "nullable",
            PropertySchema {
                properties: vec![PropertyDescriptor {
                    name: "extra".into(),
                    value_type: PropertyType::Any,
                    default: None,
                    required: false,
                    dirty: DirtyFlags::DATA,
                }],
                allow_unknown: false,
            },
            |_: &Value| Ok((Bounds::from_points(Point::ZERO, Point::new(1., 1.)), vec![])),
        ))
        .unwrap();
    let mut e = SceneEngine::with_registry(Scene::new(), registry).unwrap();
    let (handle, _) = e.create("nullable", "n", json!({}), None, None).unwrap();
    let c = e
        .set_property(handle, PropertyKey::Custom("extra".into()), Value::Null)
        .unwrap();
    let p = c
        .properties
        .iter()
        .find(|p| p.property == "custom.extra")
        .unwrap();
    assert!(!p.old_present);
    assert!(p.new_present);
    assert_eq!(p.old, Value::Null);
    assert_eq!(p.new, Value::Null);
}
#[test]
fn factory_definition_cannot_change_its_registered_identity() {
    struct Bad;
    impl ElementDefinition for Bad {
        fn kind(&self) -> &str {
            "bad"
        }
        fn schema(&self) -> PropertySchema {
            PropertySchema::default()
        }
        fn shape(&self, _: &Value) -> Result<Shape, String> {
            Ok(display::Rect::new(1., 1.).into())
        }
    }
    let mut registry = ElementRegistry::default();
    registry.register(Bad).unwrap();
    assert!(registry.create("bad", "b", json!({})).is_err());
}
#[test]
fn property_metadata_is_available_to_inspectors() {
    let mut e =
        SceneEngine::with_registry(Scene::new(), registry(Arc::new(AtomicUsize::new(0)))).unwrap();
    let (badge, _) = e.create("badge", "badge", json!({}), None, None).unwrap();
    let desc = e
        .property_descriptor(badge, &PropertyKey::Custom("radius".into()))
        .unwrap();
    assert!(desc.dirty.contains(DirtyFlags::GEOMETRY));
    assert_eq!(desc.default, Some(json!(10.)));
    let opacity = e.property_descriptor(badge, &PropertyKey::Opacity).unwrap();
    assert!(opacity.dirty.contains(DirtyFlags::OPACITY));
}

#[test]
fn all_builtin_definitions_publish_schema_and_valid_defaults() {
    let registry = ElementRegistry::default();
    for kind in registry.kinds() {
        let props = if kind == "image" {
            json!({"source":"asset://example"})
        } else {
            json!({})
        };
        let object = registry.create(kind, kind, props).unwrap();
        object.validate().unwrap();
        assert_eq!(object.shape.tag(), kind);
        assert!(
            registry
                .create(
                    kind,
                    format!("{kind}-invalid"),
                    json!({"unrecognized":true})
                )
                .is_err()
        );
    }
    assert!(registry.create("image", "missing", json!({})).is_err());
}

#[test]
fn pen_branch_factory_preserves_distinct_kind_and_child_identity() {
    let mut engine = SceneEngine::new(Scene::new()).unwrap();
    let (pen, _) = engine
        .create("pen_group", "pen-branch", json!({}), None, None)
        .unwrap();
    let (path, _) = engine
        .create("path", "child", json!({}), Some(pen), None)
        .unwrap();
    assert_eq!(engine.object(pen).unwrap().shape.tag(), "pen_group");
    assert!(engine.object(pen).unwrap().shape.allows_children());
    assert_eq!(engine.object(path).unwrap().shape.tag(), "path");
    let saved = engine.scene().to_json().unwrap();
    let restored = Scene::from_json(&saved).unwrap();
    assert_eq!(restored, *engine.scene());
    assert_eq!(
        restored.get(&"pen-branch".into()).unwrap().children,
        ["child".into()]
    );
    assert!(
        engine
            .create("pen_group", "bad", json!({"path":"M0 0L1 1"}), None, None)
            .is_err()
    );
    let pens = engine
        .query(ElementQuery {
            tag: Some("pen_group"),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(pens, vec![pen]);
    assert!(
        engine
            .query(ElementQuery {
                tag: Some("group"),
                ..Default::default()
            })
            .unwrap()
            .is_empty()
    );
}
