use gpui_flow_scene::{
    DisplayObject, Scene, Size,
    display::{Text, TextCase},
};
#[test]
fn legacy_default_and_all_supported_cases_roundtrip_without_rewriting_content() {
    let mut text = Text::new("Straße\nİ\n", Size::new(120., 80.));
    let old = serde_json::to_value(&text).unwrap();
    assert!(old.get("text_case").is_none());
    assert!(old.get("para_spacing").is_none());
    assert_eq!(serde_json::from_value::<Text>(old).unwrap(), text);
    for case in [TextCase::None, TextCase::Upper, TextCase::Lower] {
        text.text_case = case;
        text.para_spacing = 12.;
        let value = serde_json::to_value(&text).unwrap();
        assert_eq!(value["content"], "Straße\nİ\n");
        assert_eq!(serde_json::from_value::<Text>(value).unwrap(), text);
        text.validate().unwrap();
    }
}
#[test]
fn paragraph_spacing_rejects_negative_nonfinite_and_over_budget_values() {
    for value in [-1., f32::NAN, f32::INFINITY, 1_000_001.] {
        let mut text = Text::new("a\nb", Size::new(120., 80.));
        text.para_spacing = value;
        assert!(text.validate().is_err());
    }
    for value in [0., 0.5, 1_000_000.] {
        let mut text = Text::new("a\nb", Size::new(120., 80.));
        text.para_spacing = value;
        text.validate().unwrap();
    }
}
#[test]
fn unsupported_leafer_variants_are_rejected_instead_of_approximated() {
    for case in ["title", "small-caps", "small_caps", "capitalize", "UPPER"] {
        let mut value = serde_json::to_value(Text::new("content", Size::new(120., 80.))).unwrap();
        value["text_case"] = serde_json::json!(case);
        assert!(serde_json::from_value::<Text>(value).is_err(), "{case}");
    }
}
#[test]
fn styled_source_and_unicode_expansion_budgets_reject_before_scene_mutation() {
    let mut scene = Scene::new();
    let mut text = Text::new("İ".repeat(32_768), Size::new(120., 80.));
    text.text_case = TextCase::Lower;
    let before = scene.clone();
    assert!(
        scene
            .insert(DisplayObject::new("t", text), None, None)
            .is_err()
    );
    assert_eq!(scene, before);
    let mut text = Text::new("a".repeat(65_537), Size::new(120., 80.));
    text.validate().unwrap();
    text.para_spacing = 1.;
    assert!(text.validate().is_err());
}
#[test]
fn paragraph_styles_do_not_heuristically_resize_explicit_boxes() {
    let mut text = Text::new("Straße\nwords", Size::new(120., 80.));
    text.padding = [3., 4., 5., 6.];
    let bounds = text.bounds();
    let content = text.content_bounds();
    text.text_case = TextCase::Upper;
    text.para_spacing = 13.;
    text.validate().unwrap();
    assert_eq!(text.bounds(), bounds);
    assert_eq!(text.content_bounds(), content);
}
