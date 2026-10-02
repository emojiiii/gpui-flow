//! Public pure preview exercises the exact typed property commit preparation.
use gpui_flow_engine::{ChangeOrigin, EngineError, LayoutProvider, PropertyKey, SceneEngine};
use gpui_flow_scene::{Affine, DisplayObject, Scene, display};
use serde_json::json;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn engine() -> SceneEngine {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("a", display::Rect::new(40., 30.)),
            None,
            None,
        )
        .unwrap();
    SceneEngine::new(scene).unwrap()
}
#[test]
fn successful_preview_has_no_identity_revision_observer_or_cache_commit() {
    let mut engine = engine();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = calls.clone();
    engine
        .on_change(None, move |_| {
            observed.fetch_add(1, Ordering::SeqCst);
        })
        .unwrap();
    let handle = engine.handle(&"a".into()).unwrap();
    let original = engine.scene().clone();
    let change = engine.last_change().clone();
    let world = engine.world_bounds(&"a".into()).unwrap();
    let candidate = engine
        .preview_properties_batch_with_origin(
            ChangeOrigin::Animation,
            [
                (
                    handle,
                    PropertyKey::Transform,
                    json!(Affine::translation(80., 15.)),
                ),
                (handle, PropertyKey::Opacity, json!(0.5)),
            ],
        )
        .unwrap();
    assert_eq!(candidate.get(&"a".into()).unwrap().object.transform.tx, 80.);
    assert_eq!(
        candidate.get(&"a".into()).unwrap().object.style.opacity,
        0.5
    );
    assert_eq!(engine.scene(), &original);
    assert_eq!(engine.revision(), 0);
    assert_eq!(engine.last_change(), &change);
    assert_eq!(engine.world_bounds(&"a".into()).unwrap(), world);
    assert_eq!(engine.handle(&"a".into()), Some(handle));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}
#[test]
fn invalid_later_property_or_foreign_handle_cannot_publish_an_earlier_preview_field() {
    let engine = engine();
    let handle = engine.handle(&"a".into()).unwrap();
    let original = engine.scene().clone();
    assert!(
        engine
            .preview_properties_batch_with_origin(
                ChangeOrigin::Edit,
                [
                    (
                        handle,
                        PropertyKey::Transform,
                        json!(Affine::translation(80., 15.))
                    ),
                    (handle, PropertyKey::Opacity, json!(2.)),
                ]
            )
            .is_err()
    );
    let other = SceneEngine::new(original.clone()).unwrap();
    assert!(
        engine
            .preview_properties_batch_with_origin(
                ChangeOrigin::Edit,
                [(
                    other.handle(&"a".into()).unwrap(),
                    PropertyKey::Opacity,
                    json!(0.5)
                ),]
            )
            .is_err()
    );
    assert_eq!(engine.scene(), &original);
    assert_eq!(engine.revision(), 0);
}
#[test]
fn preview_runs_same_provider_and_rejects_animated_property_ownership_conflict() {
    struct FixedX;
    impl LayoutProvider for FixedX {
        fn derive(&self, _: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
            let mut next = candidate.clone();
            next.update_object(&"a".into(), |o| o.transform.tx = 0.)?;
            Ok(next)
        }
    }
    let mut engine = engine();
    engine.install_layout_provider(FixedX).unwrap();
    let handle = engine.handle(&"a".into()).unwrap();
    let properties = [(
        handle,
        PropertyKey::Transform,
        json!(Affine::translation(80., 15.)),
    )];
    assert!(matches!(
        engine.preview_properties_batch_with_origin(ChangeOrigin::Animation, properties.clone()),
        Err(EngineError::Layout(_))
    ));
    let candidate = engine
        .preview_properties_batch_with_origin(ChangeOrigin::Edit, properties)
        .unwrap();
    assert_eq!(
        candidate.get(&"a".into()).unwrap().object.transform,
        Affine::translation(0., 15.)
    );
    assert_eq!(engine.object(handle).unwrap().transform, Affine::IDENTITY);
    assert_eq!(engine.revision(), 0);
}
