use gpui_flow_engine::{PropertyKey, SceneEngine};
use gpui_flow_scene::{
    Affine, DisplayObject, Point, Scene, Shape,
    display::{Group, Path, Pen, Rect},
};
use serde_json::json;
fn near(a: f32, b: f32) {
    assert!((a - b).abs() < 0.002, "{a} != {b}");
}
fn engine(pen: bool) -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("group", Group::new())
                .with_transform(Affine::translation(10., 20.) * Affine::rotation(90.)),
            None,
            None,
        )
        .unwrap();
    let path = Path::from_svg("M0 0L10 0").unwrap();
    scene
        .insert(
            DisplayObject::new(
                "path",
                if pen {
                    Shape::Pen(Pen { path })
                } else {
                    Shape::Path(path)
                },
            )
            .with_transform(Affine::scale(2., 3.)),
            Some("group".into()),
            None,
        )
        .unwrap();
    scene
        .insert(DisplayObject::new("rect", Rect::new(2., 2.)), None, None)
        .unwrap();
    SceneEngine::new(scene).unwrap()
}
#[test]
fn motion_queries_return_world_positions_but_local_distances_for_both_kinds() {
    for pen in [false, true] {
        let e = engine(pen);
        let h = e.handle(&"path".into()).unwrap();
        let before = e.scene().clone();
        let layout = e.layout().clone();
        assert_eq!(e.motion_total(h).unwrap(), 10.);
        let sample = e.motion_point(h, 5.).unwrap().unwrap();
        near(sample.point.x, 10.);
        near(sample.point.y, 30.);
        near(sample.tangent.x, 0.);
        near(sample.tangent.y, 1.);
        near(sample.rotation_degrees, 90.);
        assert_eq!(e.scene(), &before);
        assert_eq!(e.layout(), &layout);
        assert_eq!(e.revision(), 0);
    }
}
#[test]
fn queries_reflect_path_and_parent_transform_edits_without_stale_caches() {
    let mut e = engine(false);
    let h = e.handle(&"path".into()).unwrap();
    let parent = e.handle(&"group".into()).unwrap();
    let snapshot = e.measure_path(h).unwrap();
    e.set_path_data(h, Path::from_svg("M0 0L20 0").unwrap())
        .unwrap();
    assert_eq!(snapshot.total_length(), 10.);
    assert_eq!(e.motion_total(h).unwrap(), 20.);
    e.set_property(
        parent,
        PropertyKey::Transform,
        json!(Affine::translation(100., 200.)),
    )
    .unwrap();
    let sample = e.motion_point(h, 10.).unwrap().unwrap();
    near(sample.point.x, 120.);
    near(sample.point.y, 200.);
    near(sample.rotation_degrees, 0.);
}
#[test]
fn invalid_kind_foreign_detached_and_destroyed_handles_are_rejected() {
    let mut e = engine(false);
    let other = engine(false);
    let h = e.handle(&"path".into()).unwrap();
    for invalid in [
        e.handle(&"rect".into()).unwrap(),
        other.handle(&"path".into()).unwrap(),
    ] {
        assert!(e.measure_path(invalid).is_err());
        assert!(e.motion_total(invalid).is_err());
        assert!(e.motion_point(invalid, 0.).is_err());
    }
    let (detached, _) = e.detach(h).unwrap();
    assert!(e.motion_total(h).is_err());
    assert!(e.motion_point(h, 0.).is_err());
    e.attach(&detached, None, None).unwrap();
    e.destroy(h).unwrap();
    assert!(e.motion_total(h).is_err());
}
#[test]
fn empty_paths_have_no_sample_and_collapsed_world_tangents_error() {
    let mut e = engine(false);
    let h = e.handle(&"path".into()).unwrap();
    e.set_property(h, PropertyKey::Transform, json!(Affine::scale(0., 1.)))
        .unwrap();
    assert!(e.motion_point(h, 5.).is_err());
    assert_eq!(e.motion_total(h).unwrap(), 10.);
    e.set_path_data(h, Path::default()).unwrap();
    assert_eq!(e.motion_total(h).unwrap(), 0.);
    assert_eq!(e.motion_point(h, 0.).unwrap(), None);
    assert!(e.motion_point(h, f64::NAN).is_err());
}
#[test]
fn skew_and_reflection_transform_tangents_without_using_rotation_decomposition() {
    let mut e = engine(true);
    let h = e.handle(&"path".into()).unwrap();
    let parent = e.handle(&"group".into()).unwrap();
    e.set_property(parent, PropertyKey::Transform, json!(Affine::IDENTITY))
        .unwrap();
    e.set_property(
        h,
        PropertyKey::Transform,
        json!(Affine::new(-2., 1., 0., 3., 0., 0.)),
    )
    .unwrap();
    e.set_path_data(h, Path::from_svg("M0 0L10 10").unwrap())
        .unwrap();
    let sample = e.motion_point(h, 5. * 2f64.sqrt()).unwrap().unwrap();
    near(sample.point.x, -10.);
    near(sample.point.y, 20.);
    near(sample.rotation_degrees, 116.56505);
    assert!(sample.point != Point::ZERO);
}
