use crate::{CustomMeasurement, CustomResizePolicy, DirtyFlags, EngineError};
use gpui_flow_scene::{
    Bounds, Contour, DisplayId, DisplayObject, Point, Scene, Shape, Size, display::Custom,
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Debug, Clone, PartialEq)]
pub enum PropertyType {
    Number { min: Option<f64>, max: Option<f64> },
    Boolean,
    String,
    Object,
    Array,
    Any,
}
impl PropertyType {
    fn accepts(&self, value: &Value) -> bool {
        match self {
            Self::Number { min, max } => value.as_f64().is_some_and(|n| {
                n.is_finite() && min.is_none_or(|m| n >= m) && max.is_none_or(|m| n <= m)
            }),
            Self::Boolean => value.is_boolean(),
            Self::String => value.is_string(),
            Self::Object => value.is_object(),
            Self::Array => value.is_array(),
            Self::Any => true,
        }
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyDescriptor {
    pub name: String,
    pub value_type: PropertyType,
    pub default: Option<Value>,
    pub required: bool,
    pub dirty: DirtyFlags,
}
impl PropertyDescriptor {
    pub fn number(
        name: impl Into<String>,
        default: f64,
        min: f64,
        max: f64,
        dirty: DirtyFlags,
    ) -> Self {
        Self {
            name: name.into(),
            value_type: PropertyType::Number {
                min: Some(min),
                max: Some(max),
            },
            default: Some(json!(default)),
            required: false,
            dirty,
        }
    }
}
#[derive(Debug, Default, Clone, PartialEq)]
pub struct PropertySchema {
    pub properties: Vec<PropertyDescriptor>,
    pub allow_unknown: bool,
}
impl PropertySchema {
    /// Resolve defaults without rewriting the caller's serialized input.
    pub fn resolve(&self, input: &Value) -> Result<Value, String> {
        let input = input
            .as_object()
            .ok_or("element properties must be an object")?;
        let mut output = input.clone();
        if !self.allow_unknown {
            for name in input.keys() {
                if name != "$version" && !self.properties.iter().any(|p| &p.name == name) {
                    return Err(format!("unknown property {name}"));
                }
            }
        }
        for p in &self.properties {
            if let Some(value) = output.get(&p.name) {
                if !p.value_type.accepts(value) {
                    return Err(format!("invalid property {}", p.name));
                }
            } else if let Some(default) = &p.default {
                if !p.value_type.accepts(default) {
                    return Err(format!("invalid default {}", p.name));
                }
                output.insert(p.name.clone(), default.clone());
            } else if p.required {
                return Err(format!("missing property {}", p.name));
            }
        }
        Ok(Value::Object(output))
    }
    pub fn descriptor(&self, name: &str) -> Option<&PropertyDescriptor> {
        self.properties.iter().find(|p| p.name == name)
    }
}
/// Pure, application-supplied definition. Geometry is regenerated from resolved
/// properties before every committed change affecting a registered custom kind.
/// Definitions must be deterministic: equal resolved properties produce equal
/// geometry. Preparation, validation, and final commit may invoke a definition
/// multiple times for the same input; invocation count is not a lifecycle event.
/// No GPUI callback or network/resource operation belongs in this interface.
pub trait ElementDefinition: Send + Sync + 'static {
    fn kind(&self) -> &str;
    fn version(&self) -> u32 {
        1
    }
    fn schema(&self) -> PropertySchema;
    fn shape(&self, properties: &Value) -> Result<Shape, String>;
    /// Optional intrinsic measurement. Bounds must equal generated geometry.
    fn measure(&self, _properties: &Value) -> Result<Option<CustomMeasurement>, String> {
        Ok(None)
    }
    fn resize_policy(&self) -> CustomResizePolicy {
        CustomResizePolicy::Disabled
    }
    /// Return new serializable properties; geometry is regenerated and checked.
    fn resize(&self, _properties: &Value, _size: Size) -> Result<Value, String> {
        Err("custom intrinsic resize is unsupported".into())
    }
    /// Opt into the host's asynchronous producer for this kind. This pure key
    /// grants no filesystem/network access. None means no resource is required.
    fn resource_key(&self, _properties: &Value) -> Result<Option<String>, String> {
        Ok(None)
    }
    /// Optional precise custom picking. None uses generated fallback geometry.
    /// Implementations must remain inside the geometry's declared bounds.
    fn hit_test(&self, _properties: &Value, _point: Point, _tolerance: f32) -> Option<bool> {
        None
    }
}
/// Convenient real custom-element definition backed by pure geometry generation.
pub struct CustomDefinition<F> {
    kind: String,
    schema: PropertySchema,
    geometry: F,
}
impl<F> CustomDefinition<F> {
    pub fn new(kind: impl Into<String>, schema: PropertySchema, geometry: F) -> Self {
        Self {
            kind: kind.into(),
            schema,
            geometry,
        }
    }
}
impl<F> ElementDefinition for CustomDefinition<F>
where
    F: Fn(&Value) -> Result<(Bounds, Vec<Contour>), String> + Send + Sync + 'static,
{
    fn kind(&self) -> &str {
        &self.kind
    }
    fn schema(&self) -> PropertySchema {
        self.schema.clone()
    }
    fn shape(&self, properties: &Value) -> Result<Shape, String> {
        let (bounds, contours) = (self.geometry)(properties)?;
        Ok(Custom::new(&self.kind, bounds, contours).into())
    }
}
#[derive(Clone)]
pub struct ElementRegistry {
    definitions: BTreeMap<String, Arc<dyn ElementDefinition>>,
}
impl Default for ElementRegistry {
    fn default() -> Self {
        Self::with_builtins()
    }
}
impl ElementRegistry {
    pub fn measure_custom(&self, custom: &Custom) -> Result<CustomMeasurement, EngineError> {
        self.custom_call(custom, |definition, properties| {
            let measured = definition
                .measure(properties)?
                .ok_or("custom measurement is unsupported")?;
            measured
                .validate(custom.bounds)
                .map_err(|e| e.to_string())?;
            Ok(measured)
        })
    }
    pub fn resize_custom(&self, custom: &Custom, size: Size) -> Result<Shape, EngineError> {
        let measured = self.measure_custom(custom)?;
        if !size.is_positive()
            || size.width < measured.minimum.width
            || size.height < measured.minimum.height
            || size.width > measured.maximum.width
            || size.height > measured.maximum.height
        {
            return Err(EngineError::Registry(
                "custom resize exceeds measured limits".into(),
            ));
        }
        let properties = self.custom_call(custom, |definition, properties| {
            match definition.resize_policy() {
                CustomResizePolicy::Disabled => return Err("custom resize is disabled".into()),
                CustomResizePolicy::Uniform
                    if size.width / measured.bounds.size.width
                        != size.height / measured.bounds.size.height =>
                {
                    return Err("custom resize requires uniform factors".into());
                }
                _ => {}
            }
            definition.resize(properties, size)
        })?;
        let shape = self
            .create(&custom.kind, "custom-resize-preview", properties)?
            .shape;
        let Shape::Custom(resized) = &shape else {
            return Err(EngineError::Registry(
                "custom resize changed element kind".into(),
            ));
        };
        let next = self.measure_custom(resized)?;
        if next.bounds.size != size {
            return Err(EngineError::Registry(
                "custom resize did not produce the requested dimensions".into(),
            ));
        }
        Ok(shape)
    }
    pub fn custom_resource_key(&self, custom: &Custom) -> Result<Option<String>, EngineError> {
        if self.get(&custom.kind).is_none() {
            return Ok(None);
        }
        let key = self.custom_call(custom, |definition, properties| {
            definition.resource_key(properties)
        })?;
        if key
            .as_ref()
            .is_some_and(|key| key.is_empty() || key.len() > 4096)
        {
            return Err(EngineError::Registry(
                "custom resource key must contain 1..=4096 bytes".into(),
            ));
        }
        Ok(key)
    }
    fn custom_call<T>(
        &self,
        custom: &Custom,
        call: impl FnOnce(&dyn ElementDefinition, &Value) -> Result<T, String>,
    ) -> Result<T, EngineError> {
        let definition = self
            .get(&custom.kind)
            .ok_or_else(|| EngineError::Registry("custom kind is not registered".into()))?;
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let properties = definition.schema().resolve(&custom.data)?;
            call(definition, &properties)
        }))
        .map_err(|_| EngineError::Registry("custom protocol callback panicked".into()))?
        .map_err(EngineError::Registry)
    }
    pub fn empty() -> Self {
        Self {
            definitions: BTreeMap::new(),
        }
    }
    pub fn with_builtins() -> Self {
        let mut registry = Self::empty();
        for kind in [
            "rect",
            "ellipse",
            "line",
            "polygon",
            "star",
            "path",
            "pen",
            "text",
            "image",
            "group",
            "box",
            "frame",
            "canvas",
            "pen_group",
        ] {
            registry
                .definitions
                .insert(kind.into(), Arc::new(BuiltinDefinition(kind)));
        }
        registry
    }
    pub fn register(&mut self, definition: impl ElementDefinition) -> Result<(), EngineError> {
        let kind = definition.kind().trim();
        if kind.is_empty() || self.definitions.contains_key(kind) {
            return Err(EngineError::Registry(format!(
                "empty or duplicate kind {kind}"
            )));
        }
        let schema = definition.schema();
        let mut names = std::collections::BTreeSet::new();
        for p in &schema.properties {
            if p.name.is_empty() || p.name == "$version" || !names.insert(&p.name) {
                return Err(EngineError::Registry(
                    "invalid or duplicate property name".into(),
                ));
            }
            if p.default.as_ref().is_some_and(|v| !p.value_type.accepts(v)) {
                return Err(EngineError::Registry(format!("invalid default {}", p.name)));
            }
        }
        self.definitions
            .insert(kind.to_owned(), Arc::new(definition));
        Ok(())
    }
    pub fn get(&self, kind: &str) -> Option<&dyn ElementDefinition> {
        self.definitions.get(kind).map(|d| d.as_ref())
    }
    pub fn kinds(&self) -> impl Iterator<Item = &str> {
        self.definitions.keys().map(String::as_str)
    }
    pub fn create(
        &self,
        kind: &str,
        id: impl Into<DisplayId>,
        properties: Value,
    ) -> Result<DisplayObject, EngineError> {
        let d = self
            .get(kind)
            .ok_or_else(|| EngineError::Registry(format!("unknown kind {kind}")))?;
        let mut shape = self.build(d, &properties)?;
        if !matches!(shape, Shape::Custom(_)) && shape.tag() != kind {
            return Err(EngineError::Registry(
                "definition must preserve its registered kind".into(),
            ));
        }
        if let Shape::Custom(custom) = &mut shape {
            custom.kind = kind.to_owned();
            custom.data = properties;
        }
        let object = DisplayObject::new(id, shape);
        object.validate().map_err(EngineError::Registry)?;
        Ok(object)
    }
    fn build(&self, d: &dyn ElementDefinition, input: &Value) -> Result<Shape, EngineError> {
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.build_checked(d, input)
        }))
        .map_err(|_| {
            EngineError::Registry("element definition panicked; candidate discarded".into())
        })?
    }
    fn build_checked(
        &self,
        d: &dyn ElementDefinition,
        input: &Value,
    ) -> Result<Shape, EngineError> {
        if let Some(v) = input.get("$version")
            && v.as_u64() != Some(d.version() as u64)
        {
            return Err(EngineError::Registry(format!(
                "unsupported schema version for {}",
                d.kind()
            )));
        }
        let resolved = d.schema().resolve(input).map_err(EngineError::Registry)?;
        let shape = d.shape(&resolved).map_err(EngineError::Registry)?;
        if matches!(&shape,Shape::Custom(c) if c.kind!=d.kind()) {
            return Err(EngineError::Registry(
                "custom definition changed kind".into(),
            ));
        }
        shape.validate().map_err(EngineError::Registry)?;
        Ok(shape)
    }
    /// Unknown custom kinds remain inert validated fallback geometry. Creation by
    /// unknown factory name still errors; parsing never executes definitions.
    pub(crate) fn prepare_scene(&self, scene: &mut Scene) -> Result<(), EngineError> {
        self.prepare_changes(scene, None)
    }
    pub(crate) fn prepare_changes(
        &self,
        scene: &mut Scene,
        previous: Option<&Scene>,
    ) -> Result<(), EngineError> {
        let objects: Vec<_> = scene
            .nodes()
            .filter(|n| {
                previous
                    .and_then(|s| s.get(&n.object.id))
                    .is_none_or(|old| old.object.shape != n.object.shape)
            })
            .filter_map(|n| match &n.object.shape {
                Shape::Custom(c) => self
                    .get(&c.kind)
                    .map(|_| (n.object.id.clone(), c.kind.clone(), c.data.clone())),
                _ => None,
            })
            .collect();
        for (id, kind, props) in objects {
            let definition = self.get(&kind).expect("registered");
            let mut shape = self.build(definition, &props)?;
            let Shape::Custom(custom) = &mut shape else {
                return Err(EngineError::Registry(
                    "registered custom definition must produce Custom geometry".into(),
                ));
            };
            custom.data = props;
            if scene.get(&id).expect("existing").object.shape != shape {
                scene.update_object(&id, |o| o.shape = shape)?;
            }
        }
        scene.validate()?;
        Ok(())
    }
}
struct BuiltinDefinition(&'static str);
impl ElementDefinition for BuiltinDefinition {
    fn kind(&self) -> &str {
        self.0
    }
    fn schema(&self) -> PropertySchema {
        let defaults = match self.0 {
            "rect" => {
                json!({"size":{"width":100.,"height":100.},"corner_radius":0.,"corner_radii":null})
            }
            "ellipse" => {
                json!({"size":{"width":100.,"height":100.},"start_angle":0.,"end_angle":0.,"inner_radius":0.,"closed":true})
            }
            "line" => json!({"start":{"x":0.,"y":0.},"end":{"x":100.,"y":0.}}),
            "polygon" => {
                json!({"points":[{"x":0.,"y":0.},{"x":100.,"y":0.},{"x":50.,"y":100.}],"closed":true})
            }
            "star" => {
                json!({"center":{"x":50.,"y":50.},"outer_radius":50.,"inner_radius":25.,"points":5,"start_angle":-90.})
            }
            "path" => json!({"commands":[]}),
            "pen" => json!({"path":{"commands":[]}}),
            "text" => {
                json!({"content":"","size":{"width":100.,"height":30.},"layout_origin":{"x":0.,"y":0.},"font_family":"sans-serif","font_size":14.,"font_weight":400,"italic":false,"line_height":1.2,"text_case":"none","para_spacing":0.,"letter_spacing":0.,"word_spacing":0.,"para_indent":0.,"writing_mode":"x","auto_size_align":false,"align":"start","wrap":true,"padding":[0.,0.,0.,0.],"vertical_align":"top","overflow":"clip","decoration":"none"})
            }
            "image" => {
                json!({"source":"","size":{"width":100.,"height":100.},"natural_size":null,"fit":"contain","source_crop":null,"corner_radius":0.,"corner_radii":null})
            }
            "canvas" => {
                json!({"size":{"width":100.,"height":100.},"pixel_ratio":1.,"smooth":true,"safe_resize":false,"corner_radius":0.,"corner_radii":null})
            }
            "group" | "pen_group" => json!({}),
            "box" | "frame" => {
                json!({"size":{"width":100.,"height":100.},"corner_radius":0.,"corner_radii":null,"clip_children":self.0=="frame","paint_order":"before_children"})
            }
            _ => unreachable!("built-in definition kind"),
        };
        let properties = defaults
            .as_object()
            .expect("object defaults")
            .iter()
            .map(|(name, value)| {
                let value_type = match value {
                    Value::Number(_) => PropertyType::Number {
                        min: None,
                        max: None,
                    },
                    Value::String(_) => PropertyType::String,
                    Value::Bool(_) => PropertyType::Boolean,
                    Value::Object(_) => PropertyType::Object,
                    Value::Array(_) => PropertyType::Array,
                    _ => PropertyType::Any,
                };
                let required = self.0 == "image" && name == "source";
                PropertyDescriptor {
                    name: name.clone(),
                    value_type,
                    default: (!required).then(|| value.clone()),
                    required,
                    dirty: DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT,
                }
            })
            .collect();
        PropertySchema {
            properties,
            allow_unknown: false,
        }
    }
    fn shape(&self, properties: &Value) -> Result<Shape, String> {
        if self.0 == "pen_group" {
            return Ok(gpui_flow_scene::display::Group::pen().into());
        }
        serde_json::from_value(json!({"type":self.0,"geometry":properties}))
            .map_err(|e| e.to_string())
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PropertyKey {
    Name,
    Classes,
    Data,
    Transform,
    Visible,
    Locked,
    ZIndex,
    HitPolicy,
    HitChildren,
    Fill,
    Stroke,
    FillLayers,
    StrokeLayers,
    Opacity,
    Blur,
    BackgroundBlur,
    Grayscale,
    FillRule,
    BlendMode,
    Shadow,
    ShadowLayers,
    InnerShadowLayers,
    Mask,
    Eraser,
    Shape,
    /// Plain content of a built-in Text, preserving its layout box and typography.
    TextContent,
    /// Normalized source rectangle of a built-in Image; null restores full source.
    ImageSourceCrop,
    /// Normalized Path data, preserving whether the element is a Path or Pen.
    PathData,
    Custom(String),
}
impl PropertyKey {
    pub fn name(&self) -> String {
        match self {
            Self::Custom(name) => format!("custom.{name}"),
            Self::Name => "name".into(),
            Self::Classes => "classes".into(),
            Self::Data => "data".into(),
            Self::Transform => "transform".into(),
            Self::Visible => "visible".into(),
            Self::Locked => "locked".into(),
            Self::ZIndex => "z_index".into(),
            Self::HitPolicy => "hit_policy".into(),
            Self::HitChildren => "hit_children".into(),
            Self::Fill => "style.fill".into(),
            Self::Stroke => "style.stroke".into(),
            Self::FillLayers => "style.fill_layers".into(),
            Self::StrokeLayers => "style.stroke_layers".into(),
            Self::Opacity => "style.opacity".into(),
            Self::Blur => "style.blur".into(),
            Self::BackgroundBlur => "style.background_blur".into(),
            Self::Grayscale => "style.grayscale".into(),
            Self::FillRule => "style.fill_rule".into(),
            Self::BlendMode => "style.blend_mode".into(),
            Self::Shadow => "style.shadow".into(),
            Self::ShadowLayers => "style.shadow_layers".into(),
            Self::InnerShadowLayers => "style.inner_shadow_layers".into(),
            Self::Mask => "mask".into(),
            Self::Eraser => "eraser".into(),
            Self::Shape => "shape".into(),
            Self::TextContent => "text.content".into(),
            Self::ImageSourceCrop => "image.source_crop".into(),
            Self::PathData => "path.data".into(),
        }
    }
}
pub(crate) fn apply_property(
    object: &mut DisplayObject,
    key: &PropertyKey,
    value: Value,
) -> Result<(), EngineError> {
    if *key == PropertyKey::ImageSourceCrop {
        let Shape::Image(image) = &mut object.shape else {
            return Err(EngineError::Property(
                "source crop requires a built-in Image".into(),
            ));
        };
        let crop =
            serde_json::from_value(value).map_err(|e| EngineError::Property(e.to_string()))?;
        if let Some(crop) = crop {
            gpui_flow_scene::display::Image::validate_source_crop(crop)
                .map_err(EngineError::Property)?;
        }
        image.source_crop = crop;
        return Ok(());
    }
    if *key == PropertyKey::PathData {
        let target = match &mut object.shape {
            Shape::Path(path) => path,
            Shape::Pen(pen) => &mut pen.path,
            _ => {
                return Err(EngineError::Property(
                    "path data requires a built-in Path or Pen element".into(),
                ));
            }
        };
        let commands = value
            .get("commands")
            .and_then(Value::as_array)
            .ok_or_else(|| EngineError::Property("path data requires a commands array".into()))?;
        if commands.len() > 16_384 {
            return Err(EngineError::Property(
                "path data exceeds 16384 commands".into(),
            ));
        }
        let path: gpui_flow_scene::display::Path =
            serde_json::from_value(value).map_err(|e| EngineError::Property(e.to_string()))?;
        path.to_svg().map_err(EngineError::Property)?;
        *target = path;
        return Ok(());
    }
    if *key == PropertyKey::TextContent {
        let Shape::Text(text) = &mut object.shape else {
            return Err(EngineError::Property(
                "text content requires a built-in Text element".into(),
            ));
        };
        text.content = value
            .as_str()
            .ok_or_else(|| EngineError::Property("text content must be a string".into()))?
            .to_owned();
        return Ok(());
    }
    if let PropertyKey::Custom(name) = key {
        let Shape::Custom(custom) = &mut object.shape else {
            return Err(EngineError::Property(
                "custom properties require a Custom element".into(),
            ));
        };
        let data = custom
            .data
            .as_object_mut()
            .ok_or_else(|| EngineError::Property("custom properties must be an object".into()))?;
        data.insert(name.clone(), value);
        return Ok(());
    }
    let mut encoded =
        serde_json::to_value(&*object).map_err(|e| EngineError::Property(e.to_string()))?;
    let name = key.name();
    if let Some(field) = name.strip_prefix("style.") {
        encoded["style"][field] = value;
    } else {
        encoded[&name] = value;
    }
    *object = serde_json::from_value(encoded).map_err(|e| EngineError::Property(e.to_string()))?;
    Ok(())
}
pub(crate) fn property_dirty(name: &str) -> DirtyFlags {
    match name {
        "style.blur"
        | "style.background_blur"
        | "style.grayscale"
        | "style.blend_mode"
        | "style.shadow"
        | "style.shadow_layers"
        | "style.inner_shadow_layers" => DirtyFlags::PAINT,
        "mask" => DirtyFlags::PAINT | DirtyFlags::GEOMETRY | DirtyFlags::HIT,
        "eraser" => DirtyFlags::PAINT | DirtyFlags::GEOMETRY | DirtyFlags::HIT,
        "transform" => DirtyFlags::TRANSFORM | DirtyFlags::HIT,
        "shape" | "text.content" | "path.data" | "image.source_crop" => {
            DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT
        }
        "style.stroke" | "style.stroke_layers" | "style.fill_rule" => {
            DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT
        }
        "style.opacity" | "visible" => DirtyFlags::OPACITY | DirtyFlags::PAINT | DirtyFlags::HIT,
        "style.fill" | "style.fill_layers" => DirtyFlags::PAINT | DirtyFlags::HIT,
        "locked" | "hit_policy" | "hit_children" => DirtyFlags::HIT,
        "z_index" => DirtyFlags::ORDER | DirtyFlags::PAINT | DirtyFlags::HIT,
        _ => DirtyFlags::DATA,
    }
}

impl PropertyKey {
    /// Common-property schema is shared by setters, inspectors and invalidation.
    /// Custom-property descriptors are resolved against their element definition.
    pub fn descriptor(&self) -> Option<PropertyDescriptor> {
        let (value_type, default) = match self {
            Self::Name | Self::TextContent => (PropertyType::String, Some(json!(""))),
            Self::Classes
            | Self::FillLayers
            | Self::StrokeLayers
            | Self::ShadowLayers
            | Self::InnerShadowLayers => (PropertyType::Array, Some(json!([]))),
            Self::Visible | Self::HitChildren => (PropertyType::Boolean, Some(json!(true))),
            Self::Locked => (PropertyType::Boolean, Some(json!(false))),
            Self::Opacity => (
                PropertyType::Number {
                    min: Some(0.),
                    max: Some(1.),
                },
                Some(json!(1.)),
            ),
            Self::Blur | Self::BackgroundBlur => (
                PropertyType::Number {
                    min: Some(0.),
                    max: Some(256.),
                },
                Some(json!(0.)),
            ),
            Self::Grayscale => (
                PropertyType::Number {
                    min: Some(0.),
                    max: Some(1.),
                },
                Some(json!(0.)),
            ),
            Self::ZIndex => (
                PropertyType::Number {
                    min: Some(i32::MIN as f64),
                    max: Some(i32::MAX as f64),
                },
                Some(json!(0)),
            ),
            Self::Transform => (
                PropertyType::Object,
                Some(
                    serde_json::to_value(gpui_flow_scene::Affine::IDENTITY)
                        .expect("finite default"),
                ),
            ),
            Self::BlendMode => (PropertyType::String, Some(json!("pass_through"))),
            Self::Mask | Self::Eraser => (PropertyType::String, Some(json!("none"))),
            Self::Shadow | Self::ImageSourceCrop => (PropertyType::Any, Some(Value::Null)),
            Self::HitPolicy | Self::FillRule => (PropertyType::String, None),
            Self::Shape => (PropertyType::Object, None),
            Self::PathData => (PropertyType::Object, Some(json!({"commands": []}))),
            Self::Data | Self::Fill | Self::Stroke => (PropertyType::Any, None),
            Self::Custom(_) => return None,
        };
        let name = self.name();
        Some(PropertyDescriptor {
            name: name.clone(),
            value_type,
            default,
            required: false,
            dirty: property_dirty(&name),
        })
    }
}
