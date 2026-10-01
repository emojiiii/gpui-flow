use gpui_flow_engine::*;
use gpui_flow_scene::{ContainerPaintOrder, Shape};
use serde_json::json;

#[test]
fn builtin_container_factory_resolves_legacy_default_and_validates_explicit_order() {
    let registry = ElementRegistry::default();
    for kind in ["box", "frame"] {
        let default = registry.create(kind, "default", json!({})).unwrap();
        let explicit = registry
            .create(
                kind,
                "explicit",
                json!({"paint_order":"stroke_after_children"}),
            )
            .unwrap();
        let order = |shape: Shape| match shape {
            Shape::Box(s) => s.paint_order,
            Shape::Frame(s) => s.paint_order,
            _ => panic!("container expected"),
        };
        assert_eq!(order(default.shape), ContainerPaintOrder::BeforeChildren);
        assert_eq!(
            order(explicit.shape),
            ContainerPaintOrder::StrokeAfterChildren
        );
        assert!(
            registry
                .create(kind, "invalid", json!({"paint_order":"stroke_first"}))
                .is_err()
        );
        assert!(
            registry
                .create(kind, "invalid", json!({"paint_order":true}))
                .is_err()
        );
    }
    assert!(
        registry
            .create(
                "rect",
                "invalid",
                json!({"paint_order":"stroke_after_children"})
            )
            .is_err()
    );
}
