use gpui_flow_engine::{PropertyKey, SceneEngine, SceneResources};
use gpui_flow_scene::{HitTestOptions, Point, Scene, Shape};
use serde_json::json;
#[test]
fn image_corner_factory_and_atomic_edits_keep_resource_ticket_and_update_hits() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let(h,_)=e.create("image","i",json!({"source":"x.png","fit":"fill","size":{"width":40.,"height":40.},"corner_radii":[20.,0.,0.,0.]}),None,None).unwrap();
    let mut r = SceneResources::new(&mut e).unwrap();
    r.synchronize(&mut e).unwrap();
    let ticket = r.ticket(&e, h).unwrap();
    let bounds = e.world_bounds(&"i".into()).unwrap();
    assert!(
        e.hit_test(Point::new(1., 1.), HitTestOptions::default())
            .unwrap()
            .is_none()
    );
    let Shape::Image(mut image) = e.object(h).unwrap().shape.clone() else {
        unreachable!()
    };
    image.corner_radii = Some([0., 20., 0., 0.].into());
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(image)))
        .unwrap();
    assert_eq!(e.world_bounds(&"i".into()).unwrap(), bounds);
    assert_eq!(r.ticket(&e, h), Some(ticket));
    assert!(
        e.hit_test(Point::new(1., 1.), HitTestOptions::default())
            .unwrap()
            .is_some()
    );
}
#[test]
fn invalid_image_corners_preserve_document_and_derived_cache() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h, _) = e
        .create("image", "i", json!({"source":"x.png"}), None, None)
        .unwrap();
    let before = e.scene().clone();
    let layout = e.layout().clone();
    let Shape::Image(image) = e.object(h).unwrap().shape.clone() else {
        unreachable!()
    };
    assert!(
        e.set_property(
            h,
            PropertyKey::Shape,
            json!(Shape::from(image.with_corners([-1.; 4])))
        )
        .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.layout(), &layout);
}
