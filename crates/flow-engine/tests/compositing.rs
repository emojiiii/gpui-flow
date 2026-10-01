use gpui_flow_engine::{DirtyFlags, PropertyKey, SceneEngine};
use gpui_flow_scene::{DisplayObject, Scene, display};
use serde_json::json;
#[test]
fn blend_and_mask_setters_report_specific_dirty_bits_and_reject_invalid_values() {
    let mut s = Scene::new();
    s.insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    s.insert(
        DisplayObject::new("r", display::Rect::new(20., 20.)),
        Some("g".into()),
        None,
    )
    .unwrap();
    let mut e = SceneEngine::new(s).unwrap();
    let h = e.handle(&"r".into()).unwrap();
    let c = e
        .set_property(h, PropertyKey::BlendMode, json!("multiply"))
        .unwrap();
    assert_eq!(c.properties[0].property, "style.blend_mode");
    assert!(c.dirty[&"r".into()].contains(DirtyFlags::PAINT));
    assert!(!c.dirty[&"r".into()].contains(DirtyFlags::HIT));
    let c = e
        .set_property(h, PropertyKey::Mask, json!("alpha"))
        .unwrap();
    assert!(c.dirty[&"r".into()].contains(DirtyFlags::PAINT | DirtyFlags::GEOMETRY));
    assert!(!c.dirty[&"r".into()].contains(DirtyFlags::HIT));
    for mode in [
        "overlay",
        "darken",
        "lighten",
        "color_dodge",
        "color_burn",
        "hard_light",
        "soft_light",
        "difference",
        "exclusion",
        "hue",
        "saturation",
        "color",
        "luminosity",
    ] {
        let changed = e
            .set_property(h, PropertyKey::BlendMode, json!(mode))
            .unwrap();
        assert!(changed.dirty[&"r".into()].contains(DirtyFlags::PAINT));
        assert!(!changed.dirty[&"r".into()].contains(DirtyFlags::HIT));
    }
    let before = e.scene().clone();
    assert!(
        e.set_property(h, PropertyKey::BlendMode, json!("unknown_blend"))
            .is_err()
    );
    assert_eq!(*e.scene(), before);
    assert!(
        e.set_property(
            e.handle(&"g".into()).unwrap(),
            PropertyKey::Mask,
            json!("alpha")
        )
        .is_err()
    );
    assert_eq!(
        PropertyKey::BlendMode.descriptor().unwrap().default,
        Some(json!("pass_through"))
    );
    assert_eq!(
        PropertyKey::Mask.descriptor().unwrap().default,
        Some(json!("none"))
    );
}
