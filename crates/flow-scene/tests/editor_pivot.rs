use gpui_flow_scene::{display::Rect, *};

#[test]
fn optional_local_pivot_roundtrips_defaults_and_never_changes_render_bounds() {
    let before =
        DisplayObject::new("shape", Rect::new(100., 60.)).with_transform(Affine::rotation(27.));
    let mut object = before.clone().with_pivot(Point::new(-15., 300.));
    assert_eq!(object.paint_bounds(), before.paint_bounds());
    assert_eq!(object.effect_bounds(), before.effect_bounds());
    assert_eq!(object.transform, before.transform);
    let json = serde_json::to_value(&object).unwrap();
    assert_eq!(
        serde_json::from_value::<DisplayObject>(json).unwrap(),
        object
    );
    let mut legacy = serde_json::to_value(&before).unwrap();
    legacy.as_object_mut().unwrap().remove("pivot");
    assert_eq!(
        serde_json::from_value::<DisplayObject>(legacy)
            .unwrap()
            .pivot,
        None
    );
    for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        object.pivot = Some(Point::new(value, 1.));
        assert!(object.validate().is_err());
        assert!(Scene::new().insert(object.clone(), None, None).is_err());
    }
}

#[test]
fn leafer_export_explicitly_rejects_flow_pivot_without_mutating_the_scene() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("shape", Rect::new(100., 60.)).with_pivot(Point::new(4., 5.)),
            None,
            None,
        )
        .unwrap();
    let before = scene.clone();
    assert!(
        scene
            .to_leafer_json()
            .unwrap_err()
            .to_string()
            .contains("Flow editor pivot")
    );
    assert_eq!(scene, before);
    assert_eq!(Scene::from_json(&scene.to_json().unwrap()).unwrap(), scene);
}
