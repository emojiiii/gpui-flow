//! Bounded Creative text snapshots for the existing native Scene exporter.
//! Only the rendered text fields are retained; document extensions, font files,
//! shaping and rasterization remain outside this pure conversion layer.

use crate::{
    TextAlignment, TextStyle,
    text_appearance::{MAX_FONT_FAMILY_BYTES, TextAppearance},
};
use gpui_flow::{GpuiTextRasterizer, SceneTextBackendSnapshot};
use gpui_flow_core::{Node, Size};
use gpui_flow_scene::{
    Color, DisplayObject, Style,
    display::{Text, TextAlign},
};
use std::sync::Arc;

/// The standard Creative text payload's UTF-8 budget, checked before copying.
pub const MAX_FRAME_TEXT_BYTES: usize = 16 * 1024;
const HORIZONTAL_PADDING: f32 = 10.;
const VERTICAL_PADDING: f32 = 8.;
const LINE_HEIGHT: f32 = 1.25;
// Used only to validate/admit geometry without an installed native backend.
// The compositor still rejects actual glyph production without that backend.
const ADMISSION_FONT_FAMILY: &str = "sans-serif";

/// An owned, immutable export context from the same App that renders the board.
/// The caller must verify `matches_text_system` when installing the bridge and
/// capture the current UI theme family and installed-family list at export time.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameTextBackend {
    backend: SceneTextBackendSnapshot,
    default_family: String,
    installed_families: Arc<Vec<String>>,
}

impl FrameTextBackend {
    pub fn new(
        bridge: &Arc<GpuiTextRasterizer>,
        default_family: String,
        installed_families: Arc<Vec<String>>,
    ) -> Result<Self, String> {
        validate_default_family(&default_family)?;
        Ok(Self {
            backend: SceneTextBackendSnapshot::new(bridge),
            default_family,
            installed_families,
        })
    }

    pub fn backend(&self) -> &SceneTextBackendSnapshot {
        &self.backend
    }

    /// Font invalidation supersedes a pending export, including owned clones.
    pub fn current(&self) -> bool {
        self.backend.current()
    }
}

/// The complete rendered payload, excluding identity and geometry owned by the
/// enclosing Frame object. Host metadata cannot change snapshot equivalence.
#[derive(Clone, Debug, PartialEq)]
pub struct FrameTextSnapshot {
    content: String,
    style: TextStyle,
    appearance: TextAppearance,
}

impl FrameTextSnapshot {
    pub fn from_node(node: &Node) -> Result<Self, String> {
        if node.kind != "text" {
            return Err("Frame text snapshot requires a text object".into());
        }
        if !node.bounds().is_valid() || !node.size.is_positive() {
            return Err("Frame text geometry must be finite and positive".into());
        }
        let content = node.data["text"]
            .as_str()
            .ok_or("Frame text content must be a string")?;
        if content.len() > MAX_FRAME_TEXT_BYTES {
            return Err("Frame text content exceeds 16 KiB of UTF-8".into());
        }
        // TextAppearance owns its family string. Bound it before that parser
        // copies it, just as we bound content before creating the snapshot.
        if node.data["style"]["font_family"]
            .as_str()
            .is_some_and(|family| family.len() > MAX_FONT_FAMILY_BYTES)
        {
            return Err("Frame text font family exceeds 128 bytes".into());
        }
        Ok(Self {
            content: content.to_owned(),
            style: TextStyle::from_data(&node.data)?,
            appearance: TextAppearance::from_data(&node.data)?,
        })
    }

    pub fn content_bytes(&self) -> usize {
        self.content.len()
    }

    /// Produce a local Scene object; the enclosing Frame owns its translation.
    /// Preserve the original padded wrapping/alignment box. Frame exports use
    /// the explicit ink-overflow producer, which clips only at the Frame boundary.
    /// A missing backend permits admission only, never a substitute font raster.
    pub fn display_object(
        &self,
        id: &str,
        size: Size,
        backend: Option<&FrameTextBackend>,
    ) -> Result<DisplayObject, String> {
        let (default_family, installed_families) =
            backend.map_or((ADMISSION_FONT_FAMILY, &[][..]), |backend| {
                (
                    backend.default_family.as_str(),
                    backend.installed_families.as_slice(),
                )
            });
        let text = self.scene_text(size, default_family, installed_families)?;
        let object = DisplayObject::new(id, text).with_style(Style {
            fill: Some(Color::hex(self.appearance.color).into()),
            ..Default::default()
        });
        object.validate()?;
        Ok(object)
    }

    fn scene_text(
        &self,
        size: Size,
        default_family: &str,
        installed_families: &[String],
    ) -> Result<Text, String> {
        validate_size(size)?;
        let mut text = Text::new(self.content.clone(), size);
        // Match CardRenderer exactly: family names are case-sensitive installed
        // family identities; unavailable names use the inherited UI theme font.
        text.font_family = self
            .appearance
            .installed_family(installed_families)
            .unwrap_or(default_family)
            .to_owned();
        text.font_size = self.style.font_size;
        text.font_weight = if self.style.bold { 700 } else { 400 };
        text.line_height = LINE_HEIGHT;
        text.align = match self.style.alignment {
            TextAlignment::Left => TextAlign::Start,
            TextAlignment::Center => TextAlign::Center,
            TextAlignment::Right => TextAlign::End,
        };
        text.padding = [
            VERTICAL_PADDING,
            HORIZONTAL_PADDING,
            VERTICAL_PADDING,
            HORIZONTAL_PADDING,
        ];
        // Text::new supplies wrapping, top alignment and clip overflow, without
        // introducing rich text, case conversion or additional glyph spacing.
        text.validate()?;
        Ok(text)
    }
}

fn validate_size(size: Size) -> Result<(), String> {
    // Imported/resized objects need not meet the text-placement tool's 24x32
    // minimum. This checks the export layout size, whose height may have been
    // extended by the caller to include native text overflow within the Frame.
    if !size.is_positive()
        || size.width <= HORIZONTAL_PADDING * 2.
        || size.height <= VERTICAL_PADDING * 2.
    {
        return Err(
            "Frame text needs finite dimensions wider than 20 and taller than 16 for its padding"
                .into(),
        );
    }
    Ok(())
}

fn validate_default_family(family: &str) -> Result<(), String> {
    if family.is_empty()
        || family.trim() != family
        || family.len() > MAX_FONT_FAMILY_BYTES
        || family.chars().any(char::is_control)
        || family.contains(['/', '\\'])
    {
        return Err("Frame text default font must be a bounded local font family name".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::Point;
    use gpui_flow_scene::{
        Affine, Bounds, Paint, Shape,
        display::{TextCase, TextDecoration, TextOverflow, TextVerticalAlign, TextWritingMode},
    };
    use serde_json::json;

    fn node() -> Node {
        Node::new(
            "text",
            "A title that is not text content",
            Point::new(45., 67.),
        )
        .with_kind("text")
        .with_size(Size::new(240., 100.))
        .with_data(json!({"text":"第一行 👩🏽‍💻\nCafe\u{301}\n"}))
    }

    #[test]
    fn local_mapping_preserves_unicode_and_the_native_padding_and_line_height() {
        let node = node();
        let snapshot = FrameTextSnapshot::from_node(&node).unwrap();
        let object = snapshot
            .display_object(node.id.as_str(), node.size, None)
            .unwrap();
        assert_eq!(object.transform, Affine::IDENTITY);
        assert_eq!(object.data, serde_json::Value::Null);
        assert_eq!(object.style.fill, Some(Color::hex(0x32343a).into()));
        assert!(object.style.stroke.is_none());
        let Shape::Text(text) = object.shape else {
            panic!("Creative text must use Scene's real text producer");
        };
        assert_eq!(text.content, node.data["text"].as_str().unwrap());
        assert_eq!(snapshot.content_bytes(), text.content.len());
        assert_eq!(text.size, node.size);
        assert_eq!(text.padding, [8., 10., 8., 10.]);
        assert_eq!(
            text.content_bounds(),
            Bounds::new(Point::new(10., 8.), Size::new(220., 84.))
        );
        assert_eq!(
            (text.font_size, text.font_weight, text.line_height),
            (28., 400, 1.25)
        );
        assert_eq!(text.font_family, ADMISSION_FONT_FAMILY);
        assert!(text.wrap);
        assert_eq!(text.align, TextAlign::Start);
        assert_eq!(text.vertical_align, TextVerticalAlign::Top);
        assert_eq!(text.overflow, TextOverflow::Clip);
        assert_eq!(text.writing_mode, TextWritingMode::X);
        assert_eq!(text.text_case, TextCase::None);
        assert_eq!(text.decoration, TextDecoration::None);
        assert!(!text.italic);
        assert_eq!(
            text.letter_spacing + text.word_spacing + text.para_spacing + text.para_indent,
            0.
        );
    }

    #[test]
    fn style_and_installed_family_resolution_match_the_creative_renderer() {
        for (alignment, expected) in [
            ("left", TextAlign::Start),
            ("center", TextAlign::Center),
            ("right", TextAlign::End),
        ] {
            for (font_size, bold) in [(12., false), (42.5, true), (144., true)] {
                let mut node = node();
                node.data["style"] = json!({
                    "font_size":font_size,"bold":bold,"alignment":alignment,
                    "font_family":"Installed Font","color":0x0a2bff
                });
                let snapshot = FrameTextSnapshot::from_node(&node).unwrap();
                let text = snapshot
                    .scene_text(node.size, "Theme Font", &["Installed Font".into()])
                    .unwrap();
                assert_eq!(text.font_family, "Installed Font");
                assert_eq!(text.align, expected);
                assert_eq!(text.font_size, font_size);
                assert_eq!(text.font_weight, if bold { 700 } else { 400 });
                assert_eq!(
                    snapshot
                        .display_object("t", node.size, None)
                        .unwrap()
                        .style
                        .fill,
                    Some(Paint::solid(Color::rgb(10, 43, 255)))
                );
                for installed in [vec![], vec!["installed font".into()]] {
                    assert_eq!(
                        snapshot
                            .scene_text(node.size, "Theme Font", &installed)
                            .unwrap()
                            .font_family,
                        "Theme Font"
                    );
                }
                assert_eq!(
                    snapshot.appearance.font_family.as_deref(),
                    Some("Installed Font")
                );
            }
        }
        assert!(validate_default_family(".SystemUIFont").is_ok());
        for invalid in ["", " padded", "/tmp/font", "line\nbreak"] {
            assert!(validate_default_family(invalid).is_err());
        }
    }

    #[test]
    fn geometry_uses_positive_content_bounds_instead_of_placement_minimums() {
        let mut node = node();
        for size in [
            Size::new(21., 17.),
            Size::new(
                f32::from_bits(20f32.to_bits() + 1),
                f32::from_bits(16f32.to_bits() + 1),
            ),
            Size::new(16_384., 16_384.),
        ] {
            node.size = size;
            let snapshot = FrameTextSnapshot::from_node(&node).unwrap();
            let text = snapshot.scene_text(size, "Theme", &[]).unwrap();
            assert!(text.content_bounds().size.is_positive());
            assert_eq!(text.size, size);
        }
        let snapshot = FrameTextSnapshot::from_node(&node).unwrap();
        for size in [Size::new(20., 100.), Size::new(100., 16.)] {
            node.size = size;
            assert!(FrameTextSnapshot::from_node(&node).is_ok());
            assert!(snapshot.display_object("t", size, None).is_err());
        }
        // An imported short text object is legal. Extending its export height
        // changes neither the native glyph origin nor its wrapping width.
        node.size = Size::new(240., 8.);
        let short = FrameTextSnapshot::from_node(&node).unwrap();
        let extended = short
            .scene_text(Size::new(node.size.width, 308.), "Theme", &[])
            .unwrap();
        assert_eq!(extended.content_bounds().origin, Point::new(10., 8.));
        assert_eq!(extended.content_bounds().size.width, node.size.width - 20.);
        assert_eq!(extended.content_bounds().max_y(), 300.);
        for size in [
            Size::new(-1., 100.),
            Size::new(100., 0.),
            Size::new(100., f32::NAN),
            Size::new(f32::INFINITY, 100.),
        ] {
            node.size = size;
            assert!(FrameTextSnapshot::from_node(&node).is_err());
            assert!(snapshot.display_object("t", size, None).is_err());
        }
        node.size = Size::new(240., 100.);
        node.position.x = f32::NAN;
        assert!(FrameTextSnapshot::from_node(&node).is_err());
    }

    #[test]
    fn snapshot_checks_budgets_and_ignores_unrendered_host_extensions() {
        let mut node = node();
        let snapshot = FrameTextSnapshot::from_node(&node).unwrap();
        node.data["host"] = json!({"large_extension":"x".repeat(128 * 1024)});
        assert_eq!(snapshot, FrameTextSnapshot::from_node(&node).unwrap());
        node.data["text"] = json!("é".repeat(MAX_FRAME_TEXT_BYTES / 2));
        assert_eq!(
            FrameTextSnapshot::from_node(&node).unwrap().content_bytes(),
            MAX_FRAME_TEXT_BYTES
        );
        node.data["text"] = json!("é".repeat(MAX_FRAME_TEXT_BYTES / 2 + 1));
        assert!(FrameTextSnapshot::from_node(&node).is_err());
        node.data["text"] = json!("");
        assert_eq!(
            FrameTextSnapshot::from_node(&node).unwrap().content_bytes(),
            0
        );
        node.data["text"] = json!(false);
        assert!(FrameTextSnapshot::from_node(&node).is_err());
        node.data["text"] = json!("valid");
        node.data["style"] = json!({"font_size":28,"bold":false,"alignment":"left",
            "font_family":"x".repeat(MAX_FONT_FAMILY_BYTES + 1)});
        assert!(FrameTextSnapshot::from_node(&node).is_err());
        node.data["style"]["font_family"] = json!("Valid");
        node.data["style"]["font_size"] = json!(144.0000001);
        assert!(FrameTextSnapshot::from_node(&node).is_err());
        node.data["style"]["font_size"] = json!(28);
        let valid = FrameTextSnapshot::from_node(&node).unwrap();
        node.data["style"]["color"] = json!(0x123456);
        assert_ne!(valid, FrameTextSnapshot::from_node(&node).unwrap());
        node.kind = "image".into();
        assert!(FrameTextSnapshot::from_node(&node).is_err());
    }
}
