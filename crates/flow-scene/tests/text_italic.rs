use gpui_flow_scene::{Size, display::Text};
#[test]
fn italic_default_preserves_legacy_json_and_opt_in_roundtrips() {
    let mut text = Text::new("face", Size::new(100., 40.));
    let legacy = serde_json::to_value(&text).unwrap();
    assert!(legacy.get("italic").is_none());
    assert_eq!(serde_json::from_value::<Text>(legacy).unwrap(), text);
    text.italic = true;
    let encoded = serde_json::to_value(&text).unwrap();
    assert_eq!(encoded["italic"], true);
    assert_eq!(serde_json::from_value::<Text>(encoded).unwrap(), text);
}
#[test]
fn italic_does_not_add_heuristic_width_to_explicit_boxes() {
    let mut text = Text::new("face", Size::new(100., 40.));
    text.padding = [3.; 4];
    let bounds = text.bounds();
    let content = text.content_bounds();
    text.italic = true;
    text.validate().unwrap();
    assert_eq!(text.bounds(), bounds);
    assert_eq!(text.content_bounds(), content);
}
