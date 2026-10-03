//! Strict, bounded exchange of a declared subset of pinned Leafer UI JSON.
//!
//! This is separate from Scene JSON v1. No JavaScript, resource loading, plugin
//! dispatch, computed caches, or permissive unknown-field fallback is involved.
//! See `docs/leafer-json.md` for tag-specific fields and semantic boundaries.
use crate::{
    Affine, BlendMode, Color, ContainerPaintOrder, DisplayId, DisplayObject, EraserMode, FillRule,
    HitPolicy, MaskMode, Paint, Point, Scene, Shape, Size, Stroke, StrokeAlignment, StrokeCap,
    StrokeJoin, Style, display,
};
use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Value, json};
use std::fmt;

pub const MAX_LEAFER_JSON_BYTES: usize = 1_048_576;
pub const MAX_LEAFER_NODES: usize = 512;
pub const MAX_LEAFER_DEPTH: usize = 24;
pub const MAX_LEAFER_STRING_BYTES: usize = 65_536;
pub const MAX_LEAFER_GEOMETRY_ITEMS: usize = 16_384;
const MAX_JSON_DEPTH: usize = 64;
const MAX_JSON_VALUES: usize = 65_536;
const MAX_NUMBER: f64 = 1_000_000.;

/// Admission failure. `path` identifies the JSON node/field when available.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LeaferJsonError {
    pub path: String,
    pub message: String,
}
impl fmt::Display for LeaferJsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Leafer JSON {}: {}", self.path, self.message)
    }
}
impl std::error::Error for LeaferJsonError {}
type Result<T> = std::result::Result<T, LeaferJsonError>;
fn error(path: impl Into<String>, message: impl Into<String>) -> LeaferJsonError {
    LeaferJsonError {
        path: path.into(),
        message: message.into(),
    }
}

// A Value visitor rather than Value::deserialize: duplicate keys, allocation
// budgets and depth are checked while parsing, before building the scene.
struct JsonSeed<'a> {
    depth: usize,
    values: &'a mut usize,
}
impl<'de> DeserializeSeed<'de> for JsonSeed<'_> {
    type Value = Value;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> std::result::Result<Value, D::Error> {
        if self.depth > MAX_JSON_DEPTH || *self.values >= MAX_JSON_VALUES {
            return Err(de::Error::custom("JSON depth/value budget exceeded"));
        }
        *self.values += 1;
        d.deserialize_any(self)
    }
}
impl<'de> Visitor<'de> for JsonSeed<'_> {
    type Value = Value;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("bounded JSON")
    }
    fn visit_unit<E: de::Error>(self) -> std::result::Result<Value, E> {
        Ok(Value::Null)
    }
    fn visit_bool<E: de::Error>(self, v: bool) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_i64<E: de::Error>(self, v: i64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_u64<E: de::Error>(self, v: u64) -> std::result::Result<Value, E> {
        Ok(v.into())
    }
    fn visit_f64<E: de::Error>(self, v: f64) -> std::result::Result<Value, E> {
        serde_json::Number::from_f64(v)
            .map(Value::Number)
            .ok_or_else(|| E::custom("nonfinite number"))
    }
    fn visit_str<E: de::Error>(self, v: &str) -> std::result::Result<Value, E> {
        if v.len() > MAX_LEAFER_STRING_BYTES {
            return Err(E::custom("string budget exceeded"));
        }
        Ok(v.into())
    }
    fn visit_string<E: de::Error>(self, v: String) -> std::result::Result<Value, E> {
        self.visit_str(&v)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        let mut out = Vec::new();
        while let Some(v) = a.next_element_seed(JsonSeed {
            depth: self.depth + 1,
            values: self.values,
        })? {
            out.push(v);
        }
        Ok(Value::Array(out))
    }
    fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> std::result::Result<Value, A::Error> {
        let mut out = Map::new();
        while let Some(key) = a.next_key::<String>()? {
            if key.len() > MAX_LEAFER_STRING_BYTES {
                return Err(de::Error::custom("key budget exceeded"));
            }
            if out.contains_key(&key) {
                return Err(de::Error::custom(format!("duplicate key {key}")));
            }
            let v = a.next_value_seed(JsonSeed {
                depth: self.depth + 1,
                values: self.values,
            })?;
            out.insert(key, v);
        }
        Ok(Value::Object(out))
    }
}

struct Fields {
    map: Map<String, Value>,
    path: String,
}
impl Fields {
    fn new(value: Value, path: String) -> Result<Self> {
        match value {
            Value::Object(map) => Ok(Self { map, path }),
            _ => Err(error(path, "expected node object")),
        }
    }
    fn err(&self, key: &str, message: impl Into<String>) -> LeaferJsonError {
        error(format!("{}.{}", self.path, key), message)
    }
    fn take(&mut self, key: &str) -> Option<Value> {
        self.map.remove(key)
    }
    fn number(&mut self, key: &str, default: f64) -> Result<f64> {
        match self.take(key) {
            None => Ok(default),
            Some(v) => {
                finite(&v).ok_or_else(|| self.err(key, "expected finite number within +/-1000000"))
            }
        }
    }
    fn f32(&mut self, key: &str, default: f32) -> Result<f32> {
        Ok(self.number(key, default as f64)? as f32)
    }
    fn string(&mut self, key: &str, default: &str) -> Result<String> {
        match self.take(key) {
            None => Ok(default.into()),
            Some(Value::String(v)) => Ok(v),
            _ => Err(self.err(key, "expected string")),
        }
    }
    fn boolean(&mut self, key: &str, default: bool) -> Result<bool> {
        match self.take(key) {
            None => Ok(default),
            Some(Value::Bool(v)) => Ok(v),
            _ => Err(self.err(key, "expected boolean")),
        }
    }
    fn size(&mut self, explicit: bool) -> Result<Size> {
        if explicit && (!self.map.contains_key("width") || !self.map.contains_key("height")) {
            return Err(self.err(
                "width",
                "explicit positive width and height required; auto sizing is unsupported",
            ));
        }
        Ok(Size::new(
            self.f32("width", 100.)?,
            self.f32("height", 100.)?,
        ))
    }
    fn finish(self) -> Result<()> {
        if let Some(key) = self.map.keys().next() {
            Err(self.err(key, "unknown or unsupported attribute for this tag"))
        } else {
            Ok(())
        }
    }
}
fn finite(v: &Value) -> Option<f64> {
    v.as_f64()
        .filter(|n| n.is_finite() && n.abs() <= MAX_NUMBER)
}
fn choice<'a>(f: &mut Fields, key: &str, default: &str, choices: &'a [&str]) -> Result<&'a str> {
    let value = f.string(key, default)?;
    choices
        .iter()
        .copied()
        .find(|v| *v == value)
        .ok_or_else(|| f.err(key, format!("unsupported value {value:?}")))
}
fn corners(f: &mut Fields) -> Result<(f32, Option<display::CornerRadii>)> {
    match f.take("cornerRadius") {
        None => Ok((0., None)),
        Some(Value::Array(v)) if v.len() == 4 => {
            let mut out = [0.; 4];
            for (i, v) in v.iter().enumerate() {
                out[i] = finite(v)
                    .ok_or_else(|| f.err("cornerRadius", "expected four finite radii"))?
                    as f32;
            }
            Ok((0., Some(display::CornerRadii(out))))
        }
        Some(v) => finite(&v)
            .map(|n| (n as f32, None))
            .ok_or_else(|| f.err("cornerRadius", "expected scalar or four radii")),
    }
}
fn color(v: Value, f: &Fields, key: &str) -> Result<Option<Paint>> {
    if v.is_null() {
        return Ok(None);
    }
    let s = v
        .as_str()
        .ok_or_else(|| f.err(key, "only null or hex solid colors are supported"))?;
    let hex = s
        .strip_prefix('#')
        .filter(|h| matches!(h.len(), 3 | 4 | 6 | 8) && h.bytes().all(|c| c.is_ascii_hexdigit()))
        .ok_or_else(|| f.err(key, "expected #rgb, #rgba, #rrggbb or #rrggbbaa"))?;
    let mut channels = [255u8; 4];
    if hex.len() <= 4 {
        for (i, c) in hex.bytes().enumerate() {
            channels[i] = (c as char).to_digit(16).unwrap() as u8 * 17;
        }
    } else {
        for (i, pair) in hex.as_bytes().as_chunks::<2>().0.iter().enumerate() {
            channels[i] = u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap();
        }
    }
    Ok(Some(
        Color::rgba(channels[0], channels[1], channels[2], channels[3]).into(),
    ))
}
fn style(f: &mut Fields, tag: &str) -> Result<Style> {
    let default = match tag {
        "Frame" => json!("#ffffff"),
        "Text" => json!("#000000"),
        _ => Value::Null,
    };
    if tag == "Image" && f.map.contains_key("fill") {
        return Err(f.err("fill", "Image.url owns its stretch image fill"));
    }
    let fill_value = f.take("fill").unwrap_or(default);
    let fill = color(fill_value, f, "fill")?;
    let stroke_value = f.take("stroke").unwrap_or(Value::Null);
    let stroke_paint = color(stroke_value, f, "stroke")?;
    let mut out = Style {
        fill,
        opacity: f.f32("opacity", 1.)?,
        ..Style::default()
    };
    out.fill_rule = match choice(f, "windingRule", "nonzero", &["nonzero", "evenodd"])? {
        "evenodd" => FillRule::EvenOdd,
        _ => FillRule::NonZero,
    };
    if let Some(paint) = stroke_paint {
        let default_align = match tag {
            "Path" | "Line" => "center",
            "Text" => "outside",
            _ => "inside",
        };
        let alignment = match choice(
            f,
            "strokeAlign",
            default_align,
            &["inside", "outside", "center"],
        )? {
            "inside" => StrokeAlignment::Inside,
            "outside" => StrokeAlignment::Outside,
            _ => StrokeAlignment::Center,
        };
        let cap = match choice(f, "strokeCap", "none", &["none", "round", "square"])? {
            "none" => StrokeCap::Butt,
            "square" => StrokeCap::Square,
            _ => StrokeCap::Round,
        };
        let join = match choice(f, "strokeJoin", "miter", &["miter", "round", "bevel"])? {
            "miter" => StrokeJoin::Miter,
            "bevel" => StrokeJoin::Bevel,
            _ => StrokeJoin::Round,
        };
        out.stroke = Some(Stroke {
            paint,
            width: f.f32("strokeWidth", 1.)?,
            scale_fixed: crate::StrokeScaleFixed::Disabled,
            alignment,
            cap,
            join,
            miter_limit: f.f32("miterLimit", 10.)?,
            dash_pattern: vec![],
            dash_offset: 0.,
        });
    }
    Ok(out)
}

/// Leafer's pinned setLayout uses angle * PI/180 as shear, NOT tan(angle).
fn transform(f: &mut Fields) -> Result<Affine> {
    let x = f.number("x", 0.)?;
    let y = f.number("y", 0.)?;
    let scale_x = f.number("scaleX", 1.)?;
    let scale_y = f.number("scaleY", 1.)?;
    let r = f.number("rotation", 0.)?.to_radians();
    let sx = f.number("skewX", 0.)?.to_radians();
    let sy = f.number("skewY", 0.)?.to_radians();
    let (sin, cos) = r.sin_cos();
    let t = Affine::new(
        ((cos - sy * sin) * scale_x) as f32,
        ((sin + sy * cos) * scale_x) as f32,
        ((-sin + sx * cos) * scale_y) as f32,
        ((cos + sx * sin) * scale_y) as f32,
        x as f32,
        y as f32,
    );
    if t.is_finite() {
        Ok(t)
    } else {
        Err(f.err("scaleX", "transform overflow"))
    }
}
fn points(f: &mut Fields) -> Result<Vec<Point>> {
    let Some(Value::Array(values)) = f.take("points") else {
        return Err(f.err("points", "explicit points array required"));
    };
    if values.len() > MAX_LEAFER_GEOMETRY_ITEMS * 2 {
        return Err(f.err("points", "point budget exceeded"));
    }
    if values.first().is_some_and(Value::is_object) {
        values
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                let mut p = Fields::new(v, format!("{}.points[{i}]", f.path))?;
                if !p.map.contains_key("x") || !p.map.contains_key("y") {
                    return Err(p.err("x", "x and y required"));
                }
                let point = Point::new(p.f32("x", 0.)?, p.f32("y", 0.)?);
                p.finish()?;
                Ok(point)
            })
            .collect()
    } else {
        if values.len() % 2 != 0 {
            return Err(f.err("points", "flat points must contain x/y pairs"));
        }
        values
            .as_chunks::<2>()
            .0
            .iter()
            .map(|p| {
                Ok(Point::new(
                    finite(&p[0]).ok_or_else(|| f.err("points", "expected finite coordinates"))?
                        as f32,
                    finite(&p[1]).ok_or_else(|| f.err("points", "expected finite coordinates"))?
                        as f32,
                ))
            })
            .collect()
    }
}
fn shape(f: &mut Fields, tag: &str, geometry: &mut usize) -> Result<Shape> {
    let shape = match tag {
        "Group" => Shape::Group(display::Group::new()),
        "Pen" => Shape::Group(display::Group::pen()),
        "Rect" | "Box" | "Frame" | "Canvas" | "Image" => {
            let size = f.size(tag == "Image")?;
            let (radius, radii) = corners(f)?;
            match tag {
                "Rect" => Shape::Rect(display::Rect {
                    size,
                    corner_radius: radius,
                    corner_radii: radii,
                }),
                "Box" | "Frame" => {
                    let clip = choice(
                        f,
                        "overflow",
                        if tag == "Frame" { "hide" } else { "show" },
                        &["hide", "show"],
                    )? == "hide";
                    if tag == "Box" {
                        Shape::Box(display::Box {
                            size,
                            corner_radius: radius,
                            corner_radii: radii,
                            clip_children: clip,
                            paint_order: ContainerPaintOrder::BeforeChildren,
                        })
                    } else {
                        Shape::Frame(display::Frame {
                            size,
                            corner_radius: radius,
                            corner_radii: radii,
                            clip_children: clip,
                            paint_order: ContainerPaintOrder::BeforeChildren,
                        })
                    }
                }
                "Canvas" => Shape::Canvas(display::Canvas {
                    size,
                    corner_radius: radius,
                    corner_radii: radii,
                    pixel_ratio: f.f32("pixelRatio", 1.)?,
                    smooth: f.boolean("smooth", true)?,
                    safe_resize: f.boolean("safeResize", false)?,
                }),
                _ => {
                    let mut image = display::Image::new(f.string("url", "")?, size);
                    image.fit = display::ImageFit::Fill;
                    image.corner_radius = radius;
                    image.corner_radii = radii;
                    Shape::Image(image)
                }
            }
        }
        "Ellipse" => {
            let size = f.size(false)?;
            Shape::Ellipse(display::Ellipse {
                size,
                start_angle: f.f32("startAngle", 0.)?,
                end_angle: f.f32("endAngle", 0.)?,
                inner_radius: f.f32("innerRadius", 0.)?,
                closed: f.boolean("closed", true)?,
            })
        }
        "Line" => {
            let p = if f.map.contains_key("points") {
                points(f)?
            } else {
                vec![Point::ZERO, Point::new(f.f32("width", 100.)?, 0.)]
            };
            if p.len() != 2 {
                return Err(f.err("points", "Line requires exactly two points"));
            }
            if f.boolean("closed", false)? {
                return Err(f.err("closed", "closed Line is unsupported"));
            }
            *geometry += p.len();
            Shape::Line(display::Line::new(p[0], p[1]))
        }
        "Polygon" => {
            let p = points(f)?;
            *geometry += p.len();
            Shape::Polygon(display::Polygon {
                points: p,
                closed: f.boolean("closed", true)?,
            })
        }
        "Star" => {
            let size = f.size(false)?;
            if size.width != size.height {
                return Err(f.err("height", "Star requires equal width and height"));
            }
            let count = f.number("corners", 5.)?;
            if count.fract() != 0. || !(2. ..=4096.).contains(&count) {
                return Err(f.err("corners", "expected integer in 2..=4096"));
            }
            let ratio = f.number("innerRadius", 0.382)?;
            let mut star = display::Star::new(
                count as usize,
                size.width * 0.5,
                (size.width as f64 * 0.5 * ratio) as f32,
            );
            star.start_angle = (f.number("startAngle", 0.)? - 90.) as f32;
            *geometry += count as usize * 2;
            Shape::Star(star)
        }
        "Path" => {
            if !f.map.contains_key("path") {
                return Err(f.err(
                    "path",
                    "explicit SVG path required; implicit box geometry is unsupported",
                ));
            }
            let path = path_input(f)?;
            *geometry += path.commands.len();
            Shape::Path(path)
        }
        "Text" => Shape::Text(text_shape(f)?),
        _ => return Err(f.err("tag", format!("unsupported tag {tag:?}"))),
    };
    if *geometry > MAX_LEAFER_GEOMETRY_ITEMS {
        return Err(f.err("tag", "cumulative geometry budget exceeded"));
    }
    Ok(shape)
}
/// Decode the pinned fixed-arity SVG opcode set. Canvas-only codes are errors.
fn path_input(f: &mut Fields) -> Result<display::Path> {
    let value = f.take("path").expect("presence checked");
    let svg = match value {
        Value::String(svg) => svg,
        Value::Array(values) => {
            use std::fmt::Write;
            let mut svg = String::new();
            let mut at = 0;
            let mut commands = 0;
            while at < values.len() {
                let code = finite(&values[at])
                    .filter(|n| n.fract() == 0.)
                    .ok_or_else(|| f.err("path", "expected integer path opcode"))?;
                let (letter, arity) = match code as i64 {
                    1 => ('M', 2),
                    10 => ('m', 2),
                    2 => ('L', 2),
                    20 => ('l', 2),
                    3 => ('H', 1),
                    30 => ('h', 1),
                    4 => ('V', 1),
                    40 => ('v', 1),
                    5 => ('C', 6),
                    50 => ('c', 6),
                    6 => ('S', 4),
                    60 => ('s', 4),
                    7 => ('Q', 4),
                    70 => ('q', 4),
                    8 => ('T', 2),
                    80 => ('t', 2),
                    9 => ('A', 7),
                    90 => ('a', 7),
                    11 => ('Z', 0),
                    _ => {
                        return Err(
                            f.err("path", format!("unsupported numeric path opcode {code}"))
                        );
                    }
                };
                commands += 1;
                if commands > MAX_LEAFER_GEOMETRY_ITEMS {
                    return Err(f.err("path", "path command budget exceeded"));
                }
                let args = values
                    .get(at + 1..at + 1 + arity)
                    .ok_or_else(|| f.err("path", "truncated numeric path command"))?;
                svg.push(letter);
                for (index, value) in args.iter().enumerate() {
                    let number = finite(value)
                        .ok_or_else(|| f.err("path", "expected finite bounded path coordinate"))?;
                    if matches!(letter, 'A' | 'a')
                        && matches!(index, 3 | 4)
                        && number != 0.
                        && number != 1.
                    {
                        return Err(f.err("path", "arc flags must be zero or one"));
                    }
                    write!(&mut svg, " {number}").expect("String formatting");
                }
                at += 1 + arity;
                if svg.len() > MAX_LEAFER_JSON_BYTES {
                    return Err(f.err("path", "normalized path exceeds byte budget"));
                }
            }
            svg
        }
        _ => return Err(f.err("path", "expected SVG string or numeric command array")),
    };
    display::Path::from_svg(&svg).map_err(|e| f.err("path", e))
}
fn text_shape(f: &mut Fields) -> Result<display::Text> {
    let size = f.size(true)?;
    let mut text = display::Text::new(f.string("text", "")?, size);
    text.font_family = f.string("fontFamily", "caption")?;
    text.font_size = f.f32("fontSize", 12.)?;
    let weight = f.number("fontWeight", 400.)?;
    if weight.fract() != 0. || !(100. ..=900.).contains(&weight) || weight % 100. != 0. {
        return Err(f.err(
            "fontWeight",
            "only numeric weights 100, 200, ..., 900 are supported",
        ));
    }
    text.font_weight = weight as u16;
    text.italic = f.boolean("italic", false)?;
    text.line_height = match f.take("lineHeight") {
        None => 1.5,
        Some(Value::Object(map)) => {
            let mut unit = Fields {
                map,
                path: format!("{}.lineHeight", f.path),
            };
            choice(&mut unit, "type", "", &["percent"])?;
            if !unit.map.contains_key("value") {
                return Err(unit.err("value", "required"));
            }
            let value = unit.f32("value", 1.5)?;
            unit.finish()?;
            value
        }
        Some(v) => {
            (finite(&v).ok_or_else(|| f.err("lineHeight", "expected pixels or percent unit"))?
                / text.font_size as f64) as f32
        }
    };
    text.align = match choice(f, "textAlign", "left", &["left", "center", "right"])? {
        "center" => display::TextAlign::Center,
        "right" => display::TextAlign::End,
        _ => display::TextAlign::Start,
    };
    text.vertical_align = match choice(f, "verticalAlign", "top", &["top", "middle", "bottom"])? {
        "middle" => display::TextVerticalAlign::Middle,
        "bottom" => display::TextVerticalAlign::Bottom,
        _ => display::TextVerticalAlign::Top,
    };
    text.wrap = choice(f, "textWrap", "normal", &["normal", "none"])? == "normal";
    // Flow text always has bounded overflow. Leafer's default 'show' cannot
    // truthfully be converted, so the input must opt into a supported mode.
    text.overflow = match choice(f, "textOverflow", "show", &["hide", "ellipsis"])? {
        "ellipsis" => display::TextOverflow::Ellipsis,
        _ => display::TextOverflow::Clip,
    };
    text.text_case = match choice(f, "textCase", "none", &["none", "upper", "lower"])? {
        "upper" => display::TextCase::Upper,
        "lower" => display::TextCase::Lower,
        _ => display::TextCase::None,
    };
    text.decoration = match choice(
        f,
        "textDecoration",
        "none",
        &["none", "under", "delete", "under-delete"],
    )? {
        "under" => display::TextDecoration::Underline,
        "delete" => display::TextDecoration::Strikethrough,
        "under-delete" => display::TextDecoration::UnderlineStrikethrough,
        _ => display::TextDecoration::None,
    };
    text.para_spacing = f.f32("paraSpacing", 0.)?;
    for (key, target) in [
        ("letterSpacing", &mut text.letter_spacing),
        ("wordSpacing", &mut text.word_spacing),
        ("paraIndent", &mut text.para_indent),
    ] {
        let value = f.number(key, 0.)?;
        if value < 0. {
            return Err(f.err(key, "expected nonnegative spacing"));
        }
        *target = value as f32;
    }
    choice(f, "writingMode", "x", &["x"])?;
    text.auto_size_align = f.boolean("autoSizeAlign", true)?;
    if let Some(v) = f.take("padding") {
        if let Value::Array(a) = v {
            if a.len() != 4 {
                return Err(f.err("padding", "expected scalar or four padding values"));
            }
            for (i, v) in a.iter().enumerate() {
                text.padding[i] =
                    finite(v).ok_or_else(|| f.err("padding", "expected finite padding"))? as f32;
            }
        } else {
            text.padding =
                [finite(&v).ok_or_else(|| f.err("padding", "expected finite padding"))? as f32; 4];
        }
    }
    Ok(text)
}

fn import_node(
    value: Value,
    path: String,
    depth: usize,
    parent: Option<DisplayId>,
    scene: &mut Scene,
    geometry: &mut usize,
) -> Result<()> {
    if depth > MAX_LEAFER_DEPTH {
        return Err(error(path, "node depth budget exceeded"));
    }
    if scene.len() >= MAX_LEAFER_NODES {
        return Err(error(path, "node count budget exceeded"));
    }
    let mut f = Fields::new(value, path)?;
    let tag = f.string("tag", "")?;
    let id = f.string("id", &format!("leafer-{}", scene.len()))?;
    let name = f.string("name", "")?;
    let classes = f
        .string("className", "")?
        .split_whitespace()
        .map(str::to_owned)
        .collect();
    let data = f.take("data").unwrap_or(Value::Null);
    if !data.is_null() && !data.is_object() {
        return Err(f.err("data", "expected metadata object or null"));
    }
    let transform = transform(&mut f)?;
    let style = style(&mut f, &tag)?;
    let visible = f.boolean("visible", true)?;
    let locked = f.boolean("locked", false)?;
    let z = f.number("zIndex", 0.)?;
    if z.fract() != 0. {
        return Err(f.err("zIndex", "expected integer"));
    }
    let shape = shape(&mut f, &tag, geometry)?;
    let child_values = f.take("children");
    if child_values.is_some() && !shape.allows_children() {
        return Err(f.err("children", "this tag does not support children"));
    }
    let children = match child_values {
        None => vec![],
        Some(Value::Array(v)) => v,
        _ => return Err(f.err("children", "expected array")),
    };
    let path = f.path.clone();
    f.finish()?;
    let mut object = DisplayObject::new(id, shape);
    object.name = name;
    object.classes = classes;
    object.data = data;
    object.transform = transform;
    object.style = style;
    object.visible = visible;
    object.locked = locked;
    object.z_index = z as i32;
    let id = object.id.clone();
    scene
        .insert(object, parent, None)
        .map_err(|e| error(&path, e.to_string()))?;
    for (i, child) in children.into_iter().enumerate() {
        import_node(
            child,
            format!("{path}.children[{i}]"),
            depth + 1,
            Some(id.clone()),
            scene,
            geometry,
        )?;
    }
    Ok(())
}

impl Scene {
    /// Import one Leafer node or an array of root nodes. An error never returns
    /// a partial scene and cannot modify any existing scene or resource store.
    pub fn from_leafer_json(input: &str) -> Result<Self> {
        if input.len() > MAX_LEAFER_JSON_BYTES {
            return Err(error("$", "input exceeds 1 MiB"));
        }
        let mut d = serde_json::Deserializer::from_str(input);
        let value = JsonSeed {
            depth: 0,
            values: &mut 0,
        }
        .deserialize(&mut d)
        .map_err(|e| error("$", e.to_string()))?;
        d.end().map_err(|e| error("$", e.to_string()))?;
        let roots = match value {
            Value::Object(_) => vec![value],
            Value::Array(v) => v,
            _ => return Err(error("$", "expected node or root array")),
        };
        let mut scene = Scene::new();
        let mut geometry = 0;
        for (i, value) in roots.into_iter().enumerate() {
            import_node(value, format!("$[{i}]"), 1, None, &mut scene, &mut geometry)?;
        }
        scene.validate().map_err(|e| error("$", e.to_string()))?;
        Ok(scene)
    }
    /// Publish only after every node and accumulated world geometry validates.
    pub fn replace_from_leafer_json(&mut self, input: &str) -> Result<()> {
        let candidate = Self::from_leafer_json(input)?;
        *self = candidate;
        Ok(())
    }
    /// Export a canonical array of Leafer root objects. Unsupported retained
    /// state is rejected rather than dropped. Canvas pixels are never included.
    pub fn to_leafer_json(&self) -> Result<String> {
        if self.len() > MAX_LEAFER_NODES {
            return Err(error("$", "node count budget exceeded"));
        }
        if !self.data.is_null() {
            return Err(error(
                "$.data",
                "scene-level metadata has no forest mapping",
            ));
        }
        admit_export(self)?;
        self.validate().map_err(|e| error("$", e.to_string()))?;
        let mut geometry = 0;
        let roots: Vec<_> = self
            .roots()
            .iter()
            .map(|id| export_node(self, id, 1, &mut geometry))
            .collect::<Result<_>>()?;
        let json = serde_json::to_string(&roots).map_err(|e| error("$", e.to_string()))?;
        if json.len() > MAX_LEAFER_JSON_BYTES {
            return Err(error("$", "output exceeds 1 MiB"));
        }
        // Validate the exact emitted contract as well as the source model. This
        // guards unrepresentable dormant/new fields against silent data loss.
        let restored = Self::from_leafer_json(&json)?;
        if self.roots() != restored.roots() {
            return Err(error("$", "root order cannot round-trip"));
        }
        for node in self.nodes() {
            let other = restored
                .get(&node.object.id)
                .ok_or_else(|| error("$", "missing round-trip node"))?;
            let mut object = other.object.clone();
            if !near_transform(node.object.transform, object.transform) {
                return Err(error(
                    node.object.id.to_string(),
                    "transform cannot round-trip within f32 tolerance",
                ));
            }
            object.transform = node.object.transform;
            if object != node.object
                || node.parent != other.parent
                || node.children != other.children
            {
                return Err(error(
                    node.object.id.to_string(),
                    "retained state cannot round-trip through the supported Leafer subset",
                ));
            }
        }
        Ok(json)
    }
}
// Bound programmatic source data before cloning metadata or invoking geometry
// validation/serialization. Export is a trust boundary too.
fn admit_export(scene: &Scene) -> Result<()> {
    let mut bytes = 0usize;
    let mut values = 0usize;
    let mut geometry = 0usize;
    fn string(s: &str, bytes: &mut usize) -> Result<()> {
        *bytes = bytes.saturating_add(s.len());
        if s.len() > MAX_LEAFER_STRING_BYTES || *bytes > MAX_LEAFER_JSON_BYTES {
            return Err(error("$", "export string/byte budget exceeded"));
        }
        Ok(())
    }
    for node in scene.nodes() {
        let o = &node.object;
        let s = &o.style;
        if o.eraser != EraserMode::None {
            return Err(error(o.id.to_string(), "erasers are unsupported"));
        }
        if s.has_layers()
            || s.has_shadows()
            || s.has_inner_shadows()
            || s.blur != 0.0
            || s.background_blur != 0.0
            || s.has_grayscale()
            || s.blend_mode != BlendMode::PassThrough
        {
            return Err(error(
                o.id.to_string(),
                "layered paints, effects and blend modes are unsupported",
            ));
        }
        if s.fill
            .as_ref()
            .is_some_and(|paint| !matches!(paint, Paint::Solid { .. }))
            || s.stroke
                .as_ref()
                .is_some_and(|stroke| !matches!(stroke.paint, Paint::Solid { .. }))
        {
            return Err(error(
                o.id.to_string(),
                "gradients and non-solid paints are unsupported",
            ));
        }
        if s.strokes().any(|stroke| !stroke.scale_fixed.is_disabled()) {
            return Err(error(
                o.id.to_string(),
                "scale-fixed strokes are unsupported by strict Leafer interchange",
            ));
        }
        if s.stroke
            .as_ref()
            .is_some_and(|stroke| !stroke.dash_pattern.is_empty() || stroke.dash_offset != 0.)
        {
            return Err(error(o.id.to_string(), "dashed strokes are unsupported"));
        }
        string(o.id.as_str(), &mut bytes)?;
        string(&o.name, &mut bytes)?;
        for class in &o.classes {
            string(class, &mut bytes)?;
        }
        let count = match &o.shape {
            Shape::Custom(_) => {
                return Err(error(
                    o.id.to_string(),
                    "custom tags and runtime registrations are unsupported",
                ));
            }
            Shape::Line(_) => 2,
            Shape::Polygon(v) => v.points.len(),
            Shape::Path(v) => v.commands.len(),
            Shape::Pen(v) => v.path.commands.len(),
            Shape::Star(v) => v.points.saturating_mul(2),
            Shape::Text(v) => {
                string(&v.content, &mut bytes)?;
                string(&v.font_family, &mut bytes)?;
                0
            }
            Shape::Image(v) => {
                string(&v.source, &mut bytes)?;
                0
            }
            _ => 0,
        };
        geometry = geometry.saturating_add(count);
        if geometry > MAX_LEAFER_GEOMETRY_ITEMS {
            return Err(error("$", "cumulative geometry budget exceeded"));
        }
        let mut stack = vec![(&o.data, 0usize)];
        while let Some((value, depth)) = stack.pop() {
            values += 1;
            if depth > MAX_JSON_DEPTH || values > MAX_JSON_VALUES {
                return Err(error("$", "export metadata depth/value budget exceeded"));
            }
            match value {
                Value::String(s) => string(s, &mut bytes)?,
                Value::Array(a) => {
                    let remaining = MAX_JSON_VALUES
                        .saturating_sub(values)
                        .saturating_sub(stack.len());
                    if a.len() > remaining {
                        return Err(error("$", "export metadata value budget exceeded"));
                    }
                    stack.extend(a.iter().map(|v| (v, depth + 1)));
                }
                Value::Object(m) => {
                    let remaining = MAX_JSON_VALUES
                        .saturating_sub(values)
                        .saturating_sub(stack.len());
                    if m.len() > remaining {
                        return Err(error("$", "export metadata value budget exceeded"));
                    }
                    for (key, value) in m {
                        string(key, &mut bytes)?;
                        stack.push((value, depth + 1));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}
fn near_transform(a: Affine, b: Affine) -> bool {
    [a.a, a.b, a.c, a.d, a.tx, a.ty]
        .into_iter()
        .zip([b.a, b.b, b.c, b.d, b.tx, b.ty])
        .all(|(x, y)| (x - y).abs() <= 4. * f32::EPSILON * x.abs().max(1.))
}
fn put(map: &mut Map<String, Value>, key: &str, value: impl Into<Value>) {
    map.insert(key.into(), value.into());
}
fn put_size(map: &mut Map<String, Value>, size: Size) {
    put(map, "width", size.width);
    put(map, "height", size.height);
}
fn put_corners(map: &mut Map<String, Value>, radius: f32, radii: Option<display::CornerRadii>) {
    put(
        map,
        "cornerRadius",
        radii.map_or_else(|| json!(radius), |r| json!(r.0)),
    );
}
fn put_paint(paint: Option<&Paint>) -> Result<Value> {
    match paint {
        None => Ok(Value::Null),
        Some(Paint::Solid { color: c }) => Ok(json!(format!(
            "#{:02x}{:02x}{:02x}{:02x}",
            c.r, c.g, c.b, c.a
        ))),
        _ => Err(error(
            "paint",
            "gradients and non-solid paints are unsupported",
        )),
    }
}
fn export_transform(map: &mut Map<String, Value>, t: Affine) -> Result<()> {
    put(map, "x", t.tx);
    put(map, "y", t.ty);
    let (a, b, c, d) = (t.a as f64, t.b as f64, t.c as f64, t.d as f64);
    let (sx, sy, r, kx, ky) = if b == 0. && c == 0. {
        (a, d, 0., 0., 0.)
    } else if a != 0.
        && d != 0.
        && (c / d).to_degrees().abs() <= MAX_NUMBER
        && (b / a).to_degrees().abs() <= MAX_NUMBER
    {
        (a, d, 0., (c / d).to_degrees(), (b / a).to_degrees())
    } else {
        let sx = a.hypot(b);
        let det = a * d - b * c;
        if sx == 0. || det == 0. {
            return Err(error(
                "transform",
                "non-axis singular transform is outside the export subset",
            ));
        }
        let sy = det / sx;
        let r = b.atan2(a);
        (
            sx,
            sy,
            r.to_degrees(),
            ((a * c + b * d) / (sx * sy)).to_degrees(),
            0.,
        )
    };
    if [sx, sy, r, kx, ky]
        .iter()
        .any(|n| !n.is_finite() || n.abs() > MAX_NUMBER)
    {
        return Err(error(
            "transform",
            "decomposed transform exceeds finite exchange range",
        ));
    }
    for (key, value) in [
        ("scaleX", sx),
        ("scaleY", sy),
        ("rotation", r),
        ("skewX", kx),
        ("skewY", ky),
    ] {
        put(map, key, value);
    }
    Ok(())
}
fn export_node(scene: &Scene, id: &DisplayId, depth: usize, geometry: &mut usize) -> Result<Value> {
    if depth > MAX_LEAFER_DEPTH {
        return Err(error(id.to_string(), "node depth budget exceeded"));
    }
    let node = scene
        .get(id)
        .ok_or_else(|| error(id.to_string(), "missing node"))?;
    let o = &node.object;
    let s = &o.style;
    if o.hit_policy != HitPolicy::Painted || !o.hit_children || o.mask != MaskMode::None {
        return Err(error(
            id.to_string(),
            "hit overrides and masks are unsupported",
        ));
    }
    if s.has_layers()
        || s.has_shadows()
        || s.has_inner_shadows()
        || s.blur != 0.0
        || s.background_blur != 0.0
        || s.has_grayscale()
        || s.blend_mode != BlendMode::PassThrough
    {
        return Err(error(
            id.to_string(),
            "layered paints, effects and blend modes are unsupported",
        ));
    }
    let mut m = Map::new();
    put(&mut m, "id", o.id.as_str());
    put(&mut m, "name", o.name.clone());
    if o.classes
        .iter()
        .any(|c| c.is_empty() || c.chars().any(char::is_whitespace))
    {
        return Err(error(
            id.to_string(),
            "class names must be single nonempty tokens",
        ));
    }
    put(
        &mut m,
        "className",
        o.classes.iter().cloned().collect::<Vec<_>>().join(" "),
    );
    put(&mut m, "data", o.data.clone());
    put(&mut m, "visible", o.visible);
    put(&mut m, "locked", o.locked);
    put(&mut m, "zIndex", o.z_index);
    export_transform(&mut m, o.transform)?;
    put(&mut m, "opacity", s.opacity);
    put(
        &mut m,
        "windingRule",
        if s.fill_rule == FillRule::EvenOdd {
            "evenodd"
        } else {
            "nonzero"
        },
    );
    if !matches!(o.shape, Shape::Image(_)) {
        put(&mut m, "fill", put_paint(s.fill.as_ref())?);
    }
    if let Some(stroke) = &s.stroke {
        if !stroke.dash_pattern.is_empty() || stroke.dash_offset != 0. {
            return Err(error(id.to_string(), "dashed strokes are unsupported"));
        }
        put(&mut m, "stroke", put_paint(Some(&stroke.paint))?);
        put(&mut m, "strokeWidth", stroke.width);
        put(
            &mut m,
            "strokeAlign",
            match stroke.alignment {
                StrokeAlignment::Inside => "inside",
                StrokeAlignment::Outside => "outside",
                StrokeAlignment::Center => "center",
            },
        );
        put(
            &mut m,
            "strokeCap",
            match stroke.cap {
                StrokeCap::Butt => "none",
                StrokeCap::Round => "round",
                StrokeCap::Square => "square",
            },
        );
        put(
            &mut m,
            "strokeJoin",
            match stroke.join {
                StrokeJoin::Miter => "miter",
                StrokeJoin::Round => "round",
                StrokeJoin::Bevel => "bevel",
            },
        );
        put(&mut m, "miterLimit", stroke.miter_limit);
    }
    let tag = match &o.shape {
        Shape::Rect(v) => {
            put_size(&mut m, v.size);
            put_corners(&mut m, v.corner_radius, v.corner_radii);
            "Rect"
        }
        Shape::Ellipse(v) => {
            put_size(&mut m, v.size);
            put(&mut m, "startAngle", v.start_angle);
            put(&mut m, "endAngle", v.end_angle);
            put(&mut m, "innerRadius", v.inner_radius);
            put(&mut m, "closed", v.closed);
            "Ellipse"
        }
        Shape::Line(v) => {
            put(
                &mut m,
                "points",
                json!([v.start.x, v.start.y, v.end.x, v.end.y]),
            );
            *geometry += 2;
            "Line"
        }
        Shape::Polygon(v) => {
            put(
                &mut m,
                "points",
                json!(v.points.iter().flat_map(|p| [p.x, p.y]).collect::<Vec<_>>()),
            );
            put(&mut m, "closed", v.closed);
            *geometry += v.points.len();
            "Polygon"
        }
        Shape::Star(v) => {
            if v.center != Point::new(v.outer_radius, v.outer_radius) {
                return Err(error(
                    id.to_string(),
                    "Star center must equal its outer radius on both axes",
                ));
            }
            put_size(&mut m, Size::new(v.outer_radius * 2., v.outer_radius * 2.));
            put(&mut m, "corners", v.points as u64);
            put(
                &mut m,
                "innerRadius",
                v.inner_radius as f64 / v.outer_radius as f64,
            );
            put(&mut m, "startAngle", v.start_angle as f64 + 90.);
            *geometry += v.points * 2;
            "Star"
        }
        Shape::Path(v) => {
            put(
                &mut m,
                "path",
                v.to_svg().map_err(|e| error(id.to_string(), e))?,
            );
            *geometry += v.commands.len();
            "Path"
        }
        Shape::Text(v) => {
            if v.layout_origin != Point::ZERO {
                return Err(error(
                    id.to_string(),
                    "computed text layout_origin has no explicit-size Leafer mapping",
                ));
            }
            export_text(&mut m, v);
            "Text"
        }
        Shape::Image(v) => {
            // Compare the complete source model below too: new Flow-only image
            // fields cannot disappear through this adapter unnoticed.
            if v.source_crop.is_some() {
                return Err(error(
                    id.to_string(),
                    "Flow source_crop has no supported Leafer mapping",
                ));
            }
            if v.fit != display::ImageFit::Fill || v.natural_size.is_some() || s.fill.is_some() {
                return Err(error(
                    id.to_string(),
                    "Image requires stretch fit, no natural-size metadata and no separate fill",
                ));
            }
            put_size(&mut m, v.size);
            put_corners(&mut m, v.corner_radius, v.corner_radii);
            put(&mut m, "url", v.source.clone());
            "Image"
        }
        Shape::Canvas(v) => {
            put_size(&mut m, v.size);
            put_corners(&mut m, v.corner_radius, v.corner_radii);
            put(&mut m, "pixelRatio", v.pixel_ratio);
            put(&mut m, "smooth", v.smooth);
            put(&mut m, "safeResize", v.safe_resize);
            "Canvas"
        }
        Shape::Group(group) => match group.kind() {
            display::GroupKind::Group => "Group",
            display::GroupKind::Pen => "Pen",
        },
        Shape::Box(v) => {
            put_size(&mut m, v.size);
            put_corners(&mut m, v.corner_radius, v.corner_radii);
            put(
                &mut m,
                "overflow",
                if v.clip_children { "hide" } else { "show" },
            );
            "Box"
        }
        Shape::Frame(v) => {
            put_size(&mut m, v.size);
            put_corners(&mut m, v.corner_radius, v.corner_radii);
            put(
                &mut m,
                "overflow",
                if v.clip_children { "hide" } else { "show" },
            );
            "Frame"
        }
        Shape::Pen(_) => {
            return Err(error(
                id.to_string(),
                "Flow leaf Pen cannot map to Leafer's branch Pen without an explicit conversion",
            ));
        }
        Shape::Custom(_) => {
            return Err(error(
                id.to_string(),
                "custom tags and runtime registrations are unsupported",
            ));
        }
    };
    if *geometry > MAX_LEAFER_GEOMETRY_ITEMS {
        return Err(error(id.to_string(), "cumulative geometry budget exceeded"));
    }
    put(&mut m, "tag", tag);
    if o.shape.allows_children() {
        put(
            &mut m,
            "children",
            Value::Array(
                node.children
                    .iter()
                    .map(|id| export_node(scene, id, depth + 1, geometry))
                    .collect::<Result<_>>()?,
            ),
        );
    }
    Ok(Value::Object(m))
}
fn export_text(m: &mut Map<String, Value>, v: &display::Text) {
    put_size(m, v.size);
    put(m, "text", v.content.clone());
    put(m, "fontFamily", v.font_family.clone());
    put(m, "fontSize", v.font_size);
    put(m, "fontWeight", v.font_weight);
    put(m, "italic", v.italic);
    put(
        m,
        "lineHeight",
        json!({"type":"percent","value":v.line_height}),
    );
    put(
        m,
        "textAlign",
        match v.align {
            display::TextAlign::Start => "left",
            display::TextAlign::Center => "center",
            display::TextAlign::End => "right",
        },
    );
    put(
        m,
        "verticalAlign",
        match v.vertical_align {
            display::TextVerticalAlign::Top => "top",
            display::TextVerticalAlign::Middle => "middle",
            display::TextVerticalAlign::Bottom => "bottom",
        },
    );
    put(m, "textWrap", if v.wrap { "normal" } else { "none" });
    put(
        m,
        "textOverflow",
        if v.overflow == display::TextOverflow::Ellipsis {
            "ellipsis"
        } else {
            "hide"
        },
    );
    put(
        m,
        "textCase",
        match v.text_case {
            display::TextCase::None => "none",
            display::TextCase::Upper => "upper",
            display::TextCase::Lower => "lower",
        },
    );
    put(
        m,
        "textDecoration",
        match v.decoration {
            display::TextDecoration::None => "none",
            display::TextDecoration::Underline => "under",
            display::TextDecoration::Strikethrough => "delete",
            display::TextDecoration::UnderlineStrikethrough => "under-delete",
        },
    );
    put(m, "paraSpacing", v.para_spacing);
    put(m, "letterSpacing", v.letter_spacing);
    put(m, "wordSpacing", v.word_spacing);
    put(m, "paraIndent", v.para_indent);
    put(m, "writingMode", "x");
    put(m, "autoSizeAlign", v.auto_size_align);
    put(m, "padding", json!(v.padding));
}
