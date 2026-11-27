//! Optional typography fields alongside the compatible, three-field TextStyle.
//! Font names are local family names, never file paths or download requests.

use crate::{TextStyle, tr};
use serde_json::{Value, json};

pub const DEFAULT_TEXT_COLOR: u32 = 0x32343a;
pub const MAX_FONT_FAMILY_BYTES: usize = 128;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TextAppearance {
    pub font_family: Option<String>,
    pub color: u32,
}

impl Default for TextAppearance {
    fn default() -> Self {
        Self {
            font_family: None,
            color: DEFAULT_TEXT_COLOR,
        }
    }
}

impl TextAppearance {
    pub fn from_data(data: &Value) -> Result<Self, String> {
        TextStyle::from_data(data)?;
        let style = &data["style"];
        let font_family = match style.get("font_family") {
            None => None,
            Some(Value::String(family)) => Some(family.clone()),
            _ => return Err("style.font_family must be a local font family name".into()),
        };
        let color = match style.get("color") {
            None => DEFAULT_TEXT_COLOR,
            Some(value) => value
                .as_u64()
                .filter(|value| *value <= 0xffffff)
                .ok_or("style.color must be an RGB integer between 0 and 0xffffff")?
                as u32,
        };
        let appearance = Self { font_family, color };
        appearance.validate()?;
        Ok(appearance)
    }

    pub fn validate(&self) -> Result<(), String> {
        if let Some(name) = &self.font_family
            && (name.is_empty()
                || name.trim() != name
                || name.len() > MAX_FONT_FAMILY_BYTES
                || name.chars().any(char::is_control)
                || name.contains(['/', '\\']))
        {
            return Err(tr(
                "字体必须使用不超过 128 字节的本地字体名称",
                "Font family must be a non-empty local name of at most 128 bytes",
            )
            .into());
        }
        if self.color > 0xffffff {
            return Err(tr(
                "文字颜色必须是 24 位 RGB 值",
                "Text color must be a 24-bit RGB value",
            )
            .into());
        }
        Ok(())
    }

    /// Copy all host fields and the original three TextStyle fields unchanged.
    pub fn write_to_data(&self, data: &Value) -> Result<Value, String> {
        self.validate()?;
        Self::from_data(data)?;
        let mut edited = TextStyle::from_data(data)?.write_to_data(data)?;
        let style = edited["style"]
            .as_object_mut()
            .expect("TextStyle writes an object");
        if let Some(family) = &self.font_family {
            style.insert("font_family".into(), json!(family));
        } else {
            style.remove("font_family");
        }
        style.insert("color".into(), json!(self.color));
        Ok(edited)
    }

    /// Resolve only an installed family. A missing name is retained in the
    /// document so reopening on another machine can restore its appearance.
    pub fn installed_family<'a>(&'a self, available: &[String]) -> Option<&'a str> {
        self.font_family
            .as_deref()
            .filter(|name| available.iter().any(|item| item == name))
    }
}

pub fn parse_text_color(value: &str) -> Result<u32, String> {
    let value = value.trim().strip_prefix('#').unwrap_or(value.trim());
    if value.len() != 6 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(tr(
            "请输入六位颜色，例如 #3B82F6",
            "Enter a six-digit hex color, such as #3B82F6",
        )
        .into());
    }
    u32::from_str_radix(value, 16).map_err(|_| tr("颜色无效", "Invalid color").into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_defaults_and_new_fields_preserve_existing_style_and_host_data() {
        let legacy = json!({"text":"文字","host":{"keep":true}});
        assert_eq!(
            TextAppearance::from_data(&legacy).unwrap(),
            TextAppearance::default()
        );
        let source = json!({"text":"文字","host":{"keep":true},"style":{
            "font_size":42,"bold":true,"alignment":"center","host_style":"keep"}});
        let edit = TextAppearance {
            font_family: Some("Noto Sans CJK SC".into()),
            color: 0x3b82f6,
        };
        let changed = edit.write_to_data(&source).unwrap();
        assert_eq!(
            TextStyle::from_data(&changed).unwrap(),
            TextStyle::from_data(&source).unwrap()
        );
        assert_eq!(changed["host"], source["host"]);
        assert_eq!(changed["style"]["host_style"], "keep");
        assert_eq!(TextAppearance::from_data(&changed).unwrap(), edit);
        let resized = crate::edit_text_style(&changed, |style| style.font_size = 64.).unwrap();
        assert_eq!(TextAppearance::from_data(&resized).unwrap(), edit);
        assert!(source["style"].get("font_family").is_none());
        let reset = TextAppearance::default().write_to_data(&changed).unwrap();
        assert!(reset["style"].get("font_family").is_none());
    }

    #[test]
    fn validates_names_colors_and_color_entry_before_editing() {
        for name in ["", " bad", "bad\nname", "/tmp/font", "C:\\font"] {
            assert!(
                TextAppearance {
                    font_family: Some(name.into()),
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
        assert!(
            TextAppearance {
                font_family: Some("a".repeat(129)),
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        let source =
            json!({"text":"test","style":{"font_size":28,"bold":false,"alignment":"left"}});
        for color in [
            json!(-1),
            json!(0x1000000u32),
            json!(1.5),
            json!("ff0000"),
            Value::Null,
        ] {
            let mut data = source.clone();
            data["style"]["color"] = color;
            assert!(TextAppearance::from_data(&data).is_err());
        }
        assert_eq!(parse_text_color(" #Aa01fF ").unwrap(), 0xaa01ff);
        for invalid in ["#fff", "#12345678", "blue", "12 456"] {
            assert!(parse_text_color(invalid).is_err());
        }
    }

    #[test]
    fn unavailable_font_falls_back_without_discarding_the_requested_family() {
        let appearance = TextAppearance {
            font_family: Some("Missing Family".into()),
            ..Default::default()
        };
        assert_eq!(appearance.installed_family(&["Other Family".into()]), None);
        assert_eq!(
            appearance.installed_family(&["Missing Family".into()]),
            Some("Missing Family")
        );
        assert_eq!(appearance.font_family.as_deref(), Some("Missing Family"));
    }

    #[test]
    fn standard_text_registry_checks_optional_typography_fields() {
        let registry = crate::standard_registry();
        let mut data = TextAppearance {
            font_family: Some("Noto Serif CJK SC".into()),
            color: 0x123456,
        }
        .write_to_data(&json!({"text":"标题"}))
        .unwrap();
        assert!(
            registry
                .make_node(
                    "text",
                    "type",
                    gpui_flow_core::Point::new(0., 0.),
                    Some(data.clone())
                )
                .is_ok()
        );
        data["style"]["color"] = json!(0xff123456u32);
        assert!(
            registry
                .make_node(
                    "text",
                    "type",
                    gpui_flow_core::Point::new(0., 0.),
                    Some(data)
                )
                .is_err()
        );
    }
}
