//! Persistent text formatting stored in a text node's `data.style` object.
//! Missing styles retain the legacy appearance; present styles use an explicit
//! contract. Editing copies the payload so host-owned fields survive unchanged.

use serde_json::{Value, json};

pub const MIN_TEXT_FONT_SIZE: f32 = 12.0;
pub const MAX_TEXT_FONT_SIZE: f32 = 144.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextAlignment {
    #[default]
    Left,
    Center,
    Right,
}

impl TextAlignment {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Center => "center",
            Self::Right => "right",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextStyle {
    pub font_size: f32,
    pub bold: bool,
    pub alignment: TextAlignment,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            font_size: 28.0,
            bold: false,
            alignment: TextAlignment::Left,
        }
    }
}

impl TextStyle {
    /// Read a node's complete data payload. Only an absent `style` uses legacy
    /// defaults. A provided style must contain all three fields; extra fields
    /// remain available for host extensions.
    pub fn from_data(data: &Value) -> Result<Self, String> {
        let data = data.as_object().ok_or("text data must be an object")?;
        let Some(style) = data.get("style") else {
            return Ok(Self::default());
        };
        let style = style.as_object().ok_or("style must be an object")?;
        let font_size = style
            .get("font_size")
            .and_then(Value::as_f64)
            .ok_or("style.font_size must be a number")?;
        // Validate before narrowing to f32 so out-of-range JSON values cannot
        // become valid by rounding to a boundary.
        if !font_size.is_finite()
            || !(f64::from(MIN_TEXT_FONT_SIZE)..=f64::from(MAX_TEXT_FONT_SIZE)).contains(&font_size)
        {
            return Err("style.font_size must be finite and between 12 and 144".into());
        }
        let bold = style
            .get("bold")
            .and_then(Value::as_bool)
            .ok_or("style.bold must be a boolean")?;
        let alignment = match style.get("alignment").and_then(Value::as_str) {
            Some("left") => TextAlignment::Left,
            Some("center") => TextAlignment::Center,
            Some("right") => TextAlignment::Right,
            _ => return Err("style.alignment must be left, center, or right".into()),
        };
        Ok(Self {
            font_size: font_size as f32,
            bold,
            alignment,
        })
    }

    /// Validate an in-memory style before serializing it or applying an edit.
    pub fn validate(&self) -> Result<(), String> {
        if !self.font_size.is_finite()
            || !(MIN_TEXT_FONT_SIZE..=MAX_TEXT_FONT_SIZE).contains(&self.font_size)
        {
            return Err("style.font_size must be finite and between 12 and 144".into());
        }
        Ok(())
    }

    /// Return an edited clone, preserving text, unrelated data and unknown style
    /// fields. Both the existing payload's style and the replacement must be
    /// valid. The caller can send the result through its normal undo pipeline.
    pub fn write_to_data(&self, data: &Value) -> Result<Value, String> {
        self.validate()?;
        Self::from_data(data)?;
        let mut edited = data.clone();
        let object = edited
            .as_object_mut()
            .ok_or("text data must be an object")?;
        let style = object.entry("style").or_insert_with(|| json!({}));
        let style = style.as_object_mut().ok_or("style must be an object")?;
        style.insert("font_size".into(), json!(self.font_size));
        style.insert("bold".into(), json!(self.bold));
        style.insert("alignment".into(), json!(self.alignment.as_str()));
        Ok(edited)
    }
}

/// Validate formatting independently of the host's text/content validation.
pub fn validate_text_style(data: &Value) -> Result<(), String> {
    TextStyle::from_data(data).map(|_| ())
}

/// Read, edit and validate formatting without mutating the original payload.
pub fn edit_text_style(data: &Value, edit: impl FnOnce(&mut TextStyle)) -> Result<Value, String> {
    let mut style = TextStyle::from_data(data)?;
    edit(&mut style);
    style.write_to_data(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn styled_data() -> Value {
        json!({
            "text": "创意标题",
            "style": {"font_size": 36, "bold": true, "alignment": "center"},
            "host": {"record_id": "draft-17", "tags": ["keep", "these"]}
        })
    }

    #[test]
    fn legacy_payload_has_defaults_without_being_rewritten() {
        let data = json!({"text": "Legacy", "host": {"keep": true}});
        let before = data.clone();
        assert_eq!(
            TextStyle::from_data(&data).unwrap(),
            TextStyle {
                font_size: 28.0,
                bold: false,
                alignment: TextAlignment::Left,
            }
        );
        assert!(validate_text_style(&data).is_ok());
        assert_eq!(data, before);
        assert!(data.get("style").is_none());
    }

    #[test]
    fn supplied_styles_require_complete_correctly_typed_fields() {
        for invalid in [
            Value::Null,
            json!(false),
            json!("style"),
            json!([]),
            json!({}),
        ] {
            let mut data = styled_data();
            data["style"] = invalid;
            assert!(TextStyle::from_data(&data).is_err(), "{data}");
        }
        for field in ["font_size", "bold", "alignment"] {
            let mut data = styled_data();
            data["style"].as_object_mut().unwrap().remove(field);
            assert!(validate_text_style(&data).is_err(), "missing {field}");
            for invalid in [Value::Null, json!({}), json!([])] {
                data["style"][field] = invalid;
                assert!(validate_text_style(&data).is_err(), "{data}");
            }
        }
        for (field, invalid) in [
            ("font_size", json!("28")),
            ("font_size", json!(true)),
            ("bold", json!(1)),
            ("bold", json!("false")),
            ("alignment", json!(false)),
            ("alignment", json!("Left")),
            ("alignment", json!("justify")),
        ] {
            let mut data = styled_data();
            data["style"][field] = invalid;
            assert!(validate_text_style(&data).is_err(), "{data}");
        }
        for invalid in [Value::Null, json!([]), json!(42), json!("text")] {
            assert!(TextStyle::from_data(&invalid).is_err());
            assert!(TextStyle::default().write_to_data(&invalid).is_err());
        }
    }

    #[test]
    fn font_size_bounds_apply_before_json_number_rounding() {
        for size in [12.0, 28.5, 144.0] {
            let mut data = styled_data();
            data["style"]["font_size"] = json!(size);
            assert_eq!(TextStyle::from_data(&data).unwrap().font_size, size as f32);
        }
        for size in [-12.0, 0.0, 11.9999999999, 144.0000000001, 1e100] {
            let mut data = styled_data();
            data["style"]["font_size"] = json!(size);
            assert!(validate_text_style(&data).is_err(), "{size}");
        }
    }

    #[test]
    fn nonfinite_and_out_of_range_edits_fail_without_changing_payload() {
        let data = styled_data();
        let before = data.clone();
        for font_size in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, 0.0, 145.0] {
            let style = TextStyle {
                font_size,
                ..TextStyle::default()
            };
            assert!(style.validate().is_err());
            assert!(style.write_to_data(&data).is_err());
            assert!(edit_text_style(&data, |s| s.font_size = font_size).is_err());
            assert_eq!(data, before);
        }
    }

    #[test]
    fn style_edits_preserve_text_and_host_extensions() {
        let mut data = styled_data();
        data["style"]["host_extension"] = json!({"tracking": 1.2, "color": "#112233"});
        let before = data.clone();
        let edited = edit_text_style(&data, |style| {
            style.font_size = 48.5;
            style.bold = false;
            style.alignment = TextAlignment::Right;
        })
        .unwrap();
        assert_eq!(data, before);
        assert_eq!(edited["text"], before["text"]);
        assert_eq!(edited["host"], before["host"]);
        assert_eq!(
            edited["style"]["host_extension"],
            before["style"]["host_extension"]
        );
        assert_eq!(
            TextStyle::from_data(&edited).unwrap(),
            TextStyle {
                font_size: 48.5,
                bold: false,
                alignment: TextAlignment::Right
            }
        );
    }

    #[test]
    fn edited_legacy_style_roundtrips_through_json_for_every_alignment() {
        let original = json!({"text": "Legacy", "host": [1, 2, 3]});
        let mut data = original.clone();
        for alignment in [
            TextAlignment::Left,
            TextAlignment::Center,
            TextAlignment::Right,
        ] {
            data = edit_text_style(&data, |style| {
                style.bold = !style.bold;
                style.alignment = alignment;
                style.font_size += 0.5;
            })
            .unwrap();
            let encoded = serde_json::to_string(&data).unwrap();
            let decoded: Value = serde_json::from_str(&encoded).unwrap();
            assert_eq!(TextStyle::from_data(&decoded), TextStyle::from_data(&data));
            assert_eq!(decoded["style"]["alignment"], alignment.as_str());
            assert_eq!(decoded["text"], original["text"]);
            assert_eq!(decoded["host"], original["host"]);
        }
        let style = TextStyle::from_data(&data).unwrap();
        assert_eq!(style.font_size, 29.5);
        assert!(style.bold);
        assert!(original.get("style").is_none());
    }

    #[test]
    fn malformed_existing_style_is_not_silently_replaced() {
        let mut data = styled_data();
        data["style"]["font_size"] = json!("broken");
        let before = data.clone();
        let mut edit_called = false;
        assert!(edit_text_style(&data, |_| edit_called = true).is_err());
        assert!(!edit_called);
        assert!(TextStyle::default().write_to_data(&data).is_err());
        assert_eq!(data, before);
    }
}
