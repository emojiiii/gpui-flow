use gpui_flow_engine::{
    PropertyKey, ResourceCompletion, ResourceOutcome, SceneEngine, SceneResources,
};
use gpui_flow_scene::{Scene, Shape, display::TextVerticalAlign};
use serde_json::json;
#[test]
fn factory_accepts_text_box_fields_and_invalid_updates_leave_layout_untouched() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h,_)=e.create("text","t",json!({"size":{"width":100.,"height":80.},"padding":[4.,8.,12.,16.],"vertical_align":"bottom"}),None,None).unwrap();
    let Shape::Text(t) = &e.object(h).unwrap().shape else {
        unreachable!()
    };
    assert_eq!(t.vertical_align, TextVerticalAlign::Bottom);
    assert_eq!(t.content_bounds().size.width, 76.);
    let before = e.scene().clone();
    let layout = e.layout().clone();
    let mut bad = t.clone();
    bad.padding = [40., 0., 40., 0.];
    assert!(
        e.set_property(h, PropertyKey::Shape, json!(Shape::from(bad)))
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert_eq!(e.layout(), &layout);
}
#[test]
fn padding_alignment_edits_invalidate_text_tickets_without_changing_outer_bounds() {
    let mut e = SceneEngine::new(Scene::new()).unwrap();
    let (h, _) = e.create("text", "t", json!({}), None, None).unwrap();
    let mut r = SceneResources::new(&mut e).unwrap();
    r.synchronize(&mut e).unwrap();
    let old = r.ticket(&e, h).unwrap();
    let bounds = e.world_bounds(&"t".into()).unwrap();
    let Shape::Text(mut t) = e.object(h).unwrap().shape.clone() else {
        unreachable!()
    };
    t.padding = [2., 4., 2., 4.];
    t.vertical_align = TextVerticalAlign::Middle;
    e.set_property(h, PropertyKey::Shape, json!(Shape::from(t)))
        .unwrap();
    assert_eq!(e.world_bounds(&"t".into()).unwrap(), bounds);
    assert!(r.ticket(&e, h).is_none());
    assert_eq!(
        r.complete(&mut e, old, ResourceOutcome::Loaded).unwrap().0,
        ResourceCompletion::Stale
    );
    r.synchronize(&mut e).unwrap();
    assert_ne!(r.ticket(&e, h).unwrap(), old);
}
