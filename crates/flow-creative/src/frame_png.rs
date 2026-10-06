//! Immutable static Frame exports through the existing Scene compositor.
//! Only manifest-owned PNG/JPEG copies are read, on the caller's worker thread.
//! This module never changes the canvas, loads a URI, or writes an output file.

use crate::frame_text::{FrameTextBackend, FrameTextSnapshot};
use crate::pencil::{PENCIL_NODE_KIND, PencilData};
use crate::project::{LocalProject, MAX_IMAGE_BYTES, ProjectAsset, validate_asset_key};
use crate::shapes::{SHAPE_NODE_KIND, ShapeData, ShapeKind};
use gpui_flow_composite::{
    CompositeError, CompositeLimits, ImageInputs, SceneExportFormat, SceneExportPlan,
    SceneExportRequest, StaticImage,
};
use gpui_flow_core::{
    Graph, GroupId, NodeId, Point, Rect, Selection, Size, hidden_node_ids, ordered_node_ids,
};
use gpui_flow_scene::{self as scene, Affine, Color, DisplayObject, Scene, Shape, Stroke, Style};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits};
use std::{
    collections::BTreeMap,
    fs::File,
    io::{self, BufRead, Cursor, Read, Seek, SeekFrom},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_OBJECTS: usize = 128;
const MAX_POINTS: usize = 65_536;
// Conservative admission allowance, not an allocator measurement: at most
// 65,536 normalized Points (<1 MiB), local Scene path commands (<2 MiB), and
// transient resolved Scene/owned-plan copies plus 128 bounded object records.
// Only the snapshot and owned plan survive plan(); the temporary Scene and its
// resolved copy are gone before decoding. Reserve 16 MiB throughout anyway.
// Compositor lowered path storage retains its separate max_path_bytes guard.
const HOST_RETAINED_BYTES: u64 = 16 * 1024 * 1024;
const CODEC_FIXED_BYTES: u64 = 8 * 1024 * 1024;

/// A validated, immutable source snapshot. Locks and hidden-member content are
/// intentionally absent: they do not change the exported picture. Visible
/// membership and order, Frame identity/paint, geometry and asset identities do.
#[derive(Debug, Clone, PartialEq)]
pub struct FramePngExport {
    frame_id: GroupId,
    title: String,
    bounds: Rect,
    white_background: bool,
    suggested_name: String,
    objects: Vec<FrameObject>,
}

#[derive(Debug, Clone, PartialEq)]
struct FrameObject {
    id: NodeId,
    title: String,
    position: Point,
    size: Size,
    primitive: Primitive,
}

#[derive(Debug, Clone, PartialEq)]
enum Primitive {
    Image(ProjectAsset),
    Shape(ShapeData),
    Pencil(PencilData),
    Text(FrameTextSnapshot),
}

impl FramePngExport {
    pub fn prepare(
        graph: &Graph,
        selection: &Selection,
        assets: &[ProjectAsset],
    ) -> Result<Self, String> {
        prepare_frame_png_export(graph, selection, assets)
    }

    pub fn suggested_name(&self) -> &str {
        &self.suggested_name
    }

    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    /// Physical output dimensions at scale 1 and pixel ratio 1. Fractional
    /// Frame edges keep their exact coverage inside these outward-ceiled axes.
    pub fn pixel_size(&self) -> [u32; 2] {
        [
            self.bounds.size.width.ceil() as u32,
            self.bounds.size.height.ceil() as u32,
        ]
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn has_text(&self) -> bool {
        self.objects
            .iter()
            .any(|object| matches!(object.primitive, Primitive::Text(_)))
    }

    pub fn has_white_background(&self) -> bool {
        self.white_background
    }

    pub fn matches(&self, graph: &Graph, selection: &Selection, assets: &[ProjectAsset]) -> bool {
        Self::prepare(graph, selection, assets).is_ok_and(|current| current == *self)
    }

    /// Run off the UI thread. The caller owns deadline cancellation and must
    /// recheck the live selection/resource epoch before publishing the bytes.
    /// Encoded sources and codec temporaries are released before PNG encoding;
    /// only deduplicated immutable premultiplied image inputs remain.
    pub fn encode(
        &self,
        project: &LocalProject,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, String> {
        self.encode_with_text_backend(project, None, cancelled)
    }

    pub fn encode_with_text_backend(
        &self,
        project: &LocalProject,
        backend: Option<&FrameTextBackend>,
        cancelled: &AtomicBool,
    ) -> Result<Vec<u8>, String> {
        check_active(cancelled)?;
        if self.has_text() && !backend.is_some_and(FrameTextBackend::current) {
            return Err(
                "Frame PNG text requires the current application's native font backend".into(),
            );
        }
        self.check_assets(project.assets())?;
        let plan = self.plan(backend)?;
        let (images, image_work) = self.load_images(project, cancelled)?;
        check_active(cancelled)?;
        let result = plan
            .encode_with_text(&images, HOST_RETAINED_BYTES, cancelled, |mut request| {
                request.limits.max_surface_writes = request
                    .limits
                    .max_surface_writes
                    .checked_sub(image_work)
                    .ok_or(CompositeError::WorkLimit)?;
                let mut text = gpui_flow::prepare_scene_export_overflow_text_inputs(
                    request,
                    backend.map(FrameTextBackend::backend),
                    cancelled,
                )?;
                text.pixel_work = text
                    .pixel_work
                    .checked_add(image_work)
                    .ok_or(CompositeError::WorkLimit)?;
                Ok(text)
            })
            .map_err(|error| error.to_string())?;
        check_active(cancelled)?;
        if backend.is_some_and(|backend| !backend.current()) {
            return Err("Frame PNG font snapshot changed; prepare the export again".into());
        }
        Ok(result.bytes)
    }

    fn check_assets(&self, assets: &[ProjectAsset]) -> Result<(), String> {
        for object in &self.objects {
            if let Primitive::Image(expected) = &object.primitive {
                let mut matches = assets.iter().filter(|asset| asset.key == expected.key);
                if matches.next() != Some(expected) || matches.next().is_some() {
                    return Err("Frame PNG image asset changed; prepare the export again".into());
                }
            }
        }
        Ok(())
    }

    fn plan(&self, backend: Option<&FrameTextBackend>) -> Result<SceneExportPlan, String> {
        let mut scene = Scene::new();
        for object in &self.objects {
            let mut display = if let Primitive::Text(text) = &object.primitive {
                text.display_object(object.id.as_str(), object.size, backend)?
            } else {
                object.display_object()
            };
            display.transform =
                Affine::translation(object.position.x, object.position.y) * display.transform;
            // Scene roots with equal z-index retain insertion order. Objects
            // arrive in the core's actual back-to-front ordered_node_ids order.
            scene
                .insert(display, None, None)
                .map_err(|error| error.to_string())?;
        }
        SceneExportPlan::new(
            &scene,
            SceneExportRequest {
                world_bounds: scene::Bounds::new(self.bounds.origin, self.bounds.size),
                scale: 1.,
                pixel_ratio: 1.,
                background: if self.white_background {
                    Color::WHITE
                } else {
                    Color::TRANSPARENT
                },
                format: SceneExportFormat::Png,
            },
            CompositeLimits::default(),
        )
        .map_err(|error| error.to_string())
    }

    fn load_images(
        &self,
        project: &LocalProject,
        cancelled: &AtomicBool,
    ) -> Result<(ImageInputs, u64), String> {
        let limits = CompositeLimits::default();
        let mut shared = BTreeMap::<&str, Arc<StaticImage>>::new();
        let mut images = ImageInputs::default();
        let mut pixel_work = 0u64;
        for object in &self.objects {
            check_active(cancelled)?;
            let Primitive::Image(asset) = &object.primitive else {
                continue;
            };
            let image = if let Some(image) = shared.get(asset.key.as_str()) {
                Arc::clone(image)
            } else {
                // Charge decoding and in-place premultiplication once per
                // unique source; compositor work remains independently charged.
                pixel_work = pixel_work
                    .checked_add(u64::from(asset.width) * u64::from(asset.height) * 2)
                    .ok_or("Frame PNG image work exceeds its budget")?;
                if pixel_work > limits.max_surface_writes {
                    return Err("Frame PNG image work exceeds its budget".into());
                }
                let retained = HOST_RETAINED_BYTES + images.byte_len();
                let image = Arc::new(decode_asset(project, asset, retained, cancelled)?);
                shared.insert(asset.key.as_str(), Arc::clone(&image));
                image
            };
            images.insert(object.id.as_str().into(), Ok(image));
        }
        Ok((images, pixel_work))
    }
}

/// Select exactly one Frame, with no direct nodes or edges. Every visible
/// member must be supported, even when outside the crop or fully transparent.
/// Missing members/resources fail the complete request; hidden content is skipped.
pub fn prepare_frame_png_export(
    graph: &Graph,
    selection: &Selection,
    assets: &[ProjectAsset],
) -> Result<FramePngExport, String> {
    if selection.groups.len() != 1 || !selection.nodes.is_empty() || !selection.edges.is_empty() {
        return Err("Select one Frame by itself to export PNG".into());
    }
    let id = selection.groups.iter().next().expect("one selected Frame");
    let frame = graph
        .groups
        .get(id)
        .ok_or("The selected Frame no longer exists")?;
    if frame.id != *id
        || id.as_str().trim().is_empty()
        || id.as_str().len() > 512
        || frame.title.len() > 512
    {
        return Err("Frame PNG requires a valid Frame identity and bounded title".into());
    }
    let bounds = frame.bounds;
    if !bounds.is_valid()
        || !bounds.size.is_positive()
        || bounds.max_x() <= bounds.min_x()
        || bounds.max_y() <= bounds.min_y()
        || [
            bounds.min_x(),
            bounds.min_y(),
            bounds.max_x(),
            bounds.max_y(),
        ]
        .iter()
        .any(|coordinate| coordinate.abs() > 1_000_000.)
    {
        return Err(
            "Frame PNG bounds must be finite, positive and inside the supported world".into(),
        );
    }
    check_dimensions(
        bounds.size.width.ceil() as u32,
        bounds.size.height.ceil() as u32,
    )?;
    let hidden = hidden_node_ids(graph);
    let mut visible_count = 0;
    for member in &frame.node_ids {
        if !graph.nodes.contains_key(member) {
            return Err("A Frame PNG member no longer exists".into());
        }
        if !hidden.contains(member) {
            visible_count += 1;
            if visible_count > MAX_OBJECTS {
                return Err("Frame PNG supports at most 128 visible objects".into());
            }
        }
    }
    if visible_count == 0 {
        return Err(
            "The selected Frame has no visible image, text, shape or pencil members to export"
                .into(),
        );
    }
    let mut objects = Vec::with_capacity(visible_count);
    let mut points = 0usize;
    let mut text_bytes = 0usize;
    for id in ordered_node_ids(graph) {
        if !frame.node_ids.contains(&id) || hidden.contains(&id) {
            continue;
        }
        let node = &graph.nodes[&id];
        if node.id != id
            || id.as_str().trim().is_empty()
            || id.as_str().len() > 512
            || node.title.len() > 512
            || !node.bounds().is_valid()
            || !node.size.is_positive()
        {
            return Err("Frame PNG member identity, title or geometry is invalid".into());
        }
        let primitive = match node.kind.as_str() {
            "image" => {
                let key = node.data["asset"].as_str().ok_or("Frame PNG image has no local asset")?;
                validate_asset_key(key).map_err(|error| error.to_string())?;
                let mut matches = assets.iter().filter(|asset| asset.key == key);
                let asset = matches.next().ok_or("Frame PNG image is missing from the project assets")?;
                if matches.next().is_some() {
                    return Err("Frame PNG image asset identity is ambiguous".into());
                }
                validate_asset(asset)?;
                Primitive::Image(asset.clone())
            }
            "text" => {
                let text = FrameTextSnapshot::from_node(node)?;
                text_bytes += text.content_bytes();
                if text_bytes > 256 * 1024 {
                    return Err("Frame PNG text exceeds the combined 256 KiB source budget".into());
                }
                // Admit padded geometry before offering a file destination.
                text.display_object(id.as_str(), node.size, None)?;
                Primitive::Text(text)
            }
            SHAPE_NODE_KIND => Primitive::Shape(ShapeData::from_node(node)?),
            PENCIL_NODE_KIND => {
                points = points.saturating_add(node.data["pencil"]["points"].as_array().map_or(0, Vec::len));
                if points > MAX_POINTS {
                    return Err("Frame PNG supports at most 65536 pencil points".into());
                }
                Primitive::Pencil(PencilData::from_node(node)?)
            }
            _ => return Err("Frame PNG supports local static PNG/JPEG images, text, shapes and pencil strokes; a visible member has an unsupported kind".into()),
        };
        objects.push(FrameObject {
            id,
            title: node.title.clone(),
            position: node.position,
            size: node.size,
            primitive,
        });
    }
    Ok(FramePngExport {
        frame_id: frame.id.clone(),
        title: frame.title.clone(),
        bounds,
        white_background: frame.data["flow_creative_frame"].as_u64() == Some(1),
        suggested_name: suggested_name(&frame.title),
        objects,
    })
}

impl FrameObject {
    fn display_object(&self) -> DisplayObject {
        match &self.primitive {
            Primitive::Text(_) => unreachable!("text conversion needs an explicit backend context"),
            Primitive::Image(asset) => {
                let mut image = scene::display::Image::new(asset.key.clone(), self.size);
                image.fit = scene::display::ImageFit::Cover;
                image.natural_size = Some(Size::new(asset.width as f32, asset.height as f32));
                DisplayObject::new(self.id.as_str(), image)
            }
            Primitive::Shape(shape) => {
                // Same local geometry and paint as ui/shape_preview.rs.
                let primitive: Shape = match shape.kind {
                    ShapeKind::Rectangle => {
                        scene::display::Rect::new(self.size.width, self.size.height)
                            .rounded(
                                shape
                                    .style
                                    .radius
                                    .min(self.size.width * 0.5)
                                    .min(self.size.height * 0.5),
                            )
                            .into()
                    }
                    ShapeKind::Ellipse => {
                        scene::display::Ellipse::new(self.size.width, self.size.height).into()
                    }
                    ShapeKind::Line => {
                        let line = shape.line.expect("validated line endpoints");
                        scene::display::Line::new(
                            self.local_point(line.start),
                            self.local_point(line.end),
                        )
                        .into()
                    }
                };
                DisplayObject::new(self.id.as_str(), primitive).with_style(Style {
                    fill: if shape.kind == ShapeKind::Line {
                        None
                    } else {
                        shape.style.fill.map(|color| Color::hex(color).into())
                    },
                    stroke: shape
                        .style
                        .stroke
                        .filter(|_| shape.style.stroke_width > 0.)
                        .map(|color| Stroke::new(Color::hex(color), shape.style.stroke_width)),
                    opacity: shape.style.opacity,
                    ..Default::default()
                })
            }
            Primitive::Pencil(pencil) => {
                // Same dot/path decision as ui/pencil_preview.rs, after local
                // multiplication so degenerate resized strokes stay dots.
                let first = self.local_point(pencil.points[0]);
                if pencil
                    .points
                    .iter()
                    .all(|point| self.local_point(*point) == first)
                {
                    return DisplayObject::new(
                        self.id.as_str(),
                        scene::display::Ellipse::circle(pencil.style.stroke_width / 2.),
                    )
                    .with_transform(Affine::translation(
                        first.x - pencil.style.stroke_width / 2.,
                        first.y - pencil.style.stroke_width / 2.,
                    ))
                    .with_style(Style {
                        fill: Some(Color::hex(pencil.style.color).into()),
                        opacity: pencil.style.opacity,
                        ..Default::default()
                    });
                }
                let commands = std::iter::once(scene::display::PathCommand::MoveTo { to: first })
                    .chain(pencil.points.iter().skip(1).map(|point| {
                        scene::display::PathCommand::LineTo {
                            to: self.local_point(*point),
                        }
                    }))
                    .collect();
                DisplayObject::new(self.id.as_str(), scene::display::Path::new(commands))
                    .with_style(Style {
                        fill: None,
                        stroke: Some(Stroke::new(
                            Color::hex(pencil.style.color),
                            pencil.style.stroke_width,
                        )),
                        opacity: pencil.style.opacity,
                        ..Default::default()
                    })
            }
        }
    }

    fn local_point(&self, point: Point) -> Point {
        Point::new(point.x * self.size.width, point.y * self.size.height)
    }
}

fn validate_asset(asset: &ProjectAsset) -> Result<(), String> {
    validate_asset_key(&asset.key).map_err(|error| error.to_string())?;
    if !matches!(asset.media_type.as_str(), "image/png" | "image/jpeg") {
        return Err("Frame PNG accepts static PNG/JPEG assets only".into());
    }
    if asset.byte_len == 0 || asset.byte_len > MAX_IMAGE_BYTES || asset.name.len() > 1024 {
        return Err("Frame PNG image asset exceeds its byte or metadata limit".into());
    }
    check_dimensions(asset.width, asset.height)
}

fn check_dimensions(width: u32, height: u32) -> Result<(), String> {
    let limits = CompositeLimits::default();
    if width == 0
        || height == 0
        || width > limits.max_dimension
        || height > limits.max_dimension
        || u64::from(width) * u64::from(height) > limits.max_pixels
    {
        Err("Frame PNG output and each source image must fit 4096 pixels per axis and 4194304 total pixels".into())
    } else {
        Ok(())
    }
}

fn check_active(cancelled: &AtomicBool) -> Result<(), String> {
    if cancelled.load(Ordering::Acquire) {
        Err("Frame PNG export was cancelled".into())
    } else {
        Ok(())
    }
}

fn check_memory(bytes: u64) -> Result<(), String> {
    if bytes > CompositeLimits::default().max_surface_bytes {
        Err("Frame PNG sources and decoding scratch exceed the 128 MiB working-set limit".into())
    } else {
        Ok(())
    }
}

fn decode_asset(
    project: &LocalProject,
    asset: &ProjectAsset,
    retained: u64,
    cancelled: &AtomicBool,
) -> Result<StaticImage, String> {
    check_active(cancelled)?;
    validate_asset(asset)?;
    check_memory(retained.saturating_add(asset.byte_len + 1))?;
    let path = project
        .resolve_asset_path(&asset.key)
        .map_err(|error| error.to_string())?;
    let mut file = File::open(path).map_err(|error| error.to_string())?;
    let metadata = file.metadata().map_err(|error| error.to_string())?;
    if !metadata.is_file() || metadata.len() != asset.byte_len {
        return Err("Frame PNG image changed before reading".into());
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(asset.byte_len as usize + 1)
        .map_err(|_| "Frame PNG source allocation failed")?;
    check_memory(retained.saturating_add(bytes.capacity() as u64))?;
    // Chunked reads keep cancellation responsive and never permit growth past
    // the admitted saved byte count, even if the source changes while reading.
    let mut chunk = [0u8; 16 * 1024];
    loop {
        check_active(cancelled)?;
        let remaining = asset.byte_len as usize + 1 - bytes.len();
        if remaining == 0 {
            return Err("Frame PNG image changed while reading".into());
        }
        let count = file
            .read(&mut chunk[..remaining.min(16 * 1024)])
            .map_err(|error| error.to_string())?;
        if count == 0 {
            break;
        }
        bytes.extend_from_slice(&chunk[..count]);
    }
    if bytes.len() as u64 != asset.byte_len {
        return Err("Frame PNG image changed while reading".into());
    }
    drop(file);
    let format = inspect_header(&bytes, asset, cancelled)?;
    let pixels = u64::from(asset.width) * u64::from(asset.height);
    // PNG's limit bounds its internal buffers, including compressed metadata.
    // JPEG's locked decoder does not honor max_alloc internally: allow padded
    // coefficient/upsampling planes plus its compressed input/metadata copies.
    let decoded_upper = pixels * 8; // Includes sixteen-bit PNG RGBA.
    let codec_scratch = admit_decode(asset, format, bytes.capacity() as u64, retained)?;
    let mut limits = Limits::default();
    limits.max_image_width = Some(CompositeLimits::default().max_dimension);
    limits.max_image_height = Some(CompositeLimits::default().max_dimension);
    limits.max_alloc = Some(codec_scratch);
    let mut reader = ImageReader::with_format(
        CancelReader {
            cursor: Cursor::new(bytes.as_slice()),
            cancelled,
        },
        format,
    );
    reader.limits(limits);
    check_active(cancelled)?;
    let decoder = reader
        .into_decoder()
        .map_err(|error| format!("Frame PNG image header is invalid: {error}"))?;
    if decoder.dimensions() != (asset.width, asset.height) || decoder.total_bytes() > decoded_upper
    {
        return Err(
            "Frame PNG decoded dimensions or channels disagree with the admitted asset".into(),
        );
    }
    check_active(cancelled)?;
    let decoded = DynamicImage::from_decoder(decoder)
        .map_err(|error| format!("Frame PNG image decoding failed: {error}"))?;
    check_active(cancelled)?;
    drop(bytes);
    let mut rgba = decoded.into_rgba8().into_raw();
    check_active(cancelled)?;
    check_memory(retained.saturating_add(rgba.capacity() as u64))?;
    for chunk in rgba.chunks_mut(16 * 1024) {
        check_active(cancelled)?;
        for pixel in chunk.as_chunks_mut::<4>().0 {
            let alpha = u16::from(pixel[3]);
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
    }
    check_active(cancelled)?;
    StaticImage::from_premultiplied_rgba(
        asset.width,
        asset.height,
        rgba,
        CompositeLimits::default(),
    )
    .map_err(|error| error.to_string())
}

fn admit_decode(
    asset: &ProjectAsset,
    format: ImageFormat,
    encoded_capacity: u64,
    retained: u64,
) -> Result<u64, String> {
    let pixels = u64::from(asset.width) * u64::from(asset.height);
    let (codec_scratch, encoded_copies) = if format == ImageFormat::Jpeg {
        let padded_pixels =
            u64::from(asset.width.div_ceil(32) * 32) * u64::from(asset.height.div_ceil(32) * 32);
        (padded_pixels * 16 + CODEC_FIXED_BYTES, 4)
    } else {
        // PNG enforces this internal allocation budget before reading image
        // metadata. It is separate from the decoder's returned pixel buffer.
        (pixels * 8 + CODEC_FIXED_BYTES, 1)
    };
    let encoded_peak = encoded_capacity.saturating_mul(encoded_copies);
    check_memory(
        retained
            .saturating_add(encoded_peak)
            .saturating_add(codec_scratch)
            .saturating_add(pixels * 8)
            .saturating_add(pixels * 4),
    )?;
    Ok(codec_scratch)
}

/// Inspect container structure/dimensions without allocating decoded storage.
/// Extension and display name never determine a codec. PNG animation is rejected.
fn inspect_header(
    bytes: &[u8],
    asset: &ProjectAsset,
    cancelled: &AtomicBool,
) -> Result<ImageFormat, String> {
    let format = image::guess_format(bytes).map_err(|error| error.to_string())?;
    let expected = match format {
        ImageFormat::Png => "image/png",
        ImageFormat::Jpeg => "image/jpeg",
        _ => return Err("Frame PNG accepts static PNG/JPEG bytes only".into()),
    };
    if asset.media_type != expected {
        return Err("Frame PNG image header and manifest media type disagree".into());
    }
    let dimensions = if format == ImageFormat::Png {
        if bytes.len() < 33 || bytes[8..16] != [0, 0, 0, 13, b'I', b'H', b'D', b'R'] {
            return Err("Frame PNG source has an invalid PNG header".into());
        }
        let dimensions = (
            u32::from_be_bytes(bytes[16..20].try_into().expect("PNG width")),
            u32::from_be_bytes(bytes[20..24].try_into().expect("PNG height")),
        );
        check_dimensions(dimensions.0, dimensions.1)?;
        let mut offset = 8usize;
        let mut ended = false;
        let mut chunks = 0usize;
        while offset < bytes.len() {
            check_active(cancelled)?;
            chunks += 1;
            if chunks > 65_536 || bytes.len() - offset < 12 {
                return Err("Frame PNG source has an invalid or oversized PNG chunk table".into());
            }
            let length = u32::from_be_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .expect("PNG chunk length"),
            ) as usize;
            let end = offset
                .checked_add(12)
                .and_then(|start| start.checked_add(length))
                .filter(|end| *end <= bytes.len())
                .ok_or("Frame PNG source has a truncated PNG chunk")?;
            let kind = &bytes[offset + 4..offset + 8];
            if matches!(kind, b"acTL" | b"fcTL" | b"fdAT") {
                return Err(
                    "Frame PNG cannot flatten APNG animation; use a static PNG or JPEG".into(),
                );
            }
            if kind == b"IEND" {
                if length != 0 || end != bytes.len() {
                    return Err("Frame PNG source has an invalid PNG ending".into());
                }
                ended = true;
                break;
            }
            offset = end;
        }
        if !ended {
            return Err("Frame PNG source has no complete PNG ending".into());
        }
        dimensions
    } else {
        jpeg_dimensions(bytes, cancelled)?
    };
    check_dimensions(dimensions.0, dimensions.1)?;
    if dimensions != (asset.width, asset.height) {
        return Err("Frame PNG image header dimensions disagree with the manifest".into());
    }
    Ok(format)
}

fn jpeg_dimensions(bytes: &[u8], cancelled: &AtomicBool) -> Result<(u32, u32), String> {
    let mut offset = 2usize;
    for _ in 0..65_536 {
        check_active(cancelled)?;
        if bytes.get(offset) != Some(&0xff) {
            break;
        }
        while bytes.get(offset) == Some(&0xff) {
            offset += 1;
        }
        let marker = *bytes
            .get(offset)
            .ok_or("Frame PNG source has a truncated JPEG marker")?;
        offset += 1;
        if matches!(marker, 0xda | 0xd9) {
            break;
        }
        if matches!(marker, 0x01 | 0xd0..=0xd7) {
            continue;
        }
        let length = bytes
            .get(offset..offset + 2)
            .ok_or("Frame PNG source has a truncated JPEG segment")?;
        let length = u16::from_be_bytes(length.try_into().expect("JPEG segment length")) as usize;
        let end = offset
            .checked_add(length)
            .filter(|end| length >= 2 && *end <= bytes.len())
            .ok_or("Frame PNG source has an invalid JPEG segment")?;
        if matches!(marker, 0xc0..=0xc3 | 0xc5..=0xc7 | 0xc9..=0xcb | 0xcd..=0xcf) {
            let header = &bytes[offset + 2..end];
            if !matches!(marker, 0xc0..=0xc2) || header.len() < 6 || header[0] != 8 {
                return Err("Frame PNG source uses an unsupported JPEG coding mode".into());
            }
            let components = usize::from(header[5]);
            if !(1..=4).contains(&components)
                || header.len() != 6 + 3 * components
                || header[6..].as_chunks::<3>().0.iter().any(|component| {
                    !(1..=4).contains(&(component[1] >> 4))
                        || !(1..=4).contains(&(component[1] & 15))
                })
            {
                return Err("Frame PNG source has invalid JPEG component geometry".into());
            }
            return Ok((
                u32::from(u16::from_be_bytes([header[3], header[4]])),
                u32::from(u16::from_be_bytes([header[1], header[2]])),
            ));
        }
        offset = end;
    }
    Err("Frame PNG source has no supported JPEG size header".into())
}

struct CancelReader<'a> {
    cursor: Cursor<&'a [u8]>,
    cancelled: &'a AtomicBool,
}
impl Read for CancelReader<'_> {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        check_active(self.cancelled).map_err(io::Error::other)?;
        self.cursor.read(buffer)
    }
}
impl BufRead for CancelReader<'_> {
    fn fill_buf(&mut self) -> io::Result<&[u8]> {
        check_active(self.cancelled).map_err(io::Error::other)?;
        self.cursor.fill_buf()
    }
    fn consume(&mut self, amount: usize) {
        self.cursor.consume(amount);
    }
}
impl Seek for CancelReader<'_> {
    fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        check_active(self.cancelled).map_err(io::Error::other)?;
        self.cursor.seek(position)
    }
}

fn suggested_name(title: &str) -> String {
    let mut stem = String::new();
    for character in title.chars() {
        let character = if character.is_control()
            || matches!(
                character,
                '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
            ) {
            '_'
        } else {
            character
        };
        if stem.len() + character.len_utf8() > 96 {
            break;
        }
        stem.push(character);
    }
    let stem = stem.trim_matches([' ', '.']);
    let stem = if stem
        .get(stem.len().saturating_sub(4)..)
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".png"))
    {
        &stem[..stem.len() - 4]
    } else {
        stem
    };
    if stem.trim().is_empty() {
        "Frame.png".into()
    } else {
        format!("{stem}.png")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        pencil::{PencilStyle, create_pencil_node_from_points},
        project::ProjectStore,
        shapes::{ShapeStyle, create_shape_node},
        standard_registry,
    };
    use gpui_flow_core::{FlowDocument, Group, Node, set_node_visible, write_node_order};
    use image::{Rgba, RgbaImage};
    use std::{
        fs,
        io::Write,
        path::PathBuf,
        sync::atomic::AtomicU64,
        time::{SystemTime, UNIX_EPOCH},
    };

    struct TestHome(PathBuf);
    impl TestHome {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "flow-frame-png-{}-{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn project(&self) -> LocalProject {
            ProjectStore::new(self.0.join("projects"))
                .unwrap()
                .create_project(
                    "Frame PNG tests",
                    &FlowDocument::default(),
                    &standard_registry(),
                )
                .unwrap()
        }

        fn import(
            &self,
            project: &mut LocalProject,
            name: &str,
            image: RgbaImage,
            format: ImageFormat,
        ) -> ProjectAsset {
            let mut output = Cursor::new(Vec::new());
            let image = DynamicImage::ImageRgba8(image);
            if format == ImageFormat::Jpeg {
                DynamicImage::ImageRgb8(image.into_rgb8())
                    .write_to(&mut output, format)
                    .unwrap();
            } else {
                image.write_to(&mut output, format).unwrap();
            }
            self.import_bytes(project, name, output.get_ref())
        }

        fn import_bytes(
            &self,
            project: &mut LocalProject,
            name: &str,
            bytes: &[u8],
        ) -> ProjectAsset {
            let path = self.0.join(name);
            File::create_new(&path).unwrap().write_all(bytes).unwrap();
            project.import_image(path).unwrap()
        }
    }
    impl Drop for TestHome {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn image_node(id: &str, asset: &ProjectAsset, position: Point, size: Size) -> Node {
        Node::new(id, id, position)
            .with_kind("image")
            .with_size(size)
            .with_data(serde_json::json!({"asset": asset.key}))
    }

    fn rectangle(id: &str, position: Point, size: Size, color: u32) -> Node {
        let mut shape = ShapeData::new(ShapeKind::Rectangle);
        shape.style = ShapeStyle {
            fill: Some(color),
            stroke: None,
            ..Default::default()
        };
        create_shape_node(id, id, position, size, shape).unwrap()
    }

    fn framed(nodes: Vec<Node>, bounds: Rect, white: bool) -> (Graph, Selection) {
        let mut graph = Graph::new();
        let ids = nodes.iter().map(|node| node.id.clone()).collect::<Vec<_>>();
        for node in nodes {
            graph.add_node(node).unwrap();
        }
        let mut frame = Group::new("frame", "Test board", bounds).with_nodes(ids);
        if white {
            frame.data = serde_json::json!({"flow_creative_frame": 1});
        }
        graph.add_group(frame).unwrap();
        let mut selection = Selection::default();
        selection.select_group("frame", false);
        (graph, selection)
    }

    fn png(export: &FramePngExport, project: &LocalProject) -> RgbaImage {
        let bytes = export.encode(project, &AtomicBool::new(false)).unwrap();
        image::load_from_memory_with_format(&bytes, ImageFormat::Png)
            .unwrap()
            .into_rgba8()
    }

    #[test]
    fn text_snapshot_tracks_rendered_fields_and_needs_the_real_font_backend() {
        let home = TestHome::new();
        let project = home.project();
        let node = Node::new("label", "Title", Point::new(20., 30.))
            .with_kind("text")
            .with_size(Size::new(200., 40.))
            .with_data(serde_json::json!({"text":"Frame label\n第二行", "style":{"font_size":28,"bold":false,"alignment":"left"}}));
        let (mut graph, selection) = framed(
            vec![node],
            Rect::new(Point::ZERO, Size::new(300., 200.)),
            true,
        );
        let export = FramePngExport::prepare(&graph, &selection, &[]).unwrap();
        assert!(export.has_text());
        assert!(
            export
                .encode(&project, &AtomicBool::new(false))
                .unwrap_err()
                .contains("native font backend")
        );
        assert_eq!(export.objects[0].size, Size::new(200., 40.));
        graph.nodes.get_mut(&"label".into()).unwrap().data["unrelated"] = serde_json::json!(true);
        assert!(export.matches(&graph, &selection, &[]));
        graph.nodes.get_mut(&"label".into()).unwrap().data["text"] = serde_json::json!("changed");
        assert!(!export.matches(&graph, &selection, &[]));
        let next = FramePngExport::prepare(&graph, &selection, &[]).unwrap();
        graph.nodes.get_mut(&"label".into()).unwrap().data["style"]["color"] =
            serde_json::json!(0xB45309);
        assert!(!next.matches(&graph, &selection, &[]));
        graph.nodes.get_mut(&"label".into()).unwrap().size.width = 20.;
        assert!(FramePngExport::prepare(&graph, &selection, &[]).is_err());
    }

    #[test]
    fn creative_native_text_keeps_ink_outside_its_layout_box_and_font_epoch() {
        let home = TestHome::new();
        let project = home.project();
        let (_app, bridge) =
            gpui_flow::GpuiTextRasterizer::application(gpui_kit::platform::current_platform(true));
        let backend = FrameTextBackend::new(&bridge, "sans-serif".into(), Arc::default()).unwrap();
        let node = Node::new("label", "Label", Point::new(20., 20.))
            .with_kind("text")
            .with_size(Size::new(24., 18.))
            .with_data(serde_json::json!({"text":"W\nW", "style":{
                "font_size":28,"bold":true,"alignment":"left","color":0xb45309}}));
        let (graph, selection) = framed(
            vec![node],
            Rect::new(Point::ZERO, Size::new(160., 120.)),
            true,
        );
        let source = serde_json::to_string(&graph).unwrap();
        let export = FramePngExport::prepare(&graph, &selection, &[]).unwrap();
        let cancelled = AtomicBool::new(false);
        let bytes = export
            .encode_with_text_backend(&project, Some(&backend), &cancelled)
            .unwrap();
        let pixels = image::load_from_memory(&bytes).unwrap().into_rgba8();
        let ink: Vec<_> = pixels
            .enumerate_pixels()
            .filter(|(_, _, p)| p[2] < 220)
            .collect();
        assert!(ink.len() > 50);
        assert!(
            ink.iter().any(|(x, _, _)| *x >= 44),
            "wide glyph ink must overflow the narrow layout box"
        );
        assert!(
            ink.iter().any(|(_, y, _)| *y >= 38),
            "second-line ink must overflow the short layout box"
        );
        assert!(
            ink.iter().any(|(_, _, p)| p.0 == [180, 83, 9, 255]),
            "native glyph fill must retain the selected text color"
        );
        assert_eq!(serde_json::to_string(&graph).unwrap(), source);
        bridge.invalidate();
        assert!(
            export
                .encode_with_text_backend(&project, Some(&backend), &cancelled)
                .is_err()
        );
    }

    #[test]
    fn mixed_png_uses_world_crop_cover_stack_order_and_shared_image_pixels() {
        let home = TestHome::new();
        let mut project = home.project();
        let source = RgbaImage::from_fn(8, 4, |x, _| {
            Rgba(match x {
                0..=1 => [255, 0, 0, 255],
                2..=5 => [0, 255, 0, 255],
                _ => [0, 0, 255, 255],
            })
        });
        let asset = home.import(&mut project, "wide.png", source, ImageFormat::Png);
        let pencil = create_pencil_node_from_points(
            "pencil",
            "stroke",
            &[Point::new(14.5, 24.5), Point::new(15.5, 24.5)],
            PencilStyle {
                color: 0xff0000,
                stroke_width: 2.,
                opacity: 1.,
            },
        )
        .unwrap();
        let (mut graph, selection) = framed(
            vec![
                image_node("z-image", &asset, Point::new(9., 20.), Size::new(4., 4.)),
                rectangle("a-shape", Point::new(11., 21.), Size::new(2., 2.), 0x0000ff),
                image_node("shared", &asset, Point::new(100., 100.), Size::new(4., 4.)),
                pencil,
            ],
            Rect::new(Point::new(10., 20.), Size::new(6., 6.)),
            false,
        );
        write_node_order(
            &mut graph,
            ["z-image", "a-shape", "pencil", "shared"].map(NodeId::from),
        )
        .unwrap();
        let before = (graph.clone(), selection.clone());
        let export = FramePngExport::prepare(&graph, &selection, project.assets()).unwrap();
        assert_eq!(export.object_count(), 4);
        assert_eq!(export.suggested_name(), "Test board.png");
        let (inputs, work) = export
            .load_images(&project, &AtomicBool::new(false))
            .unwrap();
        assert_eq!(inputs.byte_len(), 8 * 4 * 4);
        assert_eq!(work, 8 * 4 * 2);
        let output = png(&export, &project);
        assert_eq!(output.dimensions(), (6, 6));
        assert_eq!(output.get_pixel(0, 0).0, [0, 255, 0, 255]);
        assert_eq!(output.get_pixel(1, 1).0, [0, 0, 255, 255]);
        assert_eq!(output.get_pixel(3, 0).0, [0, 0, 0, 0]);
        assert_eq!(output.get_pixel(4, 4).0, [255, 0, 0, 255]);
        assert_eq!((graph.clone(), selection.clone()), before);
        write_node_order(
            &mut graph,
            ["a-shape", "z-image", "pencil", "shared"].map(NodeId::from),
        )
        .unwrap();
        assert!(!export.matches(&graph, &selection, project.assets()));
        let reordered = FramePngExport::prepare(&graph, &selection, project.assets()).unwrap();
        assert_eq!(
            png(&reordered, &project).get_pixel(1, 1).0,
            [0, 255, 0, 255]
        );
        // The earlier immutable snapshot remains independently encodable.
        assert_eq!(png(&export, &project), output);
    }

    #[test]
    fn png_preserves_image_alpha_and_exact_fractional_white_frame_edges() {
        let home = TestHome::new();
        let mut project = home.project();
        let asset = home.import(
            &mut project,
            "alpha.png",
            RgbaImage::from_pixel(2, 2, Rgba([255, 0, 0, 128])),
            ImageFormat::Png,
        );
        let (mut graph, selection) = framed(
            vec![image_node(
                "image",
                &asset,
                Point::new(-3., 4.),
                Size::new(2., 2.),
            )],
            Rect::new(Point::new(-3., 4.), Size::new(4.5, 3.5)),
            false,
        );
        let transparent = FramePngExport::prepare(&graph, &selection, project.assets()).unwrap();
        assert_eq!(transparent.pixel_size(), [5, 4]);
        let output = png(&transparent, &project);
        assert_eq!(output.get_pixel(0, 0).0, [255, 0, 0, 128]);
        assert_eq!(output.get_pixel(3, 0).0, [0, 0, 0, 0]);
        graph.groups.get_mut(&GroupId::from("frame")).unwrap().data =
            serde_json::json!({"flow_creative_frame": 1});
        assert!(!transparent.matches(&graph, &selection, project.assets()));
        let white = FramePngExport::prepare(&graph, &selection, project.assets()).unwrap();
        assert!(white.has_white_background());
        let output = png(&white, &project);
        assert_eq!(output.get_pixel(0, 0).0, [255, 127, 127, 255]);
        assert_eq!(output.get_pixel(3, 0).0, [255, 255, 255, 255]);
        assert!((120..=136).contains(&output.get_pixel(4, 0).0[3]));
        assert!((56..=72).contains(&output.get_pixel(4, 3).0[3]));
    }

    #[test]
    fn hidden_unsupported_members_locks_and_stale_snapshot_identity_are_explicit() {
        let bounds = Rect::new(Point::ZERO, Size::new(8., 8.));
        let (mut graph, selection) = framed(
            vec![
                rectangle("shape", Point::ZERO, Size::new(4., 4.), 0x123456),
                Node::new("hidden", "Hidden", Point::ZERO).with_kind("video"),
            ],
            bounds,
            false,
        );
        set_node_visible(&mut graph, &NodeId::from("hidden"), false).unwrap();
        graph.nodes.get_mut(&NodeId::from("shape")).unwrap().locked = true;
        graph
            .groups
            .get_mut(&GroupId::from("frame"))
            .unwrap()
            .locked = true;
        let export = FramePngExport::prepare(&graph, &selection, &[]).unwrap();
        assert_eq!(export.object_count(), 1);
        for kind in [
            "video",
            "audio",
            "model",
            "generate_image",
            "generate_video",
            "generate_model",
            "custom.sample",
            "mask",
            "image",
        ] {
            graph.nodes.get_mut(&NodeId::from("hidden")).unwrap().kind = kind.into();
            assert!(export.matches(&graph, &selection, &[]));
            set_node_visible(&mut graph, &NodeId::from("hidden"), true).unwrap();
            assert!(FramePngExport::prepare(&graph, &selection, &[]).is_err());
            assert!(!export.matches(&graph, &selection, &[]));
            set_node_visible(&mut graph, &NodeId::from("hidden"), false).unwrap();
        }
        graph.nodes.get_mut(&NodeId::from("shape")).unwrap().locked = false;
        graph
            .groups
            .get_mut(&GroupId::from("frame"))
            .unwrap()
            .locked = false;
        assert!(export.matches(&graph, &selection, &[]));
        let mut changed = selection.clone();
        changed.nodes.insert("shape".into());
        assert!(!export.matches(&graph, &changed, &[]));
        graph
            .groups
            .get_mut(&GroupId::from("frame"))
            .unwrap()
            .title
            .push('!');
        assert!(!export.matches(&graph, &selection, &[]));
        graph
            .groups
            .get_mut(&GroupId::from("frame"))
            .unwrap()
            .title
            .pop();
        graph
            .nodes
            .get_mut(&NodeId::from("shape"))
            .unwrap()
            .position
            .x = 1.;
        assert!(!export.matches(&graph, &selection, &[]));
        set_node_visible(&mut graph, &NodeId::from("shape"), false).unwrap();
        assert!(
            FramePngExport::prepare(&graph, &selection, &[])
                .unwrap_err()
                .contains("no visible")
        );
        graph.nodes.remove(&NodeId::from("hidden"));
        assert!(
            FramePngExport::prepare(&graph, &selection, &[])
                .unwrap_err()
                .contains("no longer exists")
        );
    }

    #[test]
    fn image_and_output_limits_cancellation_and_combined_memory_fail_closed() {
        let home = TestHome::new();
        let project = home.project();
        let (mut graph, selection) = framed(
            vec![rectangle("shape", Point::ZERO, Size::new(2., 2.), 0x123456)],
            Rect::new(Point::ZERO, Size::new(8., 8.)),
            false,
        );
        let export = FramePngExport::prepare(&graph, &selection, &[]).unwrap();
        assert!(
            export
                .encode(&project, &AtomicBool::new(true))
                .unwrap_err()
                .contains("cancelled")
        );
        for size in [
            Size::new(4097., 1.),
            Size::new(2049., 2048.),
            Size::new(f32::NAN, 2.),
        ] {
            graph
                .groups
                .get_mut(&GroupId::from("frame"))
                .unwrap()
                .bounds
                .size = size;
            assert!(FramePngExport::prepare(&graph, &selection, &[]).is_err());
        }
        assert!(check_dimensions(4096, 1024).is_ok());
        assert!(check_dimensions(4096, 1025).is_err());
        assert!(check_memory(128 * 1024 * 1024).is_ok());
        assert!(check_memory(128 * 1024 * 1024 + 1).is_err());
        let asset = ProjectAsset {
            key: "assets/test.png".into(),
            name: "test.png".into(),
            media_type: "image/png".into(),
            byte_len: 100,
            width: 4096,
            height: 1025,
        };
        assert!(validate_asset(&asset).is_err());
        let bounded = ProjectAsset {
            width: 2048,
            height: 2048,
            ..asset.clone()
        };
        assert!(admit_decode(&bounded, ImageFormat::Png, 1024, HOST_RETAINED_BYTES).is_ok());
        // A second retained large input can make decoding exceed the shared
        // budget despite every individual buffer passing its own limit.
        assert!(
            admit_decode(
                &bounded,
                ImageFormat::Png,
                32 * 1024 * 1024,
                HOST_RETAINED_BYTES + 16 * 1024 * 1024
            )
            .is_err()
        );
        let (graph, selection) = framed(
            vec![image_node("image", &asset, Point::ZERO, Size::new(2., 2.))],
            Rect::new(Point::ZERO, Size::new(8., 8.)),
            false,
        );
        assert!(FramePngExport::prepare(&graph, &selection, &[asset]).is_err());
    }

    #[test]
    fn jpeg_png_headers_asset_identity_and_decode_failures_are_checked() {
        let home = TestHome::new();
        let mut project = home.project();
        let asset = home.import(
            &mut project,
            "actually-jpeg.png",
            RgbaImage::from_pixel(3, 2, Rgba([230, 40, 20, 255])),
            ImageFormat::Jpeg,
        );
        assert_eq!(asset.media_type, "image/jpeg");
        let (graph, selection) = framed(
            vec![image_node("image", &asset, Point::ZERO, Size::new(3., 2.))],
            Rect::new(Point::ZERO, Size::new(3., 2.)),
            false,
        );
        let export = FramePngExport::prepare(&graph, &selection, project.assets()).unwrap();
        let output = png(&export, &project);
        let pixel = output.get_pixel(1, 1).0;
        assert!(pixel[0] > 215 && pixel[1] < 55 && pixel[2] < 35 && pixel[3] == 255);
        let mut changed = asset.clone();
        changed.name.push('!');
        assert!(!export.matches(&graph, &selection, &[changed.clone()]));
        let stale = FramePngExport::prepare(&graph, &selection, &[changed]).unwrap();
        assert!(
            stale
                .encode(&project, &AtomicBool::new(false))
                .unwrap_err()
                .contains("asset changed")
        );
        let png_asset = home.import(
            &mut project,
            "static.png",
            RgbaImage::from_pixel(2, 2, Rgba([0, 255, 0, 255])),
            ImageFormat::Png,
        );
        let bytes = fs::read(project.resolve_asset_path(&png_asset.key).unwrap()).unwrap();
        let mut wrong = png_asset.clone();
        wrong.width = 3;
        assert!(
            inspect_header(&bytes, &wrong, &AtomicBool::new(false))
                .unwrap_err()
                .contains("dimensions")
        );
        wrong = png_asset.clone();
        wrong.media_type = "image/jpeg".into();
        assert!(
            inspect_header(&bytes, &wrong, &AtomicBool::new(false))
                .unwrap_err()
                .contains("media type")
        );
        let mut animated = bytes[..33].to_vec();
        animated.extend_from_slice(&[0, 0, 0, 8]);
        animated.extend_from_slice(b"acTL");
        animated.extend_from_slice(&[0; 12]);
        animated.extend_from_slice(&bytes[33..]);
        assert!(
            inspect_header(&animated, &png_asset, &AtomicBool::new(false))
                .unwrap_err()
                .contains("APNG")
        );
        let mut corrupt = bytes.clone();
        let idat = corrupt
            .windows(4)
            .position(|window| window == b"IDAT")
            .unwrap();
        corrupt[idat + 4] ^= 0xff;
        let corrupt_asset = home.import_bytes(&mut project, "corrupt.png", &corrupt);
        let (graph, selection) = framed(
            vec![image_node(
                "bad",
                &corrupt_asset,
                Point::ZERO,
                Size::new(2., 2.),
            )],
            Rect::new(Point::ZERO, Size::new(2., 2.)),
            false,
        );
        let export = FramePngExport::prepare(&graph, &selection, project.assets()).unwrap();
        assert!(export.encode(&project, &AtomicBool::new(false)).is_err());
        let mut missing_graph = graph.clone();
        missing_graph
            .nodes
            .get_mut(&NodeId::from("bad"))
            .unwrap()
            .data = serde_json::json!({"asset": "https://example.invalid/image.png"});
        assert!(FramePngExport::prepare(&missing_graph, &selection, project.assets()).is_err());
    }
}
