use gpui_flow_engine::{DirtyFlags, PropertyKey, PropertyType, SceneEngine};
use gpui_flow_scene::{display, *};
use serde_json::json;

fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("g", display::Frame::new(80., 80.)).with_style(Style {
                blur: 2.,
                ..Default::default()
            }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("mid", display::Group::new()),
            Some("g".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(20., 20.)),
            Some("mid".into()),
            None,
        )
        .unwrap();
    SceneEngine::new(scene).unwrap()
}

#[test]
fn blur_schema_is_paint_only_and_edits_preserve_geometry_hits_and_layout() {
    let schema = PropertyKey::Blur.descriptor().unwrap();
    assert_eq!(schema.name, "style.blur");
    assert_eq!(schema.default, Some(json!(0.)));
    assert_eq!(
        schema.value_type,
        PropertyType::Number {
            min: Some(0.),
            max: Some(256.)
        }
    );
    assert_eq!(schema.dirty, DirtyFlags::PAINT);
    let mut e = engine();
    let handle = e.handle(&"r".into()).unwrap();
    let geometry = e.world_bounds(&"r".into()).unwrap();
    let parent_geometry = e.world_bounds(&"g".into()).unwrap();
    let hit = e.hit_test(Point::new(5., 5.), Default::default()).unwrap();
    for amount in [0.25, 256., 0.] {
        let change = e
            .set_property(handle, PropertyKey::Blur, json!(amount))
            .unwrap();
        assert_eq!(change.properties.len(), 1);
        assert_eq!(change.properties[0].property, "style.blur");
        assert_eq!(change.dirty[&"r".into()], DirtyFlags::PAINT);
        assert_eq!(change.layout.bounds_recomputed, 0);
        assert_eq!(e.world_bounds(&"r".into()).unwrap(), geometry);
        assert_eq!(e.world_bounds(&"g".into()).unwrap(), parent_geometry);
        assert_eq!(
            e.hit_test(Point::new(5., 5.), Default::default()).unwrap(),
            hit
        );
        assert!(change.full_paint_invalidation);
        assert!(change.damage_bounds().unwrap().max_x() >= 88.);
        let revision = e.revision();
        let same = e
            .set_property(handle, PropertyKey::Blur, json!(amount))
            .unwrap();
        assert!(same.properties.is_empty());
        assert!(!same.full_paint_invalidation);
        assert_eq!(e.revision(), revision);
    }
    let revision = e.revision();
    assert!(
        e.set_property(handle, PropertyKey::Blur, json!(-0.))
            .unwrap()
            .properties
            .is_empty()
    );
    assert_eq!(e.revision(), revision);
}

#[test]
fn blur_invalid_hidden_engine_edits_and_effect_overflow_are_atomic() {
    let mut e = engine();
    let handle = e.handle(&"r".into()).unwrap();
    e.set_property(handle, PropertyKey::Visible, json!(false))
        .unwrap();
    let before = e.scene().clone();
    let revision = e.revision();
    for value in [
        json!(-1),
        json!(256.01),
        json!(null),
        json!(true),
        json!("2"),
        json!({"blur":2}),
        json!([]),
    ] {
        assert!(e.set_property(handle, PropertyKey::Blur, value).is_err());
        assert_eq!(e.scene(), &before);
        assert_eq!(e.revision(), revision);
    }
    let result = e.transaction(|s| {
        s.update_object(&"r".into(), |o| {
            o.shape = display::Rect::new(1e-20, 1e-20).into();
            o.transform = Affine::scale(1e38, 1e38);
            o.style.blur = 256.;
        })
    });
    assert!(result.is_err());
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
}

#[test]
fn blur_ancestor_receives_descendant_paint_opacity_order_and_removal_invalidation() {
    let mut e = engine();
    let initial = e.effect_bounds(&"g".into()).unwrap();
    let child = e.handle(&"r".into()).unwrap();
    for (property, value) in [
        (
            PropertyKey::Fill,
            serde_json::to_value(Paint::solid(Color::TRANSPARENT)).unwrap(),
        ),
        (PropertyKey::Opacity, json!(0.5)),
        (PropertyKey::ZIndex, json!(2)),
        (PropertyKey::Eraser, json!("pixel")),
    ] {
        let change = e.set_property(child, property, value).unwrap();
        assert!(change.full_paint_invalidation);
        assert_eq!(e.effect_bounds(&"g".into()).unwrap(), initial);
        assert!(change.damage.contains(&initial.unwrap()));
    }
    let change = e
        .set_property(e.handle(&"g".into()).unwrap(), PropertyKey::Blur, json!(0.))
        .unwrap();
    assert!(change.full_paint_invalidation);
    assert!(change.damage.contains(&initial.unwrap()));
    let change = e
        .set_property(child, PropertyKey::Opacity, json!(1.))
        .unwrap();
    assert!(!change.full_paint_invalidation);
}

#[test]
fn blur_metadata_edits_do_not_request_full_paint() {
    let mut e = engine();
    let change = e
        .set_property(
            e.handle(&"r".into()).unwrap(),
            PropertyKey::Name,
            json!("named"),
        )
        .unwrap();
    assert!(!change.full_paint_invalidation);
}

#[test]
fn removing_the_last_blurred_subtree_still_requests_full_paint() {
    let mut e = engine();
    let old_extent = e.effect_bounds(&"g".into()).unwrap().unwrap();
    let change = e
        .transaction(|scene| {
            scene.remove(&"g".into())?;
            Ok(())
        })
        .unwrap();
    assert!(change.full_paint_invalidation);
    assert!(change.damage.contains(&old_extent));
    assert!(e.scene().roots().is_empty());
}

#[test]
fn nested_blur_analytic_extents_include_child_effects_then_own_shadows_and_scale() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("g", display::Group::new())
                .with_transform(Affine::scale(2., 2.))
                .with_style(Style {
                    blur: 1.,
                    shadow: Some(Shadow::new(20., 0., 0., Color::BLACK)),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("r", display::Rect::new(10., 10.)).with_style(Style {
                blur: 2.,
                ..Default::default()
            }),
            Some("g".into()),
            None,
        )
        .unwrap();
    let e = SceneEngine::new(scene).unwrap();
    assert_eq!(
        e.world_bounds(&"g".into()).unwrap().unwrap(),
        Bounds::new(Point::ZERO, Size::new(20., 20.))
    );
    assert_eq!(
        e.effect_bounds(&"r".into()).unwrap().unwrap(),
        Bounds::new(Point::new(-16., -16.), Size::new(52., 52.))
    );
    assert_eq!(
        e.effect_bounds(&"g".into()).unwrap().unwrap(),
        Bounds::new(Point::new(-24., -24.), Size::new(108., 68.))
    );
}
