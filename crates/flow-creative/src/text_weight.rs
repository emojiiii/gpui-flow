//! Optional integer font weights alongside the compatible three-field TextStyle.
//! Reading legacy data never rewrites it. Explicit edits keep the legacy `bold`
//! field synchronized so consumers without font-weight support remain compatible.

use crate::{TextStyle, tr};
use serde_json::{Value, json};

pub const MIN_TEXT_FONT_WEIGHT: u16 = 100;
pub const MAX_TEXT_FONT_WEIGHT: u16 = 900;

const INVALID_WEIGHT: &str = "style.font_weight must be an integer between 100 and 900";

/// Read a complete text-node payload without modifying it. An absent weight
/// maps the validated legacy `bold` field to 700 or 400; absent styles use 400.
/// An explicit weight takes precedence over `bold` and must be a JSON integer
/// in 100..=900. Floats (including 400.0), strings and null are rejected rather
/// than rounded or coerced. Existing TextStyle validation remains unchanged.
pub fn read_weight(data: &Value) -> Result<u16, String> {
    let style = TextStyle::from_data(data)?;
    match data["style"].get("font_weight") {
        None => Ok(if style.bold { 700 } else { 400 }),
        Some(weight) => weight
            .as_u64()
            .filter(|weight| {
                (u64::from(MIN_TEXT_FONT_WEIGHT)..=u64::from(MAX_TEXT_FONT_WEIGHT)).contains(weight)
            })
            .map(|weight| weight as u16)
            .ok_or_else(|| INVALID_WEIGHT.into()),
    }
}

/// Return an edited clone with an explicit weight and legacy `bold` set to
/// `weight >= 600`. Validate the existing TextStyle and optional weight before
/// editing; malformed existing values are not silently repaired. The requested
/// weight must also be in 100..=900. TextStyle's writer supplies its three default
/// fields for legacy data and preserves unrelated payload and style fields.
pub fn write_weight(data: &Value, weight: u16) -> Result<Value, String> {
    read_weight(data)?;
    if !(MIN_TEXT_FONT_WEIGHT..=MAX_TEXT_FONT_WEIGHT).contains(&weight) {
        return Err(tr(
            "字重必须在 100 到 900 之间",
            "Font weight must be between 100 and 900",
        )
        .into());
    }
    let mut style = TextStyle::from_data(data)?;
    style.bold = weight >= 600;
    let mut edited = style.write_to_data(data)?;
    edited["style"]
        .as_object_mut()
        .expect("TextStyle writes an object")
        .insert("font_weight".into(), json!(weight));
    Ok(edited)
}

/// Apply the legacy bold shortcut as an explicit 700 (on) or 400 (off) edit.
/// Replaces any prior valid weight so the two fields cannot conflict afterward.
pub fn set_bold(data: &Value, bold: bool) -> Result<Value, String> {
    write_weight(data, if bold { 700 } else { 400 })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styled_data() -> Value {
        json!({
            "text": "创意标题",
            "style": {"font_size": 42.5, "bold": true, "alignment": "center"},
            "host": {"record_id": "draft-17", "tags": ["keep", "these"]}
        })
    }

    #[test]
    fn legacy_weights_are_read_without_rewriting_data() {
        let legacy = json!({"text": "Legacy", "host": {"keep": true}});
        let mut regular = styled_data();
        regular["style"]["bold"] = json!(false);
        for (data, expected) in [(legacy, 400), (regular, 400), (styled_data(), 700)] {
            let before = data.clone();
            assert_eq!(read_weight(&data).unwrap(), expected);
            assert_eq!(data, before);
            assert!(data["style"].get("font_weight").is_none());
        }
    }

    #[test]
    fn integer_bounds_and_intermediate_weights_are_exact() {
        for weight in [100, 101, 400, 550, 599, 600, 700, 899, 900] {
            let mut data = styled_data();
            data["style"]["font_weight"] = json!(weight);
            let before = data.clone();
            assert_eq!(read_weight(&data).unwrap(), weight);
            assert_eq!(data, before);
            let edited = write_weight(&data, weight).unwrap();
            assert_eq!(read_weight(&edited).unwrap(), weight);
            assert_eq!(edited["style"]["bold"], weight >= 600);
        }
        let data = styled_data();
        let before = data.clone();
        for weight in [0, 99, 901, u16::MAX] {
            assert!(write_weight(&data, weight).is_err(), "{weight}");
            assert_eq!(data, before);
        }
    }

    #[test]
    fn malformed_weights_are_rejected_without_coercion_or_repair() {
        for invalid in [
            Value::Null,
            json!("400"),
            json!(false),
            json!(400.0),
            json!(399.5),
            json!(99.9999999999),
            json!(900.0000000001),
            json!(99),
            json!(901),
            json!(-100),
            json!(u64::MAX),
            json!([]),
            json!({}),
        ] {
            let mut data = styled_data();
            data["style"]["font_weight"] = invalid;
            let before = data.clone();
            // The extension does not alter the original TextStyle contract.
            assert!(TextStyle::from_data(&data).is_ok());
            assert!(read_weight(&data).is_err(), "{data}");
            assert!(write_weight(&data, 400).is_err(), "{data}");
            assert!(set_bold(&data, false).is_err(), "{data}");
            assert_eq!(data, before);
        }
    }

    #[test]
    fn existing_text_style_must_be_valid_for_every_operation() {
        let mut invalid_data = vec![
            Value::Null,
            json!([]),
            json!({"style": null}),
            json!({"style": []}),
            json!({"style": {"font_weight": 400}}),
        ];
        for field in ["font_size", "bold", "alignment"] {
            let mut data = styled_data();
            data["style"]["font_weight"] = json!(400);
            data["style"][field] = Value::Null;
            invalid_data.push(data);
        }
        for data in invalid_data {
            let before = data.clone();
            assert!(read_weight(&data).is_err(), "{data}");
            assert!(write_weight(&data, 400).is_err(), "{data}");
            assert!(set_bold(&data, true).is_err(), "{data}");
            assert_eq!(data, before);
        }
    }

    #[test]
    fn edits_preserve_host_fields_and_initialize_legacy_style() {
        let mut source = styled_data();
        source["style"]["host_extension"] = json!({"tracking": 1.2});
        source["style"]["font_family"] = json!("Noto Sans CJK SC");
        source["style"]["color"] = json!(0x123456);
        let before = source.clone();
        let edited = write_weight(&source, 550).unwrap();
        assert_eq!(source, before);
        assert_eq!(edited["text"], source["text"]);
        assert_eq!(edited["host"], source["host"]);
        for field in [
            "font_size",
            "alignment",
            "host_extension",
            "font_family",
            "color",
        ] {
            assert_eq!(edited["style"][field], source["style"][field]);
        }
        let legacy = json!({"text": "Legacy", "host": [1, 2, 3]});
        let initialized = write_weight(&legacy, 900).unwrap();
        assert_eq!(initialized["text"], legacy["text"]);
        assert_eq!(initialized["host"], legacy["host"]);
        assert_eq!(
            initialized["style"],
            json!({"font_size": 28.0, "bold": true, "alignment": "left", "font_weight": 900})
        );
        assert!(legacy.get("style").is_none());
    }

    #[test]
    fn bold_shortcut_replaces_previous_weight_and_roundtrips() {
        let source = write_weight(&styled_data(), 900).unwrap();
        let before = source.clone();
        let regular = set_bold(&source, false).unwrap();
        assert_eq!(read_weight(&regular).unwrap(), 400);
        assert!(!TextStyle::from_data(&regular).unwrap().bold);
        let bold = set_bold(&regular, true).unwrap();
        assert_eq!(read_weight(&bold).unwrap(), 700);
        assert!(TextStyle::from_data(&bold).unwrap().bold);
        assert_eq!(source, before);
        let decoded: Value = serde_json::from_str(&serde_json::to_string(&bold).unwrap()).unwrap();
        assert_eq!(read_weight(&decoded).unwrap(), 700);
        assert!(TextStyle::from_data(&decoded).unwrap().bold);
    }
}
