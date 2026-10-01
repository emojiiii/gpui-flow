use gpui_flow_engine::*;
use gpui_flow_scene::{
    Color, DisplayObject, HitPolicy, HitTestOptions, MAX_PAINT_LAYERS, Point, Scene, Stroke, Style,
    display,
};
use serde_json::{json, to_value};

fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    let mut group = DisplayObject::new("group", display::Group::new());
    group.hit_policy = HitPolicy::None;
    scene.insert(group, None, None).unwrap();
    scene
        .insert(
            DisplayObject::new("rect", display::Rect::new(20.0, 20.0)).with_style(Style {
                fill: None,
                ..Style::default()
            }),
            Some("group".into()),
            None,
        )
        .unwrap();
    SceneEngine::new(scene).unwrap()
}
#[test]
fn fill_layer_setter_invalidates_paint_and_hit_without_rebuilding_geometry() {
    let mut engine = engine();
    let handle = engine.handle(&"rect".into()).unwrap();
    assert!(
        engine
            .hit_test(Point::new(10.0, 10.0), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    let fill = json!([{"type":"solid","color":{"r":255,"g":0,"b":0,"a":255}}]);
    let change = engine
        .set_property(handle, PropertyKey::FillLayers, fill.clone())
        .unwrap();
    assert_eq!(change.properties.len(), 1);
    assert_eq!(change.properties[0].property, "style.fill_layers");
    assert_eq!(change.properties[0].old, json!([]));
    assert_eq!(change.properties[0].new, fill);
    assert!(change.dirty[&"rect".into()].contains(DirtyFlags::PAINT | DirtyFlags::HIT));
    assert!(!change.dirty[&"rect".into()].contains(DirtyFlags::GEOMETRY));
    assert_eq!(change.layout.bounds_recomputed, 0);
    assert_eq!(change.layout.world_recomputed, 0);
    assert_eq!(change.layout.paint_invalidated, 1);
    assert!(
        change
            .damage_bounds()
            .unwrap()
            .contains(Point::new(10.0, 10.0))
    );
    assert!(
        engine
            .hit_test(Point::new(10.0, 10.0), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
    assert!(
        !engine
            .set_property(handle, PropertyKey::FillLayers, fill)
            .unwrap()
            .changed()
    );
    engine
        .set_property(handle, PropertyKey::FillLayers, json!([]))
        .unwrap();
    assert!(
        engine
            .hit_test(Point::new(10.0, 10.0), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
}
#[test]
fn stroke_layers_update_ancestor_bounds_hits_and_old_plus_new_damage() {
    let mut engine = engine();
    let handle = engine.handle(&"rect".into()).unwrap();
    let change = engine
        .set_property(
            handle,
            PropertyKey::StrokeLayers,
            to_value(vec![
                Stroke::new(Color::BLACK, 12.0),
                Stroke::new(Color::WHITE, 2.0),
                Stroke::new(Color::TRANSPARENT, 100.0),
            ])
            .unwrap(),
        )
        .unwrap();
    assert!(
        change.dirty[&"rect".into()]
            .contains(DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT)
    );
    assert_eq!(change.layout.world_recomputed, 0);
    assert_eq!(change.layout.bounds_recomputed, 2);
    let expanded = engine.world_bounds(&"rect".into()).unwrap().unwrap();
    assert_eq!(expanded.min_x(), -6.0);
    assert_eq!(
        engine.world_bounds(&"group".into()).unwrap(),
        Some(expanded)
    );
    assert_eq!(
        engine.scene().world_bounds(&"group".into()).unwrap(),
        Some(expanded)
    );
    assert_eq!(
        engine
            .hit_test(Point::new(-5.0, 10.0), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "rect"
    );
    let change = engine
        .set_property(handle, PropertyKey::StrokeLayers, json!([]))
        .unwrap();
    assert!(change.damage_bounds().unwrap().contains(expanded.origin));
    assert_eq!(
        engine
            .world_bounds(&"group".into())
            .unwrap()
            .unwrap()
            .min_x(),
        0.0
    );
    assert!(
        engine
            .hit_test(Point::new(-5.0, 10.0), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
}
#[test]
fn stack_schema_and_failed_changes_preserve_revision_layout_and_input() {
    let mut engine = engine();
    let handle = engine.handle(&"rect".into()).unwrap();
    for key in [PropertyKey::FillLayers, PropertyKey::StrokeLayers] {
        let descriptor = key.descriptor().unwrap();
        assert_eq!(descriptor.value_type, PropertyType::Array);
        assert_eq!(descriptor.default, Some(json!([])));
        let before = engine.scene().clone();
        let revision = engine.revision();
        let layout = engine.layout().clone();
        for value in [json!(null), json!({}), json!([{"width":-1}])] {
            assert!(engine.set_property(handle, key.clone(), value).is_err());
            assert_eq!(engine.scene(), &before);
            assert_eq!(engine.revision(), revision);
            assert_eq!(engine.layout(), &layout);
        }
    }
    let group = engine.handle(&"group".into()).unwrap();
    assert!(
        engine
            .set_property(
                group,
                PropertyKey::FillLayers,
                json!([{"type":"solid","color":{"r":0,"g":0,"b":0,"a":255}}])
            )
            .is_err()
    );
    let revision = engine.revision();
    assert!(
        engine
            .set_property(
                handle,
                PropertyKey::StrokeLayers,
                to_value(vec![Stroke::new(Color::BLACK, 1.0); MAX_PAINT_LAYERS + 1]).unwrap()
            )
            .is_err()
    );
    assert_eq!(engine.revision(), revision);
}
#[test]
fn closure_edits_classify_layer_reordering_and_restoration() {
    let mut engine = engine();
    let change = engine
        .transaction(|scene| {
            scene.update_object(&"rect".into(), |object| {
                object.style.fill_layers = vec![Color::BLACK.into(), Color::WHITE.into()];
            })
        })
        .unwrap();
    assert!(
        change
            .properties
            .iter()
            .any(|change| change.property == "style.fill_layers")
    );
    let change = engine
        .transaction(|scene| {
            scene.update_object(&"rect".into(), |object| object.style.fill_layers.reverse())
        })
        .unwrap();
    assert!(change.changed());
    assert_eq!(change.layout.paint_invalidated, 1);
    assert_eq!(change.layout.bounds_recomputed, 0);
    assert_eq!(
        Scene::from_json(&engine.scene().to_json().unwrap()).unwrap(),
        *engine.scene()
    );
}

#[test]
fn aligned_fill_rule_updates_geometry_damage_and_picking_atomically() {
    use gpui_flow_scene::{FillRule, StrokeAlignment};
    let mut e = engine();
    e.transaction(|s| {
        s.update_object(&"rect".into(), |o| {
            o.shape = display::Path::from_svg("M0 0H40V40H0Z M10 10H30V30H10Z")
                .unwrap()
                .into();
            o.style.stroke =
                Some(Stroke::new(Color::BLACK, 4.).with_alignment(StrokeAlignment::Outside));
        })
    })
    .unwrap();
    let h = e.handle(&"rect".into()).unwrap();
    assert!(
        e.hit_test(Point::new(11., 20.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    let change = e
        .set_property(h, PropertyKey::FillRule, json!("even_odd"))
        .unwrap();
    assert!(
        change.dirty[&"rect".into()]
            .contains(DirtyFlags::GEOMETRY | DirtyFlags::HIT | DirtyFlags::PAINT)
    );
    assert_eq!(change.layout.bounds_recomputed, 2);
    assert!(change.damage_bounds().is_some());
    assert_eq!(
        e.hit_test(Point::new(11., 20.), HitTestOptions::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "rect"
    );
    let before = e.scene().clone();
    let rev = e.revision();
    let cache = e.layout().clone();
    assert!(
        e.set_property(h, PropertyKey::FillRule, json!("invalid"))
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), rev);
    assert_eq!(e.layout(), &cache);
    e.transaction(|s| s.update_object(&"rect".into(), |o| o.style.fill_rule = FillRule::NonZero))
        .unwrap();
    assert!(
        e.hit_test(Point::new(11., 20.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
}
