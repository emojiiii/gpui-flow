use gpui_flow_scene::{
    Size,
    display::{Text, TextDecoration as D},
};
#[test]
fn decoration_defaults_roundtrip_without_changing_explicit_bounds() {
    let mut t = Text::new("source", Size::new(100., 60.));
    let json = serde_json::to_value(&t).unwrap();
    assert!(json.get("decoration").is_none());
    let old = t.bounds();
    for kind in [
        D::None,
        D::Underline,
        D::Strikethrough,
        D::UnderlineStrikethrough,
    ] {
        t.decoration = kind;
        t.validate().unwrap();
        assert_eq!(
            serde_json::from_value::<Text>(serde_json::to_value(&t).unwrap()).unwrap(),
            t
        );
        assert_eq!(t.bounds(), old);
    }
}
#[test]
fn decorated_text_rejects_over_budget_content_and_unknown_modes() {
    let mut t = Text::new("x".repeat(65537), Size::new(100., 60.));
    t.validate().unwrap();
    t.decoration = D::Underline;
    assert!(t.validate().is_err());
    t.content = "small".into();
    let mut value = serde_json::to_value(t).unwrap();
    value["decoration"] = serde_json::json!("wavy");
    assert!(serde_json::from_value::<Text>(value).is_err());
}
