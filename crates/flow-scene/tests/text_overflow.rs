use gpui_flow_scene::{
    Shape, Size,
    display::{Text, TextOverflow},
};
#[test]
fn text_overflow_preserves_legacy_defaults_and_roundtrips_opt_in() {
    let mut t = Text::new("original", Size::new(80., 30.));
    let old = serde_json::to_value(&t).unwrap();
    assert!(old.get("overflow").is_none());
    assert_eq!(serde_json::from_value::<Text>(old).unwrap(), t);
    t.overflow = TextOverflow::Ellipsis;
    let value = serde_json::to_value(&t).unwrap();
    assert_eq!(value["overflow"], "ellipsis");
    assert_eq!(serde_json::from_value::<Text>(value).unwrap(), t);
}
#[test]
fn text_ellipsis_does_not_rewrite_component_bounds_or_content() {
    let mut t = Text::new("original long content", Size::new(80., 30.));
    t.padding = [3.; 4];
    let before = t.clone();
    t.overflow = TextOverflow::Ellipsis;
    assert_eq!(t.bounds(), before.bounds());
    assert_eq!(t.content_bounds(), before.content_bounds());
    assert_eq!(t.content, before.content);
    assert_eq!(
        Shape::from(t).local_bounds(),
        Shape::from(before).local_bounds()
    );
}
#[test]
fn text_ellipsis_rejects_oversized_content_without_restricting_legacy_clip() {
    let mut t = Text::new("x".repeat(65537), Size::new(80., 30.));
    t.validate().unwrap();
    t.overflow = TextOverflow::Ellipsis;
    assert!(t.validate().is_err());
    t.content.truncate(65536);
    t.validate().unwrap();
    let mut v = serde_json::to_value(&t).unwrap();
    v["overflow"] = serde_json::json!("show");
    assert!(serde_json::from_value::<Text>(v).is_err());
}
