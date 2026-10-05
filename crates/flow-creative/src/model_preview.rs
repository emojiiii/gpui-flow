//! Bounded, CPU-only previews of real static GLB geometry.
//!
//! This is intentionally a small preview renderer, not a glTF viewer: only an
//! embedded BIN, triangle primitives, node transforms and opaque base colors are
//! supported. The default (or first) scene is flattened with instance transforms.
//! Textures, vertex colors, authored normals and PBR lighting are not reproduced;
//! callers must display `PreviewMesh::warnings()` alongside the preview.
//! Parsing never opens a path, fetches a URI, or decodes an image. Run parsing and
//! rendering off the UI thread, and discard stale results in the calling host.

use gltf::{
    accessor::{DataType, Dimensions},
    mesh::{Mode, Semantic},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    f64::consts::{FRAC_PI_2, PI, TAU},
    fmt,
};

pub const MAX_GLB_BYTES: usize = 32 * 1024 * 1024;
pub const MAX_PREVIEW_VERTICES: usize = 100_000;
pub const MAX_PREVIEW_TRIANGLES: usize = 50_000;
pub const MAX_PREVIEW_DIMENSION: u32 = 512;
const MAX_JSON_BYTES: usize = 4 * 1024 * 1024;
const MAX_NODES: usize = 1_024;
const MAX_NODE_DEPTH: usize = 64;
const MAX_INSTANCES: usize = 256;
const MAX_PRIMITIVES: usize = 2_048;
const MAX_ACCESSORS: usize = 4_096;
const MAX_JSON_VALUES: usize = 200_000;
const MAX_COORDINATE: f64 = 1.0e12;
// A triangle-count limit alone does not bound pathological overdraw. The sum of
// clipped triangle bounding-box areas is charged before any pixel loop starts.
const MAX_RASTER_SAMPLES: usize = 16_000_000;
const BACKGROUND: [u8; 4] = [23, 27, 35, 255];
const SIMPLIFIED_WARNING: &str = "Simplified static preview: opaque baseColorFactor with two-sided flat lighting; textures, PBR, vertex colors, authored normals and scene cameras are not reproduced.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PreviewError {
    Invalid(String),
    Unsupported(String),
    BudgetExceeded(&'static str),
}

impl fmt::Display for PreviewError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Invalid(message) => write!(f, "Invalid GLB preview: {message}"),
            Self::Unsupported(message) => write!(f, "Unsupported GLB preview: {message}"),
            Self::BudgetExceeded(limit) => write!(f, "GLB preview exceeds {limit}"),
        }
    }
}
impl std::error::Error for PreviewError {}

type Result<T> = std::result::Result<T, PreviewError>;

fn invalid(message: impl Into<String>) -> PreviewError {
    PreviewError::Invalid(message.into())
}
fn unsupported(message: impl Into<String>) -> PreviewError {
    PreviewError::Unsupported(message.into())
}

#[derive(Clone, Debug)]
struct Triangle {
    indices: [usize; 3],
    color: [f32; 3],
}

/// Validated, flattened geometry. Private storage preserves parser invariants.
#[derive(Clone, Debug)]
pub struct PreviewMesh {
    vertices: Vec<Vec3>,
    triangles: Vec<Triangle>,
    min: Vec3,
    max: Vec3,
    warnings: Vec<String>,
}

impl PreviewMesh {
    /// Includes vertices duplicated by primitive references and node instances.
    pub fn vertex_count(&self) -> usize {
        self.vertices.len()
    }
    pub fn triangle_count(&self) -> usize {
        self.triangles.len()
    }
    /// World-space, axis-aligned minimum and maximum, after node transforms.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        (self.min.as_f32(), self.max.as_f32())
    }
    pub fn warnings(&self) -> &[String] {
        &self.warnings
    }
}

/// A complete, opaque, row-major RGBA8 image. Origin is the top-left pixel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreviewImage {
    pub width: u32,
    pub height: u32,
    pub rgba8: Vec<u8>,
}

impl PreviewImage {
    /// Convert channel order for hosts expecting BGRA8. Alpha is always 255.
    pub fn into_bgra8(mut self) -> Vec<u8> {
        for pixel in self.rgba8.as_chunks_mut::<4>().0 {
            pixel.swap(0, 2);
        }
        self.rgba8
    }
}

/// Orbit controls in radians; call `fit` after loading a different mesh.
/// Camera state is private so invalid UI input cannot poison a future render.
#[derive(Clone, Debug, PartialEq)]
pub struct OrbitCamera {
    target: Vec3,
    radius: f64,
    distance: f64,
    yaw: f64,
    pitch: f64,
}

/// Portable view controls only, never a camera matrix or world-space bounds.
/// Deserialization alone does not validate these values: apply them exclusively
/// through `OrbitCamera::from_state` when restoring a document.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrbitCameraState {
    pub yaw_radians: f32,
    pub pitch_radians: f32,
    /// Camera distance divided by the current model's bounding-sphere radius.
    pub distance_factor: f32,
}

impl Default for OrbitCamera {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            radius: 1.0,
            distance: 3.2,
            yaw: 0.45,
            pitch: 0.25,
        }
    }
}

impl OrbitCamera {
    pub fn state(&self) -> OrbitCameraState {
        OrbitCameraState {
            yaw_radians: self.yaw as f32,
            pitch_radians: self.pitch as f32,
            distance_factor: (self.distance / self.radius) as f32,
        }
    }

    /// Restore bounded controls, recomputing center/scale from validated geometry.
    /// Invalid persisted values fail rather than silently changing the document.
    pub fn from_state(mesh: &PreviewMesh, state: OrbitCameraState) -> Result<Self> {
        let max_pitch = (FRAC_PI_2 - 0.01) as f32;
        if !state.yaw_radians.is_finite()
            || state.yaw_radians.abs() > std::f32::consts::TAU
            || !state.pitch_radians.is_finite()
            || state.pitch_radians.abs() > max_pitch
            || !state.distance_factor.is_finite()
            || !(0.05..=100.0).contains(&state.distance_factor)
        {
            return Err(invalid(
                "saved orbit camera controls are outside finite bounds",
            ));
        }
        let mut camera = Self::fit(mesh);
        camera.yaw = f64::from(state.yaw_radians).rem_euclid(TAU);
        camera.pitch = f64::from(state.pitch_radians).clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
        camera.distance = camera.radius * f64::from(state.distance_factor);
        Ok(camera)
    }

    /// Frame the whole world-space bounding sphere, including in portrait views.
    /// The 45-degree field of view is measured across the smaller image dimension.
    pub fn fit(mesh: &PreviewMesh) -> Self {
        let radius = (mesh.max.sub(mesh.min).length() * 0.5).max(1.0e-6);
        Self {
            target: mesh.min.add(mesh.max).scale(0.5),
            radius,
            distance: radius * 3.2,
            ..Self::default()
        }
    }

    /// Positive yaw moves the eye toward +X; pitch is clamped away from the poles.
    /// Non-finite deltas are ignored.
    pub fn orbit(&mut self, delta_yaw_radians: f32, delta_pitch_radians: f32) {
        if delta_yaw_radians.is_finite() {
            self.yaw = (self.yaw + f64::from(delta_yaw_radians)).rem_euclid(TAU);
        }
        if delta_pitch_radians.is_finite() {
            self.pitch = (self.pitch + f64::from(delta_pitch_radians))
                .clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
        }
    }

    /// Multiplicative distance: values below 1 zoom in, above 1 zoom out.
    /// Non-positive/non-finite factors are ignored; distance is bounded.
    pub fn zoom(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.distance =
                (self.distance * f64::from(factor)).clamp(self.radius * 0.05, self.radius * 100.0);
        }
    }
}

/// Parse a GLB 2.0 with exactly JSON + BIN chunks. Uses gltf 1.4.1's mature
/// container/schema parser and accessor readers; additional checks enforce the
/// preview's stricter resource and feature limits before readers are invoked.
pub fn parse_glb(bytes: &[u8]) -> Result<PreviewMesh> {
    if bytes.len() > MAX_GLB_BYTES {
        return Err(PreviewError::BudgetExceeded("32 MiB file limit"));
    }
    if bytes.len() < 12 || !bytes.starts_with(b"glTF") {
        return Err(invalid("expected a GLB 2.0 container"));
    }
    // gltf 1.4.1 subtracts 12 from this header length internally. Reject an
    // undersized header before that subtraction, and reject trailing bytes.
    let declared =
        u32::from_le_bytes(bytes[8..12].try_into().map_err(|_| invalid("header"))?) as usize;
    if declared != bytes.len() {
        return Err(invalid("GLB header length does not match file length"));
    }
    let glb = gltf::binary::Glb::from_slice(bytes).map_err(|error| invalid(error.to_string()))?;
    let bin = glb
        .bin
        .as_deref()
        .ok_or_else(|| unsupported("an embedded BIN chunk is required"))?;
    if glb.json.len() > MAX_JSON_BYTES {
        return Err(PreviewError::BudgetExceeded("4 MiB JSON limit"));
    }
    if !glb.json.len().is_multiple_of(4)
        || !bin.len().is_multiple_of(4)
        || 28 + glb.json.len() + bin.len() != bytes.len()
    {
        return Err(invalid("expected exactly aligned JSON and BIN chunks"));
    }
    let root: Value =
        serde_json::from_slice(&glb.json).map_err(|error| invalid(error.to_string()))?;
    preflight_json(&root)?;
    let json: gltf::json::Root =
        serde_json::from_value(root).map_err(|error| invalid(error.to_string()))?;
    let document = gltf::Document::from_json(json).map_err(|error| invalid(error.to_string()))?;
    validate_document(&document, bin)?;
    let scene = document
        .default_scene()
        .or_else(|| document.scenes().next())
        .ok_or_else(|| unsupported("a scene containing mesh nodes is required"))?;
    let mut mesh = PreviewMesh {
        vertices: Vec::new(),
        triangles: Vec::new(),
        min: Vec3::splat(f64::INFINITY),
        max: Vec3::splat(f64::NEG_INFINITY),
        warnings: vec![SIMPLIFIED_WARNING.into()],
    };
    if document.scenes().count() > 1 {
        mesh.warnings
            .push("Only the default (or first) scene is previewed.".into());
    }
    let mut visited = vec![false; document.nodes().count()];
    for node in scene.nodes() {
        append_node(node, Mat4::IDENTITY, bin, &mut mesh, &mut visited, 1)?;
    }
    if mesh.triangles.is_empty() {
        return Err(invalid("selected scene has no triangles"));
    }
    Ok(mesh)
}

fn preflight_json(root: &Value) -> Result<()> {
    if !root.is_object() {
        return Err(invalid("JSON root must be an object"));
    }
    if root.pointer("/asset/version").and_then(Value::as_str) != Some("2.0")
        || root
            .pointer("/asset/minVersion")
            .is_some_and(|value| value.as_str() != Some("2.0"))
    {
        return Err(unsupported("only glTF asset version 2.0 is supported"));
    }
    for (key, maximum) in [
        ("nodes", MAX_NODES),
        ("meshes", MAX_INSTANCES),
        ("accessors", MAX_ACCESSORS),
        ("bufferViews", MAX_ACCESSORS),
        ("materials", MAX_PRIMITIVES),
        ("scenes", MAX_NODES),
        ("images", MAX_PRIMITIVES),
        ("textures", MAX_PRIMITIVES),
        ("samplers", MAX_PRIMITIVES),
        ("cameras", MAX_NODES),
        ("buffers", 1),
    ] {
        if let Some(array) = root.get(key).and_then(Value::as_array)
            && array.len() > maximum
        {
            return Err(PreviewError::BudgetExceeded("document object limit"));
        }
    }
    for key in [
        "animations",
        "skins",
        "extensionsRequired",
        "extensionsUsed",
    ] {
        if let Some(value) = root.get(key)
            && value.as_array().is_none_or(|array| !array.is_empty())
        {
            return Err(unsupported(format!("{key} are not supported")));
        }
    }
    if let Some(nodes) = root.get("nodes").and_then(Value::as_array) {
        for node in nodes {
            if node.get("matrix").is_some()
                && ["translation", "rotation", "scale"]
                    .iter()
                    .any(|key| node.get(key).is_some())
            {
                return Err(invalid("a node cannot specify both matrix and TRS"));
            }
        }
    }
    // Unknown extension payloads would otherwise be discarded with gltf's
    // minimal feature set, concealing compressed or modified geometry.
    let mut remaining = MAX_JSON_VALUES;
    inspect_json(root, 0, &mut remaining)
}

fn inspect_json(value: &Value, depth: usize, remaining: &mut usize) -> Result<()> {
    if depth > 64 {
        return Err(PreviewError::BudgetExceeded("JSON nesting limit"));
    }
    *remaining = remaining
        .checked_sub(1)
        .ok_or(PreviewError::BudgetExceeded("JSON value limit"))?;
    match value {
        Value::Object(object) => {
            for (key, child) in object {
                if key == "extensions" && child.as_object().is_none_or(|map| !map.is_empty()) {
                    return Err(unsupported(
                        "glTF extensions (including compressed geometry) are not supported",
                    ));
                }
                if key == "uri" {
                    return Err(unsupported(
                        "external and data URIs are not supported; use embedded BIN data",
                    ));
                }
                // Application extras cannot change geometry and are not interpreted.
                if key != "extras" {
                    inspect_json(child, depth + 1, remaining)?;
                }
            }
        }
        Value::Array(array) => {
            for child in array {
                inspect_json(child, depth + 1, remaining)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn validate_document(document: &gltf::Document, bin: &[u8]) -> Result<()> {
    let buffers: Vec<_> = document.buffers().collect();
    if buffers.len() != 1 || !matches!(buffers[0].source(), gltf::buffer::Source::Bin) {
        return Err(unsupported("exactly one embedded BIN buffer is required"));
    }
    let length = buffers[0].length();
    if length > bin.len() || bin.len() - length > 3 {
        return Err(invalid("BIN byteLength/padding mismatch"));
    }
    for view in document.views() {
        let end = view
            .offset()
            .checked_add(view.length())
            .ok_or_else(|| invalid("buffer view offset overflow"))?;
        if view.buffer().index() != 0 || end > length {
            return Err(invalid("buffer view is outside the embedded buffer"));
        }
        if let Some(stride) = view.stride()
            && (!(4..=252).contains(&stride) || stride % 4 != 0)
        {
            return Err(invalid("invalid buffer byteStride"));
        }
    }
    // Validate even unused accessors, before any gltf iterator reads a byte.
    for accessor in document.accessors() {
        validate_accessor(&accessor)?;
    }
    let mut unique_vertices = 0;
    let mut unique_triangles = 0;
    let mut primitives = 0;
    for mesh in document.meshes() {
        if mesh.weights().is_some() {
            return Err(unsupported("morph weights are not supported"));
        }
        for primitive in mesh.primitives() {
            primitives += 1;
            if primitives > MAX_PRIMITIVES {
                return Err(PreviewError::BudgetExceeded("primitive limit"));
            }
            if primitive.mode() != Mode::Triangles {
                return Err(unsupported("only TRIANGLES primitives are supported"));
            }
            if primitive.morph_targets().next().is_some() {
                return Err(unsupported("morph targets are not supported"));
            }
            let positions = primitive
                .get(&Semantic::Positions)
                .ok_or_else(|| invalid("POSITION is required"))?;
            if positions.data_type() != DataType::F32
                || positions.dimensions() != Dimensions::Vec3
                || positions.normalized()
            {
                return Err(unsupported(
                    "POSITION must be an unnormalized FLOAT VEC3 accessor",
                ));
            }
            if positions.offset() % 4 != 0
                || positions.view().is_none_or(|view| view.offset() % 4 != 0)
            {
                return Err(invalid("POSITION must be four-byte aligned"));
            }
            for (semantic, attribute) in primitive.attributes() {
                if matches!(semantic, Semantic::Joints(_) | Semantic::Weights(_)) {
                    return Err(unsupported("skinning attributes are not supported"));
                }
                if attribute.count() != positions.count() {
                    return Err(invalid("vertex attribute counts differ"));
                }
            }
            let count = if let Some(indices) = primitive.indices() {
                if !matches!(
                    indices.data_type(),
                    DataType::U8 | DataType::U16 | DataType::U32
                ) || indices.dimensions() != Dimensions::Scalar
                    || indices.normalized()
                    || indices.view().is_some_and(|view| view.stride().is_some())
                {
                    return Err(unsupported(
                        "indices must be tightly packed unsigned scalars",
                    ));
                }
                indices.count()
            } else {
                positions.count()
            };
            if count == 0 || count % 3 != 0 {
                return Err(invalid(
                    "TRIANGLES require a non-zero multiple of three indices/vertices",
                ));
            }
            add_budget(
                &mut unique_vertices,
                positions.count(),
                MAX_PREVIEW_VERTICES,
                "100,000 source vertices",
            )?;
            add_budget(
                &mut unique_triangles,
                count / 3,
                MAX_PREVIEW_TRIANGLES,
                "50,000 source triangles",
            )?;
            // Do not let inactive scenes hide malformed geometry. These readers
            // are invoked only after all accessor byte ranges have been checked.
            let reader = primitive.reader(|buffer| (buffer.index() == 0).then_some(bin));
            let points = reader
                .read_positions()
                .ok_or_else(|| invalid("unable to read POSITION data"))?;
            let mut read_count = 0;
            for point in points {
                if !Vec3::from_f32(point).valid() {
                    return Err(invalid(
                        "POSITION contains a non-finite or unbounded coordinate",
                    ));
                }
                read_count += 1;
            }
            if read_count != positions.count() {
                return Err(invalid("truncated POSITION accessor"));
            }
            if primitive.indices().is_some() {
                let indices = reader
                    .read_indices()
                    .ok_or_else(|| invalid("unable to read index data"))?
                    .into_u32();
                let mut read_count = 0;
                for index in indices {
                    if index as usize >= positions.count() {
                        return Err(invalid("triangle index exceeds POSITION count"));
                    }
                    read_count += 1;
                }
                if read_count != count {
                    return Err(invalid("truncated index accessor"));
                }
            }
            let material = primitive.material();
            if material.alpha_mode() != gltf::material::AlphaMode::Opaque {
                return Err(unsupported("alpha MASK/BLEND materials are not supported"));
            }
            if !material
                .pbr_metallic_roughness()
                .base_color_factor()
                .iter()
                .all(|x| x.is_finite() && (0.0..=1.0).contains(x))
            {
                return Err(invalid("baseColorFactor must be finite and within [0, 1]"));
            }
        }
    }
    let nodes: Vec<_> = document.nodes().collect();
    let mut parents = vec![0u8; nodes.len()];
    let mut instances = 0;
    for node in &nodes {
        if node.skin().is_some() || node.weights().is_some() {
            return Err(unsupported("skinning and morph weights are not supported"));
        }
        Mat4::from_gltf(node.transform())?;
        if node.mesh().is_some() {
            add_budget(&mut instances, 1, MAX_INSTANCES, "256 mesh instances")?;
        }
        for child in node.children() {
            let count = parents
                .get_mut(child.index())
                .ok_or_else(|| invalid("invalid child node"))?;
            *count += 1;
            if *count > 1 {
                return Err(invalid("a node may not have multiple parents"));
            }
        }
    }
    // glTF JSON validation does not guarantee an acyclic node graph. Validate
    // every node, including nodes outside the chosen scene, before flattening.
    let mut states = vec![0u8; nodes.len()];
    let mut heights = vec![0usize; nodes.len()];
    for node in &nodes {
        node_height(node.clone(), &mut states, &mut heights, 1)?;
    }
    Ok(())
}

fn validate_accessor(accessor: &gltf::Accessor<'_>) -> Result<()> {
    if accessor.sparse().is_some() {
        return Err(unsupported("sparse accessors are not supported"));
    }
    if accessor.count() == 0 || accessor.count() > MAX_PREVIEW_TRIANGLES * 3 {
        return Err(PreviewError::BudgetExceeded("accessor element limit"));
    }
    let view = accessor
        .view()
        .ok_or_else(|| invalid("accessor requires an embedded buffer view"))?;
    let component_size = match accessor.data_type() {
        DataType::I8 | DataType::U8 => 1,
        DataType::I16 | DataType::U16 => 2,
        DataType::U32 | DataType::F32 => 4,
    };
    let components = match accessor.dimensions() {
        Dimensions::Scalar => 1,
        Dimensions::Vec2 => 2,
        Dimensions::Vec3 => 3,
        Dimensions::Vec4 => 4,
        // Matrix accessors only matter to unsupported animation/skinning here.
        _ => return Err(unsupported("matrix accessors are not supported")),
    };
    let element_size = component_size * components;
    let stride = view.stride().unwrap_or(element_size);
    if stride < element_size
        || stride % component_size != 0
        || !accessor.offset().is_multiple_of(component_size)
        || view.offset() % component_size != 0
    {
        return Err(invalid("accessor size, alignment or stride is invalid"));
    }
    let end = (accessor.count() - 1)
        .checked_mul(stride)
        .and_then(|size| size.checked_add(element_size))
        .and_then(|size| accessor.offset().checked_add(size))
        .ok_or_else(|| invalid("accessor offset/count overflow"))?;
    if end > view.length() {
        return Err(invalid("accessor exceeds its buffer view"));
    }
    Ok(())
}

fn node_height(
    node: gltf::Node<'_>,
    states: &mut [u8],
    heights: &mut [usize],
    depth: usize,
) -> Result<usize> {
    if depth > MAX_NODE_DEPTH {
        return Err(PreviewError::BudgetExceeded("64-level node depth"));
    }
    let index = node.index();
    match states[index] {
        1 => return Err(invalid("node hierarchy contains a cycle")),
        2 => return Ok(heights[index]),
        _ => {}
    }
    states[index] = 1;
    let mut height = 1usize;
    for child in node.children() {
        height = height.max(1 + node_height(child, states, heights, depth + 1)?);
    }
    if height > MAX_NODE_DEPTH {
        return Err(PreviewError::BudgetExceeded("64-level node depth"));
    }
    states[index] = 2;
    heights[index] = height;
    Ok(height)
}

fn append_node(
    node: gltf::Node<'_>,
    parent: Mat4,
    bin: &[u8],
    output: &mut PreviewMesh,
    visited: &mut [bool],
    depth: usize,
) -> Result<()> {
    if depth > MAX_NODE_DEPTH {
        return Err(PreviewError::BudgetExceeded("64-level node depth"));
    }
    if std::mem::replace(&mut visited[node.index()], true) {
        return Err(invalid("selected scene references a node more than once"));
    }
    let world = parent.mul(Mat4::from_gltf(node.transform())?)?;
    if let Some(mesh) = node.mesh() {
        for primitive in mesh.primitives() {
            let positions = primitive
                .get(&Semantic::Positions)
                .ok_or_else(|| invalid("missing positions"))?;
            let count = positions.count();
            let mut total = output.vertices.len();
            add_budget(
                &mut total,
                count,
                MAX_PREVIEW_VERTICES,
                "100,000 instanced vertices",
            )?;
            let index_count = primitive
                .indices()
                .map_or(count, |accessor| accessor.count());
            let mut triangles = output.triangles.len();
            add_budget(
                &mut triangles,
                index_count / 3,
                MAX_PREVIEW_TRIANGLES,
                "50,000 instanced triangles",
            )?;
            let reader = primitive.reader(|buffer| (buffer.index() == 0).then_some(bin));
            let positions = reader
                .read_positions()
                .ok_or_else(|| invalid("unable to read POSITION data"))?;
            let start = output.vertices.len();
            for position in positions {
                let point = world.point(Vec3::from_f32(position))?;
                output.min = output.min.min(point);
                output.max = output.max.max(point);
                output.vertices.push(point);
            }
            if output.vertices.len() != total {
                return Err(invalid("truncated POSITION accessor"));
            }
            let indices: Vec<u32> = if primitive.indices().is_some() {
                reader
                    .read_indices()
                    .ok_or_else(|| invalid("unable to read index data"))?
                    .into_u32()
                    .collect()
            } else {
                (0..count as u32).collect()
            };
            if indices.len() != index_count {
                return Err(invalid("truncated index accessor"));
            }
            if indices.iter().any(|&index| index as usize >= count) {
                return Err(invalid("triangle index exceeds POSITION count"));
            }
            let [r, g, b, _] = primitive
                .material()
                .pbr_metallic_roughness()
                .base_color_factor();
            for triangle in indices.as_chunks::<3>().0 {
                output.triangles.push(Triangle {
                    indices: [
                        start + triangle[0] as usize,
                        start + triangle[1] as usize,
                        start + triangle[2] as usize,
                    ],
                    color: [r, g, b],
                });
            }
        }
    }
    for child in node.children() {
        append_node(child, world, bin, output, visited, depth + 1)?;
    }
    Ok(())
}

fn add_budget(value: &mut usize, amount: usize, maximum: usize, label: &'static str) -> Result<()> {
    *value = value
        .checked_add(amount)
        .filter(|total| *total <= maximum)
        .ok_or(PreviewError::BudgetExceeded(label))?;
    Ok(())
}

/// Render perspective-correct depth-tested triangles into an opaque RGBA8 image.
/// Triangles are clipped against all six frustum planes before projection.
/// An excessive overdraw workload returns an error, never a partial image.
pub fn render(
    mesh: &PreviewMesh,
    camera: &OrbitCamera,
    width: u32,
    height: u32,
) -> Result<PreviewImage> {
    if width == 0 || height == 0 || width > MAX_PREVIEW_DIMENSION || height > MAX_PREVIEW_DIMENSION
    {
        return Err(PreviewError::BudgetExceeded(
            "1..=512 pixel output dimensions",
        ));
    }
    let near = camera.radius * 0.005;
    let far = camera.distance + camera.radius * 4.0;
    let outward = Vec3(
        camera.yaw.sin() * camera.pitch.cos(),
        camera.pitch.sin(),
        camera.yaw.cos() * camera.pitch.cos(),
    );
    let eye = camera.target.add(outward.scale(camera.distance));
    let right = Vec3(camera.yaw.cos(), 0.0, -camera.yaw.sin());
    let up = outward.cross(right);
    let forward = outward.scale(-1.0);
    let to_camera = |point: Vec3| {
        let relative = point.sub(eye);
        Vec3(relative.dot(right), relative.dot(up), relative.dot(forward))
    };
    let vertices: Vec<_> = mesh.vertices.iter().copied().map(to_camera).collect();
    let focal = f64::from(width.min(height)) / (2.0 * (PI / 8.0).tan());
    let half_x = f64::from(width) * 0.5 / focal;
    let half_y = f64::from(height) * 0.5 / focal;
    let planes = [
        (Vec3(0.0, 0.0, 1.0), -near),
        (Vec3(0.0, 0.0, -1.0), far),
        (Vec3(1.0, 0.0, half_x), 0.0),
        (Vec3(-1.0, 0.0, half_x), 0.0),
        (Vec3(0.0, 1.0, half_y), 0.0),
        (Vec3(0.0, -1.0, half_y), 0.0),
    ];
    let pixels = width as usize * height as usize;
    let mut image = PreviewImage {
        width,
        height,
        rgba8: Vec::with_capacity(pixels * 4),
    };
    for _ in 0..pixels {
        image.rgba8.extend_from_slice(&BACKGROUND);
    }
    // Reciprocal view-space Z is linear in screen-space barycentric coordinates.
    // Larger reciprocal depth is nearer. No average-depth triangle sorting.
    let mut depth = vec![0.0; pixels];
    let mut samples = 0;
    let light = Vec3(-0.35, 0.65, -1.0).normalized();
    for triangle in &mesh.triangles {
        let points = triangle.indices.map(|index| vertices[index]);
        let normal = points[1].sub(points[0]).cross(points[2].sub(points[0]));
        if normal.length() <= f64::MIN_POSITIVE {
            continue;
        }
        let shade = 0.25 + 0.75 * normal.normalized().dot(light).abs();
        let color = [
            linear_to_srgb(f64::from(triangle.color[0]) * shade),
            linear_to_srgb(f64::from(triangle.color[1]) * shade),
            linear_to_srgb(f64::from(triangle.color[2]) * shade),
            255,
        ];
        let mut polygon = points.to_vec();
        for (normal, offset) in planes {
            polygon = clip_polygon(&polygon, normal, offset);
        }
        for index in 1..polygon.len().saturating_sub(1) {
            let projected =
                [polygon[0], polygon[index], polygon[index + 1]].map(|point| ScreenPoint {
                    x: f64::from(width) * 0.5 + point.0 * focal / point.2,
                    y: f64::from(height) * 0.5 - point.1 * focal / point.2,
                    inverse_z: point.2.recip(),
                });
            raster_triangle(projected, color, &mut image, &mut depth, &mut samples)?;
        }
    }
    Ok(image)
}

fn clip_polygon(input: &[Vec3], normal: Vec3, offset: f64) -> Vec<Vec3> {
    let mut output = Vec::with_capacity(input.len() + 1);
    let Some(&last) = input.last() else {
        return output;
    };
    let mut previous = last;
    let mut previous_distance = normal.dot(previous) + offset;
    for &current in input {
        let distance = normal.dot(current) + offset;
        if (distance >= 0.0) != (previous_distance >= 0.0) {
            let t = (previous_distance / (previous_distance - distance)).clamp(0.0, 1.0);
            output.push(previous.add(current.sub(previous).scale(t)));
        }
        if distance >= 0.0 {
            output.push(current);
        }
        previous = current;
        previous_distance = distance;
    }
    output
}

#[derive(Clone, Copy)]
struct ScreenPoint {
    x: f64,
    y: f64,
    inverse_z: f64,
}

fn edge(a: ScreenPoint, b: ScreenPoint, x: f64, y: f64) -> f64 {
    (b.x - a.x) * (y - a.y) - (b.y - a.y) * (x - a.x)
}

fn raster_triangle(
    points: [ScreenPoint; 3],
    color: [u8; 4],
    image: &mut PreviewImage,
    depth: &mut [f64],
    samples: &mut usize,
) -> Result<()> {
    if points.iter().any(|point| {
        !point.x.is_finite()
            || !point.y.is_finite()
            || !point.inverse_z.is_finite()
            || point.inverse_z <= 0.0
    }) {
        return Err(invalid("non-finite projection"));
    }
    let [a, b, c] = points;
    let area = edge(a, b, c.x, c.y);
    if area.abs() < 1.0e-9 {
        return Ok(());
    }
    let min_x =
        a.x.min(b.x)
            .min(c.x)
            .floor()
            .clamp(0.0, f64::from(image.width)) as usize;
    let max_x =
        a.x.max(b.x)
            .max(c.x)
            .ceil()
            .clamp(0.0, f64::from(image.width)) as usize;
    let min_y =
        a.y.min(b.y)
            .min(c.y)
            .floor()
            .clamp(0.0, f64::from(image.height)) as usize;
    let max_y =
        a.y.max(b.y)
            .max(c.y)
            .ceil()
            .clamp(0.0, f64::from(image.height)) as usize;
    add_budget(
        samples,
        (max_x - min_x) * (max_y - min_y),
        MAX_RASTER_SAMPLES,
        "16 million raster sample budget; try a smaller preview",
    )?;
    for y in min_y..max_y {
        for x in min_x..max_x {
            let px = x as f64 + 0.5;
            let py = y as f64 + 0.5;
            let weights = [
                edge(b, c, px, py) / area,
                edge(c, a, px, py) / area,
                edge(a, b, px, py) / area,
            ];
            if weights.iter().any(|weight| *weight < -1.0e-10) {
                continue;
            }
            let inverse_z =
                weights[0] * a.inverse_z + weights[1] * b.inverse_z + weights[2] * c.inverse_z;
            let index = y * image.width as usize + x;
            if inverse_z > depth[index] {
                depth[index] = inverse_z;
                image.rgba8[index * 4..index * 4 + 4].copy_from_slice(&color);
            }
        }
    }
    Ok(())
}

fn linear_to_srgb(value: f64) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let srgb = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round() as u8
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Vec3(f64, f64, f64);
impl Vec3 {
    const ZERO: Self = Self(0.0, 0.0, 0.0);
    fn splat(x: f64) -> Self {
        Self(x, x, x)
    }
    fn from_f32([x, y, z]: [f32; 3]) -> Self {
        Self(f64::from(x), f64::from(y), f64::from(z))
    }
    fn as_f32(self) -> [f32; 3] {
        [self.0 as f32, self.1 as f32, self.2 as f32]
    }
    fn add(self, rhs: Self) -> Self {
        Self(self.0 + rhs.0, self.1 + rhs.1, self.2 + rhs.2)
    }
    fn sub(self, rhs: Self) -> Self {
        Self(self.0 - rhs.0, self.1 - rhs.1, self.2 - rhs.2)
    }
    fn scale(self, scale: f64) -> Self {
        Self(self.0 * scale, self.1 * scale, self.2 * scale)
    }
    fn dot(self, rhs: Self) -> f64 {
        self.0 * rhs.0 + self.1 * rhs.1 + self.2 * rhs.2
    }
    fn cross(self, rhs: Self) -> Self {
        Self(
            self.1 * rhs.2 - self.2 * rhs.1,
            self.2 * rhs.0 - self.0 * rhs.2,
            self.0 * rhs.1 - self.1 * rhs.0,
        )
    }
    fn length(self) -> f64 {
        self.dot(self).sqrt()
    }
    fn normalized(self) -> Self {
        let length = self.length();
        if length > 0.0 {
            self.scale(length.recip())
        } else {
            Self::ZERO
        }
    }
    fn min(self, rhs: Self) -> Self {
        Self(self.0.min(rhs.0), self.1.min(rhs.1), self.2.min(rhs.2))
    }
    fn max(self, rhs: Self) -> Self {
        Self(self.0.max(rhs.0), self.1.max(rhs.1), self.2.max(rhs.2))
    }
    fn valid(self) -> bool {
        [self.0, self.1, self.2]
            .iter()
            .all(|x| x.is_finite() && x.abs() <= MAX_COORDINATE)
    }
}

/// Column-major, matching glTF; composed as parent * local.
#[derive(Clone, Copy)]
struct Mat4([[f64; 4]; 4]);
impl Mat4 {
    const IDENTITY: Self = Self([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);
    fn from_gltf(transform: gltf::scene::Transform) -> Result<Self> {
        if let gltf::scene::Transform::Decomposed {
            translation,
            rotation,
            scale,
        } = transform
        {
            if !translation
                .into_iter()
                .chain(rotation)
                .chain(scale)
                .all(|value| value.is_finite())
            {
                return Err(invalid("non-finite node transform"));
            }
            let norm: f64 = rotation.iter().map(|&value| f64::from(value).powi(2)).sum();
            if (norm - 1.0).abs() > 1.0e-3 {
                return Err(invalid("node quaternion must have unit length"));
            }
        }
        let matrix = Self(transform.matrix().map(|column| column.map(f64::from)));
        matrix.validate()?;
        Ok(matrix)
    }
    fn validate(self) -> Result<()> {
        if self
            .0
            .iter()
            .flatten()
            .any(|x| !x.is_finite() || x.abs() > MAX_COORDINATE)
            || self.0[0][3] != 0.0
            || self.0[1][3] != 0.0
            || self.0[2][3] != 0.0
            || self.0[3][3] != 1.0
        {
            return Err(invalid("node matrix must be finite, bounded and affine"));
        }
        Ok(())
    }
    fn mul(self, rhs: Self) -> Result<Self> {
        let mut result = [[0.0; 4]; 4];
        for (column, values) in result.iter_mut().enumerate() {
            for (row, value) in values.iter_mut().enumerate() {
                *value = (0..4).map(|k| self.0[k][row] * rhs.0[column][k]).sum();
            }
        }
        let result = Self(result);
        result.validate()?;
        Ok(result)
    }
    fn point(self, point: Vec3) -> Result<Vec3> {
        if !point.valid() {
            return Err(invalid(
                "POSITION contains a non-finite or unbounded coordinate",
            ));
        }
        let [x, y, z] = [0, 1, 2].map(|row| {
            self.0[0][row] * point.0
                + self.0[1][row] * point.1
                + self.0[2][row] * point.2
                + self.0[3][row]
        });
        let result = Vec3(x, y, z);
        if !result.valid() {
            return Err(invalid(
                "transformed POSITION exceeds finite coordinate bounds",
            ));
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // These are actual GLB 2.0 test containers: JSON describes real FLOAT
    // position bytes and unsigned indices in an embedded BIN. They are fixtures,
    // not generated-user-model substitutes or renderer-only placeholder shapes.
    struct Fixture {
        json: Value,
        bin: Vec<u8>,
    }
    impl Fixture {
        fn new() -> Self {
            Self {
                json: json!({
                    "asset": {"version": "2.0"}, "scene": 0,
                    "scenes": [{"nodes": []}], "nodes": [], "meshes": [],
                    "buffers": [{"byteLength": 0}], "bufferViews": [],
                    "accessors": [], "materials": []
                }),
                bin: vec![0xAA; 4],
            }
        }

        fn add(
            &mut self,
            points: &[[f32; 3]],
            indices: Option<&[u32]>,
            stride: usize,
            index_width: usize,
            color: [f32; 4],
        ) {
            assert!(stride >= 12 && stride.is_multiple_of(4));
            while !self.bin.len().is_multiple_of(4) {
                self.bin.push(0);
            }
            let offset = self.bin.len();
            // Non-zero bufferView and accessor offsets exercise both origins.
            self.bin.extend_from_slice(&[0xBB; 4]);
            for point in points {
                for component in point {
                    self.bin.extend_from_slice(&component.to_le_bytes());
                }
                self.bin.resize(self.bin.len() + stride - 12, 0xCC);
            }
            let views = self.json["bufferViews"].as_array_mut().unwrap();
            let position_view = views.len();
            let mut view =
                json!({"buffer": 0, "byteOffset": offset, "byteLength": self.bin.len() - offset});
            if stride != 12 {
                view["byteStride"] = json!(stride);
            }
            views.push(view);
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for point in points {
                for i in 0..3 {
                    min[i] = min[i].min(point[i]);
                    max[i] = max[i].max(point[i]);
                }
            }
            let accessors = self.json["accessors"].as_array_mut().unwrap();
            let positions = accessors.len();
            accessors.push(
                json!({"bufferView": position_view, "byteOffset": 4, "componentType": 5126,
                "count": points.len(), "type": "VEC3", "min": min, "max": max}),
            );
            let mut primitive = json!({"attributes": {"POSITION": positions}, "mode": 4});
            if let Some(indices) = indices {
                let offset = self.bin.len();
                for &index in indices {
                    match index_width {
                        1 => self.bin.push(index as u8),
                        2 => self.bin.extend_from_slice(&(index as u16).to_le_bytes()),
                        4 => self.bin.extend_from_slice(&index.to_le_bytes()),
                        _ => panic!("unsupported test index width"),
                    }
                }
                let views = self.json["bufferViews"].as_array_mut().unwrap();
                let indices_view = views.len();
                views.push(json!({"buffer": 0, "byteOffset": offset, "byteLength": indices.len() * index_width}));
                let accessors = self.json["accessors"].as_array_mut().unwrap();
                primitive["indices"] = json!(accessors.len());
                accessors.push(json!({"bufferView": indices_view, "componentType": match index_width {1 => 5121, 2 => 5123, _ => 5125},
                    "count": indices.len(), "type": "SCALAR"}));
            }
            let materials = self.json["materials"].as_array_mut().unwrap();
            primitive["material"] = json!(materials.len());
            materials.push(
                json!({"pbrMetallicRoughness": {"baseColorFactor": color}, "doubleSided": true}),
            );
            let meshes = self.json["meshes"].as_array_mut().unwrap();
            let mesh_index = meshes.len();
            meshes.push(json!({"primitives": [primitive]}));
            let nodes = self.json["nodes"].as_array_mut().unwrap();
            let node_index = nodes.len();
            nodes.push(json!({"mesh": mesh_index}));
            self.json["scenes"][0]["nodes"]
                .as_array_mut()
                .unwrap()
                .push(json!(node_index));
            self.json["buffers"][0]["byteLength"] = json!(self.bin.len());
        }

        fn bytes(&self) -> Vec<u8> {
            let mut json = serde_json::to_vec(&self.json).unwrap();
            while !json.len().is_multiple_of(4) {
                json.push(b' ');
            }
            let mut bin = self.bin.clone();
            while !bin.len().is_multiple_of(4) {
                bin.push(0);
            }
            let mut bytes = Vec::new();
            bytes.extend_from_slice(b"glTF");
            bytes.extend_from_slice(&2u32.to_le_bytes());
            bytes.extend_from_slice(&((28 + json.len() + bin.len()) as u32).to_le_bytes());
            bytes.extend_from_slice(&(json.len() as u32).to_le_bytes());
            bytes.extend_from_slice(b"JSON");
            bytes.extend_from_slice(&json);
            bytes.extend_from_slice(&(bin.len() as u32).to_le_bytes());
            bytes.extend_from_slice(b"BIN\0");
            bytes.extend_from_slice(&bin);
            bytes
        }
        fn parse(&self) -> Result<PreviewMesh> {
            parse_glb(&self.bytes())
        }
    }

    const TRIANGLE: [[f32; 3]; 3] = [[-1.0, -1.0, 0.0], [1.0, -1.0, 0.0], [0.0, 1.0, 0.0]];
    const ASYMMETRIC: [[f32; 3]; 4] = [
        [-1.0, -0.7, -0.35],
        [1.8, -0.7, -0.35],
        [-0.8, 1.4, -0.35],
        [-0.3, -0.2, 0.9],
    ];
    const TETRA_INDICES: [u32; 12] = [0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3];

    fn triangle_fixture() -> Fixture {
        let mut fixture = Fixture::new();
        fixture.add(&TRIANGLE, Some(&[0, 1, 2]), 12, 2, [0.9, 0.3, 0.1, 1.0]);
        fixture
    }
    fn frontal_camera() -> OrbitCamera {
        OrbitCamera {
            yaw: 0.0,
            pitch: 0.0,
            distance: 3.0,
            ..OrbitCamera::default()
        }
    }
    fn foreground(image: &PreviewImage) -> Vec<bool> {
        image
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .map(|pixel| pixel != BACKGROUND.as_slice())
            .collect()
    }
    fn pixel(image: &PreviewImage, x: usize, y: usize) -> &[u8] {
        let offset = (y * image.width as usize + x) * 4;
        &image.rgba8[offset..offset + 4]
    }

    #[test]
    fn parses_real_asymmetric_glb_and_orbit_changes_silhouette() {
        let mut fixture = Fixture::new();
        fixture.add(
            &ASYMMETRIC,
            Some(&TETRA_INDICES),
            16,
            2,
            [0.3, 0.8, 0.6, 1.0],
        );
        let mesh = fixture.parse().unwrap();
        assert_eq!(mesh.vertex_count(), 4);
        assert_eq!(mesh.triangle_count(), 4);
        assert_eq!(mesh.bounds(), ([-1.0, -0.7, -0.35], [1.8, 1.4, 0.9]));
        assert!(mesh.warnings()[0].contains("PBR"));
        let mut camera = OrbitCamera::fit(&mesh);
        let front = render(&mesh, &camera, 128, 128).unwrap();
        camera.orbit(FRAC_PI_2 as f32, 0.2);
        let side = render(&mesh, &camera, 128, 128).unwrap();
        let front_mask = foreground(&front);
        let side_mask = foreground(&side);
        assert!(front_mask.iter().filter(|&&set| set).count() > 100);
        assert!(side_mask.iter().filter(|&&set| set).count() > 100);
        assert!(
            front_mask
                .iter()
                .zip(&side_mask)
                .filter(|(a, b)| a != b)
                .count()
                > 100
        );
        assert_eq!(
            front,
            render(&mesh, &OrbitCamera::fit(&mesh), 128, 128).unwrap()
        );
    }

    #[test]
    fn indexed_strided_and_nonindexed_geometry_render_identically() {
        let mut indexed = Fixture::new();
        indexed.add(
            &ASYMMETRIC,
            Some(&TETRA_INDICES),
            20,
            2,
            [0.8, 0.5, 0.3, 1.0],
        );
        let expanded: Vec<_> = TETRA_INDICES
            .iter()
            .map(|&index| ASYMMETRIC[index as usize])
            .collect();
        let mut nonindexed = Fixture::new();
        nonindexed.add(&expanded, None, 12, 2, [0.8, 0.5, 0.3, 1.0]);
        let a = indexed.parse().unwrap();
        let b = nonindexed.parse().unwrap();
        assert_eq!(a.triangle_count(), b.triangle_count());
        let camera = OrbitCamera::fit(&a);
        assert_eq!(
            render(&a, &camera, 96, 80).unwrap(),
            render(&b, &camera, 96, 80).unwrap()
        );
    }

    #[test]
    fn supports_all_unsigned_index_widths() {
        let mut reference = None;
        for width in [1, 2, 4] {
            let mut fixture = Fixture::new();
            fixture.add(&TRIANGLE, Some(&[0, 1, 2]), 16, width, [1.0; 4]);
            let mesh = fixture.parse().unwrap();
            let image = render(&mesh, &frontal_camera(), 64, 64).unwrap();
            if let Some(reference) = &reference {
                assert_eq!(reference, &image);
            }
            reference = Some(image);
        }
    }

    #[test]
    fn applies_parent_matrix_child_trs_and_mesh_instances() {
        let mut fixture = Fixture::new();
        fixture.add(
            &[[0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]],
            None,
            16,
            2,
            [1.0; 4],
        );
        fixture.json["nodes"] = json!([
            {"matrix": [1,0,0,0, 0,1,0,0, 0,0,1,0, 10,20,30,1], "children": [1]},
            {"translation": [1,2,3], "scale": [2,3,4], "mesh": 0},
            {"translation": [-10,0,0], "mesh": 0}
        ]);
        fixture.json["scenes"][0]["nodes"] = json!([0, 2]);
        let mesh = fixture.parse().unwrap();
        assert_eq!(mesh.vertex_count(), 6);
        assert_eq!(mesh.triangle_count(), 2);
        assert_eq!(mesh.vertices[0], Vec3(11.0, 22.0, 33.0));
        assert_eq!(mesh.vertices[1], Vec3(13.0, 22.0, 33.0));
        assert_eq!(mesh.vertices[2], Vec3(11.0, 25.0, 33.0));
        assert_eq!(mesh.bounds(), ([-10.0, 0.0, 0.0], [13.0, 25.0, 33.0]));
        fixture.json["nodes"][1]["rotation"] = json!([
            0.0,
            0.0,
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2
        ]);
        let rotated = fixture.parse().unwrap();
        assert!((rotated.vertices[1].0 - 11.0).abs() < 1.0e-5);
        assert!((rotated.vertices[1].1 - 24.0).abs() < 1.0e-5);
        assert!((rotated.vertices[2].0 - 8.0).abs() < 1.0e-5);
    }

    #[test]
    fn z_buffer_keeps_front_color_independent_of_triangle_order() {
        let mut fixture = Fixture::new();
        // Both triangles have the same projected outline when the eye is at z=3.
        fixture.add(
            &TRIANGLE.map(|p| [p[0] * 2.0 / 3.0, p[1] * 2.0 / 3.0, 1.0]),
            None,
            12,
            2,
            [0.0, 1.0, 0.0, 1.0],
        );
        fixture.add(&TRIANGLE, None, 12, 2, [1.0, 0.0, 0.0, 1.0]);
        let mesh = fixture.parse().unwrap();
        let image = render(&mesh, &frontal_camera(), 96, 96).unwrap();
        assert!(pixel(&image, 48, 48)[1] > 100);
        assert_eq!(pixel(&image, 48, 48)[0], 0);
        let mut reversed = mesh.clone();
        reversed.triangles.reverse();
        assert_eq!(image, render(&reversed, &frontal_camera(), 96, 96).unwrap());
    }

    #[test]
    fn crossing_triangles_use_per_pixel_perspective_depth() {
        // The red face slopes from depth 1 on the left to depth 5 on the right;
        // green is at depth 2.8. Their projected vertices coincide. Any whole-
        // triangle painter ordering necessarily gets one side wrong.
        let slope = [[-0.5, -0.5, 2.0], [2.5, -2.5, -2.0], [0.0, 2.5, -2.0]];
        let flat = [[-1.4, -1.4, 0.2], [1.4, -1.4, 0.2], [0.0, 1.4, 0.2]];
        let mut fixture = Fixture::new();
        fixture.add(&slope, None, 12, 2, [1.0, 0.0, 0.0, 1.0]);
        fixture.add(&flat, None, 12, 2, [0.0, 1.0, 0.0, 1.0]);
        let mesh = fixture.parse().unwrap();
        let image = render(&mesh, &frontal_camera(), 100, 100).unwrap();
        assert!(
            pixel(&image, 20, 75)[0] > 100,
            "left must show the red face"
        );
        assert_eq!(pixel(&image, 20, 75)[1], 0);
        assert!(
            pixel(&image, 80, 75)[1] > 100,
            "right must show the green face"
        );
        assert_eq!(pixel(&image, 80, 75)[0], 0);
        let mut reversed = mesh.clone();
        reversed.triangles.reverse();
        assert_eq!(
            image,
            render(&reversed, &frontal_camera(), 100, 100).unwrap()
        );
    }

    #[test]
    fn clips_near_crossings_instead_of_dropping_entire_triangles() {
        let mut fixture = Fixture::new();
        fixture.add(
            &[[-0.5, -0.5, 2.0], [0.5, -0.5, 2.0], [0.0, 0.8, 3.5]],
            None,
            12,
            2,
            [0.3, 0.6, 1.0, 1.0],
        );
        let mesh = fixture.parse().unwrap();
        let image = render(&mesh, &frontal_camera(), 64, 64).unwrap();
        assert!(foreground(&image).iter().filter(|&&set| set).count() > 100);
        assert_eq!(image.rgba8.len(), 64 * 64 * 4);
        let polygon = clip_polygon(
            &[
                Vec3(-1.0, 0.0, 1.0),
                Vec3(1.0, 0.0, 1.0),
                Vec3(0.0, 1.0, -1.0),
            ],
            Vec3(0.0, 0.0, 1.0),
            -0.01,
        );
        assert_eq!(polygon.len(), 4);
        assert!(polygon.iter().all(|point| point.2 >= 0.01 - 1.0e-12));
        let mut behind = Fixture::new();
        behind.add(&TRIANGLE.map(|p| [p[0], p[1], 4.0]), None, 12, 2, [1.0; 4]);
        let hidden = render(&behind.parse().unwrap(), &frontal_camera(), 64, 64).unwrap();
        assert!(foreground(&hidden).iter().all(|set| !set));
    }

    #[test]
    fn camera_controls_and_image_conversion_are_bounded() {
        let mesh = triangle_fixture().parse().unwrap();
        let mut camera = OrbitCamera::fit(&mesh);
        let initial = camera.clone();
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            camera.orbit(value, value);
            camera.zoom(value);
        }
        camera.zoom(0.0);
        camera.zoom(-1.0);
        assert_eq!(camera, initial);
        camera.orbit(f32::MAX, f32::MAX);
        camera.zoom(f32::MAX);
        assert!(camera.yaw.is_finite() && camera.pitch < FRAC_PI_2);
        assert_eq!(camera.distance, camera.radius * 100.0);
        camera.zoom(f32::MIN_POSITIVE);
        assert_eq!(camera.distance, camera.radius * 0.05);
        render(&mesh, &camera, 1, 512).unwrap();
        let image = render(&mesh, &initial, 64, 64).unwrap();
        let bgra = image.clone().into_bgra8();
        for (rgba, bgra) in image
            .rgba8
            .as_chunks::<4>()
            .0
            .iter()
            .zip(bgra.as_chunks::<4>().0.iter())
        {
            assert_eq!(bgra, &[rgba[2], rgba[1], rgba[0], 255]);
        }
        for (width, height) in [(0, 1), (1, 0), (513, 512), (512, 513), (u32::MAX, 1)] {
            assert!(matches!(
                render(&mesh, &initial, width, height),
                Err(PreviewError::BudgetExceeded(_))
            ));
        }
    }

    #[test]
    fn persisted_camera_controls_roundtrip_and_reject_unbounded_state() {
        let mesh = triangle_fixture().parse().unwrap();
        let mut camera = OrbitCamera::fit(&mesh);
        camera.orbit(0.7, -0.3);
        camera.zoom(0.8);
        let json = serde_json::to_value(camera.state()).unwrap();
        let state: OrbitCameraState = serde_json::from_value(json).unwrap();
        let restored = OrbitCamera::from_state(&mesh, state).unwrap();
        assert_eq!(restored.state(), state);
        assert_eq!(restored.target, camera.target);
        assert_eq!(restored.radius, camera.radius);
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            assert!(
                OrbitCamera::from_state(
                    &mesh,
                    OrbitCameraState {
                        yaw_radians: value,
                        ..state
                    }
                )
                .is_err()
            );
            assert!(
                OrbitCamera::from_state(
                    &mesh,
                    OrbitCameraState {
                        pitch_radians: value,
                        ..state
                    }
                )
                .is_err()
            );
            assert!(
                OrbitCamera::from_state(
                    &mesh,
                    OrbitCameraState {
                        distance_factor: value,
                        ..state
                    }
                )
                .is_err()
            );
        }
        for value in [-1.0, 0.0, 0.049, 101.0] {
            assert!(
                OrbitCamera::from_state(
                    &mesh,
                    OrbitCameraState {
                        distance_factor: value,
                        ..state
                    }
                )
                .is_err()
            );
        }
        assert!(
            serde_json::from_value::<OrbitCameraState>(json!({
                "yaw_radians": 0, "pitch_radians": 0, "distance_factor": 3, "matrix": []
            }))
            .is_err()
        );
        camera.orbit(-100.0, f32::MAX);
        camera.zoom(f32::MAX);
        OrbitCamera::from_state(&mesh, camera.state()).unwrap();
        camera.zoom(f32::MIN_POSITIVE);
        OrbitCamera::from_state(&mesh, camera.state()).unwrap();
    }

    #[test]
    fn rejects_external_uris_even_on_unused_images_or_buffers() {
        for uri in [
            "https://example.invalid/model.bin",
            "../private.bin",
            "data:application/octet-stream;base64,AAAA",
        ] {
            let mut fixture = triangle_fixture();
            fixture.json["buffers"][0]["uri"] = json!(uri);
            assert!(matches!(fixture.parse(), Err(PreviewError::Unsupported(_))));
            let mut fixture = triangle_fixture();
            fixture.json["images"] = json!([{"uri": uri}]);
            assert!(matches!(fixture.parse(), Err(PreviewError::Unsupported(_))));
        }
    }

    #[test]
    fn rejects_extensions_compression_morph_skin_and_animation() {
        for key in ["extensionsUsed", "extensionsRequired"] {
            let mut fixture = triangle_fixture();
            fixture.json[key] = json!(["KHR_draco_mesh_compression"]);
            assert!(matches!(fixture.parse(), Err(PreviewError::Unsupported(_))));
        }
        let mut fixture = triangle_fixture();
        fixture.json["meshes"][0]["primitives"][0]["extensions"] =
            json!({"KHR_draco_mesh_compression": {"bufferView": 0}});
        assert!(matches!(fixture.parse(), Err(PreviewError::Unsupported(_))));
        for key in ["animations", "skins"] {
            let mut fixture = triangle_fixture();
            fixture.json[key] = json!([{}]);
            assert!(matches!(fixture.parse(), Err(PreviewError::Unsupported(_))));
        }
        let mut fixture = triangle_fixture();
        fixture.json["meshes"][0]["primitives"][0]["targets"] = json!([{"POSITION":0}]);
        assert!(fixture.parse().is_err());
        let mut fixture = triangle_fixture();
        fixture.json["meshes"][0]["weights"] = json!([0.0]);
        assert!(fixture.parse().is_err());
        let mut fixture = triangle_fixture();
        fixture.json["nodes"][0]["weights"] = json!([0.0]);
        assert!(fixture.parse().is_err());
    }

    #[test]
    fn rejects_transparent_materials_and_nontriangle_primitives() {
        for mode in ["MASK", "BLEND"] {
            let mut fixture = triangle_fixture();
            fixture.json["materials"][0]["alphaMode"] = json!(mode);
            assert!(matches!(fixture.parse(), Err(PreviewError::Unsupported(_))));
        }
        for mode in [0, 1, 2, 3, 5, 6] {
            let mut fixture = triangle_fixture();
            fixture.json["meshes"][0]["primitives"][0]["mode"] = json!(mode);
            assert!(fixture.parse().is_err());
        }
        let mut fixture = triangle_fixture();
        fixture.json["materials"][0]["pbrMetallicRoughness"]["baseColorFactor"] =
            json!([2.0, 0.0, 0.0, 1.0]);
        assert!(fixture.parse().is_err());
    }

    #[test]
    fn rejects_nonfinite_vertices_bad_indices_offsets_and_stride() {
        for value in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, f32::MAX] {
            let mut fixture = triangle_fixture();
            fixture.bin[8..12].copy_from_slice(&value.to_le_bytes());
            assert!(fixture.parse().is_err());
        }
        let mut fixture = Fixture::new();
        fixture.add(&TRIANGLE, Some(&[0, 1, 3]), 12, 2, [1.0; 4]);
        assert!(fixture.parse().is_err());
        for value in [1u64, u64::MAX, 1024] {
            let mut fixture = triangle_fixture();
            fixture.json["accessors"][0]["byteOffset"] = json!(value);
            assert!(fixture.parse().is_err());
            let mut fixture = triangle_fixture();
            fixture.json["bufferViews"][0]["byteOffset"] = json!(value);
            assert!(fixture.parse().is_err());
        }
        for stride in [4, 8, 13, 256, 1_000_000] {
            let mut fixture = triangle_fixture();
            fixture.json["bufferViews"][0]["byteStride"] = json!(stride);
            assert!(fixture.parse().is_err());
        }
        let mut fixture = triangle_fixture();
        fixture.json["accessors"][0]["count"] = json!(4);
        assert!(fixture.parse().is_err());
        let mut fixture = triangle_fixture();
        fixture.json["accessors"][0]["sparse"] = json!({
            "count": 1, "indices": {"bufferView": 1, "componentType": 5123}, "values": {"bufferView": 0}
        });
        assert!(fixture.parse().is_err());
        let mut fixture = triangle_fixture();
        fixture.json["buffers"][0]["byteLength"] = json!(fixture.bin.len() + 4);
        assert!(fixture.parse().is_err());
    }

    #[test]
    fn rejects_short_forged_trailing_and_truncated_containers_without_panicking() {
        for bytes in [&[][..], b"glTF", b"{}", b"glTF\x02\0\0\0\x01\0\0\0"] {
            assert!(parse_glb(bytes).is_err());
        }
        let valid = triangle_fixture().bytes();
        let mut missing_bin = valid.clone();
        let json_length = u32::from_le_bytes(missing_bin[12..16].try_into().unwrap()) as usize;
        missing_bin.truncate(20 + json_length);
        let length = missing_bin.len() as u32;
        missing_bin[8..12].copy_from_slice(&length.to_le_bytes());
        assert!(matches!(
            parse_glb(&missing_bin),
            Err(PreviewError::Unsupported(_))
        ));
        for end in 0..valid.len() {
            assert!(parse_glb(&valid[..end]).is_err());
        }
        let mut trailing = valid.clone();
        trailing.extend_from_slice(&[0; 4]);
        assert!(parse_glb(&trailing).is_err());
        let length = trailing.len() as u32;
        trailing[8..12].copy_from_slice(&length.to_le_bytes());
        assert!(parse_glb(&trailing).is_err());
        for value in [0u32, 1, 11, u32::MAX] {
            let mut bad = valid.clone();
            bad[8..12].copy_from_slice(&value.to_le_bytes());
            assert!(parse_glb(&bad).is_err());
        }
        let mut bad_chunk = valid;
        bad_chunk[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(parse_glb(&bad_chunk).is_err());
    }

    #[test]
    fn rejects_cycles_excessive_depth_duplicate_parents_and_bad_transforms() {
        let mut fixture = triangle_fixture();
        fixture.json["nodes"][0]["children"] = json!([0]);
        assert!(fixture.parse().is_err());
        let mut fixture = triangle_fixture();
        fixture.json["nodes"] = json!([{"children":[2]}, {"children":[2]}, {"mesh":0}]);
        fixture.json["scenes"][0]["nodes"] = json!([0, 1]);
        assert!(fixture.parse().is_err());
        let mut fixture = triangle_fixture();
        fixture.json["nodes"] = json!(
            (0..65)
                .map(|i| if i == 64 {
                    json!({"mesh":0})
                } else {
                    json!({"children":[i+1]})
                })
                .collect::<Vec<_>>()
        );
        assert!(matches!(
            fixture.parse(),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let mut fixture = triangle_fixture();
        fixture.json["nodes"][0]["rotation"] = json!([0, 0, 0, 0]);
        assert!(fixture.parse().is_err());
        let mut fixture = triangle_fixture();
        fixture.json["nodes"][0]["matrix"] =
            json!([1, 0, 0, 0.1, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]);
        assert!(fixture.parse().is_err());
        let mut fixture = triangle_fixture();
        fixture.json["nodes"][0]["scale"] = json!([1.0e30, 1, 1]);
        assert!(fixture.parse().is_err());
    }

    #[test]
    fn rejects_file_geometry_instance_and_raster_budgets() {
        assert!(matches!(
            parse_glb(&vec![0; MAX_GLB_BYTES + 1]),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let mut too_much_json = triangle_fixture();
        too_much_json.json["extras"] = json!("x".repeat(MAX_JSON_BYTES));
        assert!(matches!(
            too_much_json.parse(),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let mut fixture = triangle_fixture();
        fixture.json["accessors"][0]["count"] = json!(MAX_PREVIEW_TRIANGLES * 3 + 1);
        assert!(matches!(
            fixture.parse(),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let mut too_many_vertices = Fixture::new();
        too_many_vertices.add(
            &vec![[0.0; 3]; MAX_PREVIEW_VERTICES + 1],
            Some(&[0, 1, 2]),
            12,
            2,
            [1.0; 4],
        );
        assert!(matches!(
            too_many_vertices.parse(),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let mut too_many_triangles = Fixture::new();
        let indices: Vec<_> = (0..25_001).flat_map(|_| [0, 1, 2]).collect();
        for _ in 0..2 {
            too_many_triangles.add(&TRIANGLE, Some(&indices), 12, 2, [1.0; 4]);
        }
        assert!(matches!(
            too_many_triangles.parse(),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let mut fixture = triangle_fixture();
        fixture.json["nodes"] = json!(
            (0..=MAX_INSTANCES)
                .map(|_| json!({"mesh":0}))
                .collect::<Vec<_>>()
        );
        fixture.json["scenes"][0]["nodes"] = json!((0..=MAX_INSTANCES).collect::<Vec<_>>());
        assert!(matches!(
            fixture.parse(),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let mut expanded_vertices = Fixture::new();
        expanded_vertices.add(&vec![[0.0; 3]; 501], Some(&[0, 1, 2]), 12, 2, [1.0; 4]);
        expanded_vertices.json["nodes"] =
            json!((0..200).map(|_| json!({"mesh":0})).collect::<Vec<_>>());
        expanded_vertices.json["scenes"][0]["nodes"] = json!((0..200).collect::<Vec<_>>());
        assert!(matches!(
            expanded_vertices.parse(),
            Err(PreviewError::BudgetExceeded(_))
        ));
        // Source geometry is small, but repeated instances exceed the expanded
        // triangle budget. The budget is checked before allocating the instance.
        let indices: Vec<_> = (0..1_000).flat_map(|_| [0, 1, 2]).collect();
        let mut fixture = Fixture::new();
        fixture.add(&TRIANGLE, Some(&indices), 12, 2, [1.0; 4]);
        fixture.json["nodes"] = json!((0..51).map(|_| json!({"mesh":0})).collect::<Vec<_>>());
        fixture.json["scenes"][0]["nodes"] = json!((0..51).collect::<Vec<_>>());
        assert!(matches!(
            fixture.parse(),
            Err(PreviewError::BudgetExceeded(_))
        ));
        let mut mesh = triangle_fixture().parse().unwrap();
        mesh.triangles = vec![mesh.triangles[0].clone(); 1_000];
        assert!(matches!(
            render(&mesh, &frontal_camera(), 512, 512),
            Err(PreviewError::BudgetExceeded(_))
        ));
    }

    #[test]
    fn default_scene_selection_does_not_merge_unrelated_scenes() {
        let mut fixture = triangle_fixture();
        fixture.json["nodes"] = json!([{"mesh":0}, {"mesh":0,"translation":[10,0,0]}]);
        fixture.json["scenes"] = json!([{"nodes":[0]}, {"nodes":[1]}]);
        fixture.json["scene"] = json!(1);
        let selected = fixture.parse().unwrap();
        assert_eq!(selected.triangle_count(), 1);
        assert_eq!(selected.bounds().0[0], 9.0);
        assert_eq!(selected.warnings().len(), 2);
        fixture.json.as_object_mut().unwrap().remove("scene");
        assert_eq!(fixture.parse().unwrap().bounds().0[0], -1.0);
    }
}
