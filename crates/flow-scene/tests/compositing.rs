use gpui_flow_scene::{BlendMode, DisplayObject, MaskMode, Scene, Style, display};
#[test]
fn legacy_documents_keep_passthrough_and_no_mask() {
    let o:DisplayObject=serde_json::from_str(r#"{"id":"r","shape":{"type":"rect","geometry":{"size":{"width":20,"height":20}}},"style":{"opacity":0.5}}"#).unwrap();
    assert_eq!(o.mask, MaskMode::None);
    assert_eq!(o.style.blend_mode, BlendMode::PassThrough);
    assert_eq!(o.style.opacity, 0.5);
    let mut o = o;
    o.mask = MaskMode::Alpha;
    o.style.blend_mode = BlendMode::Screen;
    let v = serde_json::to_value(&o).unwrap();
    assert_eq!(v["mask"], "alpha");
    assert_eq!(v["style"]["blend_mode"], "screen");
    assert_eq!(serde_json::from_value::<DisplayObject>(v).unwrap(), o);
}
#[test]
fn mask_requires_direct_group_parent_and_preserves_geometric_hit() {
    let mask = DisplayObject::new("mask", display::Rect::new(10., 10.)).with_mask(MaskMode::Alpha);
    let mut s = Scene::new();
    assert!(s.insert(mask.clone(), None, None).is_err());
    assert!(s.is_empty());
    s.insert(DisplayObject::new("g", display::Group::new()), None, None)
        .unwrap();
    s.insert(mask, Some("g".into()), None).unwrap();
    s.insert(
        DisplayObject::new("r", display::Rect::new(50., 50.)).with_style(Style::default()),
        Some("g".into()),
        None,
    )
    .unwrap();
    assert_eq!(
        s.hit_test(gpui_flow_scene::Point::new(40., 40.), Default::default())
            .unwrap()
            .unwrap()
            .id
            .as_str(),
        "r"
    );
}
