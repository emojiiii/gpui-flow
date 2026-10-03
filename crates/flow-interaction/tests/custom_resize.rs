use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::*;
use gpui_flow_interaction::{AccessPolicy, Capability, scene::*};
use gpui_flow_scene::{Bounds, Point, Scene, Shape, Size, display::Custom};
use serde_json::{Value, json};
struct Card;
impl ElementDefinition for Card {
    fn kind(&self) -> &str {
        "resizable"
    }
    fn schema(&self) -> PropertySchema {
        PropertySchema {
            allow_unknown: false,
            properties: vec![PropertyDescriptor::number(
                "width",
                40.,
                10.,
                400.,
                DirtyFlags::GEOMETRY,
            )],
        }
    }
    fn shape(&self, p: &Value) -> Result<Shape, String> {
        Ok(Custom::new(self.kind(), bounds(p), vec![]).into())
    }
    fn measure(&self, p: &Value) -> Result<Option<CustomMeasurement>, String> {
        Ok(Some(CustomMeasurement {
            bounds: bounds(p),
            minimum: Size::new(10., 20.),
            maximum: Size::new(400., 20.),
        }))
    }
    fn resize_policy(&self) -> CustomResizePolicy {
        CustomResizePolicy::Dimensions
    }
    fn resize(&self, p: &Value, size: Size) -> Result<Value, String> {
        let mut p = p.clone();
        p["width"] = json!(size.width);
        Ok(p)
    }
}
fn bounds(p: &Value) -> Bounds {
    Bounds::new(
        Point::ZERO,
        Size::new(p["width"].as_f64().unwrap() as f32, 20.),
    )
}
fn setup() -> (
    SceneEditor,
    ScenePluginHost,
    SceneToolController,
    ElementHandle,
) {
    let mut registry = ElementRegistry::default();
    registry.register(Card).unwrap();
    let mut scene = Scene::new();
    scene
        .insert(
            registry.create("resizable", "a", json!({})).unwrap(),
            None,
            None,
        )
        .unwrap();
    let editor = SceneEditor::from_engine(SceneEngine::with_registry(scene, registry).unwrap());
    let h = editor.engine.handle(&"a".into()).unwrap();
    let mut tools = SceneToolController::new();
    tools.register_inner("sizing", "custom").unwrap();
    (editor, ScenePluginHost::new(), tools, h)
}
#[test]
fn repeated_custom_resize_is_atomic_and_one_undo_with_stale_token_rejection() {
    let (mut e, mut host, mut tools, h) = setup();
    let before = e.scene().clone();
    let token = tools.open_inner(&mut e, &mut host, "sizing", h).unwrap();
    let revision = e.engine.revision();
    for width in [80., 120.] {
        tools
            .resize_inner(
                &mut e,
                &mut host,
                token,
                Size::new(width, 20.),
                Point::new(1., 0.5),
            )
            .unwrap();
    }
    assert_eq!(e.engine.revision(), revision + 2);
    let resized = e.scene().clone();
    assert_ne!(before, resized);
    tools.finish(&mut e, &mut host, token).unwrap();
    assert!(e.undo());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
    assert!(e.redo());
    assert_eq!(e.scene(), &resized);
    assert!(
        tools
            .resize_inner(&mut e, &mut host, token, Size::new(160., 20.), Point::ZERO)
            .is_err()
    );
    assert_eq!(e.scene(), &resized);
}
#[test]
fn cancel_restores_exact_document_and_invalid_dimensions_leave_session_usable() {
    let (mut e, mut host, mut tools, h) = setup();
    let before = e.scene().clone();
    let token = tools.open_inner(&mut e, &mut host, "sizing", h).unwrap();
    for size in [
        Size::new(500., 20.),
        Size::new(80., 30.),
        Size::new(f32::INFINITY, 20.),
    ] {
        assert!(
            tools
                .resize_inner(&mut e, &mut host, token, size, Point::ZERO)
                .is_err()
        );
        assert_eq!(e.scene(), &before);
    }
    tools
        .resize_inner(&mut e, &mut host, token, Size::new(80., 20.), Point::ZERO)
        .unwrap();
    assert!(tools.cancel(&mut e, &mut host, token).unwrap());
    assert_eq!(e.scene(), &before);
    assert!(!e.undo());
}
#[test]
fn readonly_and_detached_target_cannot_be_resized_by_queued_tool_callback() {
    let (mut e, mut host, mut tools, h) = setup();
    let token = tools.open_inner(&mut e, &mut host, "sizing", h).unwrap();
    let before = e.scene().clone();
    host.set_policy(AccessPolicy::read_only(), &mut e);
    assert!(
        tools
            .resize_inner(&mut e, &mut host, token, Size::new(80., 20.), Point::ZERO)
            .is_err()
    );
    assert_eq!(e.scene(), &before);
    assert!(!host.allows(Capability::Edit));
    let (mut e, mut host, mut tools, h) = setup();
    let token = tools.open_inner(&mut e, &mut host, "sizing", h).unwrap();
    e.engine.detach(h).unwrap();
    assert!(
        tools
            .resize_inner(&mut e, &mut host, token, Size::new(80., 20.), Point::ZERO)
            .is_err()
    );
}
