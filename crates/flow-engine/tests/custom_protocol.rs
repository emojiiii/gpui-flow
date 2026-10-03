use gpui_flow_engine::*;
use gpui_flow_scene::{Affine, Bounds, DisplayObject, Point, Scene, Shape, Size, display::Custom};
use serde_json::{Value, json};
use std::{
    future::Future,
    pin::pin,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    task::{Context, Poll, Wake, Waker},
};

struct Card {
    policy: CustomResizePolicy,
    bad: bool,
}
impl ElementDefinition for Card {
    fn kind(&self) -> &str {
        "card"
    }
    fn schema(&self) -> PropertySchema {
        PropertySchema {
            allow_unknown: false,
            properties: vec![
                PropertyDescriptor::number("width", 40., 10., 400., DirtyFlags::GEOMETRY),
                PropertyDescriptor::number("height", 20., 10., 400., DirtyFlags::GEOMETRY),
                PropertyDescriptor {
                    name: "source".into(),
                    value_type: PropertyType::String,
                    default: Some(json!("a")),
                    required: false,
                    dirty: DirtyFlags::GEOMETRY,
                },
            ],
        }
    }
    fn shape(&self, p: &Value) -> Result<Shape, String> {
        Ok(Custom::new("card", bounds(p), vec![]).into())
    }
    fn measure(&self, p: &Value) -> Result<Option<CustomMeasurement>, String> {
        let mut bounds = bounds(p);
        if self.bad {
            bounds.size.width += 1.;
        }
        Ok(Some(CustomMeasurement {
            bounds,
            minimum: Size::new(10., 10.),
            maximum: Size::new(400., 400.),
        }))
    }
    fn resize_policy(&self) -> CustomResizePolicy {
        self.policy
    }
    fn resize(&self, p: &Value, size: Size) -> Result<Value, String> {
        let mut p = p.clone();
        p["width"] = json!(size.width);
        p["height"] = json!(size.height);
        Ok(p)
    }
    fn resource_key(&self, p: &Value) -> Result<Option<String>, String> {
        Ok(Some(p["source"].as_str().unwrap().into()))
    }
}
fn bounds(p: &Value) -> Bounds {
    Bounds::new(
        Point::new(5., 7.),
        Size::new(
            p["width"].as_f64().unwrap() as f32,
            p["height"].as_f64().unwrap() as f32,
        ),
    )
}
fn setup(policy: CustomResizePolicy, bad: bool) -> (SceneEngine, ElementHandle) {
    let mut registry = ElementRegistry::default();
    registry.register(Card { policy, bad }).unwrap();
    let mut object = registry.create("card", "a", json!({})).unwrap();
    object.transform =
        Affine::translation(50., 30.) * Affine::rotation(25.) * Affine::scale(2., -1.);
    let mut scene = Scene::new();
    scene.insert(object, None, None).unwrap();
    let engine = SceneEngine::with_registry(scene, registry).unwrap();
    let h = engine.handle(&"a".into()).unwrap();
    (engine, h)
}
#[test]
fn measured_resize_preserves_affine_anchor_and_is_pure_until_atomic_commit() {
    let (mut e, h) = setup(CustomResizePolicy::Dimensions, false);
    let before = e.scene().clone();
    let revision = e.revision();
    let old = e.object(h).unwrap().clone();
    let anchor = Point::new(1., 0.5);
    let patch = e
        .preview_custom_resize(h, Size::new(80., 60.), anchor)
        .unwrap();
    assert_eq!(e.scene(), &before);
    assert_eq!(e.revision(), revision);
    assert_eq!(
        (
            patch.transform.a,
            patch.transform.b,
            patch.transform.c,
            patch.transform.d
        ),
        (
            old.transform.a,
            old.transform.b,
            old.transform.c,
            old.transform.d
        )
    );
    let original = old.transform.transform_point(Point::new(45., 17.));
    let resized = patch.transform.transform_point(Point::new(85., 37.));
    assert!((original.x - resized.x).abs() < 0.0001 && (original.y - resized.y).abs() < 0.0001);
    e.set_properties(
        h,
        [
            (
                PropertyKey::Shape,
                serde_json::to_value(patch.shape).unwrap(),
            ),
            (
                PropertyKey::Transform,
                serde_json::to_value(patch.transform).unwrap(),
            ),
        ],
    )
    .unwrap();
    assert_eq!(e.revision(), revision + 1);
    assert_eq!(
        e.measure_custom(h).unwrap().bounds.size,
        Size::new(80., 60.)
    );
}
#[test]
fn policy_limits_invalid_measurement_and_bad_anchors_reject_without_changes() {
    for (policy, bad, size, anchor) in [
        (
            CustomResizePolicy::Disabled,
            false,
            Size::new(80., 40.),
            Point::ZERO,
        ),
        (
            CustomResizePolicy::Uniform,
            false,
            Size::new(80., 60.),
            Point::ZERO,
        ),
        (
            CustomResizePolicy::Dimensions,
            true,
            Size::new(80., 40.),
            Point::ZERO,
        ),
        (
            CustomResizePolicy::Dimensions,
            false,
            Size::new(1., 20.),
            Point::ZERO,
        ),
        (
            CustomResizePolicy::Dimensions,
            false,
            Size::new(40., 20.),
            Point::new(f32::NAN, 0.),
        ),
    ] {
        let (e, h) = setup(policy, bad);
        let before = e.scene().clone();
        assert!(e.preview_custom_resize(h, size, anchor).is_err());
        assert_eq!(e.scene(), &before);
    }
    let (e, h) = setup(CustomResizePolicy::Uniform, false);
    assert!(
        e.preview_custom_resize(h, Size::new(80., 40.), Point::ZERO)
            .is_ok()
    );
}
struct WakeCount(AtomicUsize);
impl Wake for WakeCount {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[test]
fn custom_source_changes_wake_cancel_before_render_and_reject_late_results() {
    let (mut e, h) = setup(CustomResizePolicy::Dimensions, false);
    let mut r = SceneResources::new(&mut e).unwrap();
    let events = r.synchronize(&mut e).unwrap();
    assert_eq!(events.len(), 1);
    let t = r.ticket(&e, h).unwrap();
    assert_eq!(t.kind, ResourceKind::Custom);
    let token = r.cancellation_token(&e, t).unwrap();
    let mut cancelled = pin!(token.cancelled());
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    assert_eq!(cancelled.as_mut().poll(&mut cx), Poll::Pending);
    e.set_property(h, PropertyKey::Custom("source".into()), json!("b"))
        .unwrap();
    assert!(token.is_cancelled());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert_eq!(cancelled.as_mut().poll(&mut cx), Poll::Ready(()));
    assert_eq!(
        r.complete(&mut e, t, ResourceOutcome::Loaded).unwrap().0,
        ResourceCompletion::Stale
    );
    let events = r.synchronize(&mut e).unwrap();
    assert_eq!(events.len(), 2);
    let fresh = r.ticket(&e, h).unwrap();
    assert_ne!(t, fresh);
    assert_eq!(r.custom_key(&e, fresh).as_deref(), Some("b"));
    assert_eq!(
        r.complete(&mut e, fresh, ResourceOutcome::Loaded)
            .unwrap()
            .0,
        ResourceCompletion::Accepted
    );
    assert_eq!(
        r.complete(&mut e, fresh, ResourceOutcome::Loaded)
            .unwrap()
            .0,
        ResourceCompletion::AlreadySettled
    );
    assert_eq!(r.counts(&e).loaded, 1);
}
#[test]
fn detach_dispose_and_tracker_drop_cancel_custom_lifetimes() {
    let (mut e, h) = setup(CustomResizePolicy::Dimensions, false);
    let mut r = SceneResources::new(&mut e).unwrap();
    r.synchronize(&mut e).unwrap();
    let t = r.ticket(&e, h).unwrap();
    let token = r.cancellation_token(&e, t).unwrap();
    e.detach(h).unwrap();
    assert!(token.is_cancelled());
    assert_eq!(
        r.complete(&mut e, t, ResourceOutcome::Loaded).unwrap().0,
        ResourceCompletion::Stale
    );
    let (mut e, h) = setup(CustomResizePolicy::Dimensions, false);
    let mut r = SceneResources::new(&mut e).unwrap();
    r.synchronize(&mut e).unwrap();
    let t = r.ticket(&e, h).unwrap();
    let token = r.cancellation_token(&e, t).unwrap();
    r.dispose(&mut e).unwrap();
    assert!(token.is_cancelled());
    assert!(r.dispose(&mut e).unwrap().is_empty());
    let (mut e, h) = setup(CustomResizePolicy::Dimensions, false);
    let mut r = SceneResources::new(&mut e).unwrap();
    r.synchronize(&mut e).unwrap();
    let token = r.cancellation_token(&e, r.ticket(&e, h).unwrap()).unwrap();
    drop(r);
    assert!(token.is_cancelled());
}
#[test]
fn inert_custom_without_registered_resource_declaration_is_not_pending() {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new(
                "inert",
                Custom::new(
                    "unknown",
                    Bounds::new(Point::ZERO, Size::new(20., 20.)),
                    vec![],
                ),
            ),
            None,
            None,
        )
        .unwrap();
    let mut e = SceneEngine::new(scene).unwrap();
    let mut r = SceneResources::new(&mut e).unwrap();
    assert!(r.synchronize(&mut e).unwrap().is_empty());
    assert_eq!(r.counts(&e).total(), 0);
    assert!(r.frame(&e).is_some());
}
#[test]
fn explicit_cancelled_outcome_wakes_producer_and_is_terminal() {
    let (mut e, h) = setup(CustomResizePolicy::Dimensions, false);
    let mut r = SceneResources::new(&mut e).unwrap();
    r.synchronize(&mut e).unwrap();
    let t = r.ticket(&e, h).unwrap();
    let token = r.cancellation_token(&e, t).unwrap();
    let wakes = Arc::new(WakeCount(AtomicUsize::new(0)));
    let waker = Waker::from(wakes.clone());
    let mut cx = Context::from_waker(&waker);
    let mut wait = pin!(token.cancelled());
    assert!(wait.as_mut().poll(&mut cx).is_pending());
    r.complete(
        &mut e,
        t,
        ResourceOutcome::Cancelled(ResourceCancellation::NotAuthorized),
    )
    .unwrap();
    assert!(token.is_cancelled());
    assert_eq!(wakes.0.load(Ordering::SeqCst), 1);
    assert_eq!(
        r.complete(&mut e, t, ResourceOutcome::Loaded).unwrap().0,
        ResourceCompletion::AlreadySettled
    );
}
#[test]
fn layout_cannot_move_parent_behind_custom_resize_anchor() {
    struct MoveParent;
    impl LayoutProvider for MoveParent {
        fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
            let mut next = candidate.clone();
            let Shape::Custom(custom) = &next.get(&"a".into()).unwrap().object.shape else {
                unreachable!()
            };
            let x = if custom.bounds.size.width > 40. {
                20.
            } else {
                0.
            };
            next.update_object(&"parent".into(), |o| {
                o.transform = Affine::translation(x, 0.)
            })?;
            Ok(next)
        }
    }
    let (mut e, h) = setup(CustomResizePolicy::Dimensions, false);
    e.transaction(|scene| {
        scene.insert(
            DisplayObject::new("parent", gpui_flow_scene::display::Group::new()),
            None,
            None,
        )?;
        scene.reparent(&"a".into(), Some("parent".into()), None, false)
    })
    .unwrap();
    e.install_layout_provider(MoveParent).unwrap();
    let before = e.scene().clone();
    assert!(
        e.preview_custom_resize(h, Size::new(80., 40.), Point::new(0.5, 0.5))
            .is_err()
    );
    assert_eq!(e.scene(), &before);
}
