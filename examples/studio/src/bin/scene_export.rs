//! Fixed-world scene export with a separate decoded preview and JSON receipts.
//! Embedded fixtures only: no URL fetch, native screenshot, or automatic upload.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneView,
    composite::{
        CanvasPixels, CompositeLimits, SceneExport, SceneExportFormat, SceneExportRequest,
    },
    scene::{
        Affine, Bounds as SceneBounds, Color, DisplayObject, Point as ScenePoint, Scene, Shape,
        Size as SceneSize, Style, display,
    },
};
use serde_json::{Value, json};
use std::{
    io::Write,
    path::{Path, PathBuf},
    rc::Rc,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

const WIDTH: f32 = 440.;
const HEIGHT: f32 = 320.;
const BACKGROUND: Color = Color::rgb(232, 238, 247);

struct FixtureCache {
    image: Arc<RenderImage>,
}
impl FixtureCache {
    fn new(cx: &mut App) -> Entity<Self> {
        let bgra = image::RgbaImage::from_fn(96, 72, |x, y| {
            let alpha = if x < 24 {
                0
            } else if x < 56 {
                128
            } else {
                255
            };
            let color = if (x / 12 + y / 12) % 2 == 0 {
                [55, 155, 245, alpha]
            } else {
                [225, 100, 55, alpha]
            };
            image::Rgba(color)
        });
        let cache = cx.new(|_| Self {
            image: Arc::new(RenderImage::new(vec![image::Frame::new(bgra)])),
        });
        cx.observe_release(&cache, |cache, cx| cx.drop_image(cache.image.clone(), None))
            .detach();
        cache
    }
}
impl ImageCache for FixtureCache {
    fn load(
        &mut self,
        resource: &Resource,
        _: &mut Window,
        _: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        match resource {
            Resource::Embedded(name) if name.as_ref() == "ready" => Some(Ok(self.image.clone())),
            Resource::Embedded(name) if name.as_ref() == "pending" => None,
            Resource::Embedded(name) if name.as_ref() == "failed" => Some(Err(
                ImageCacheError::Asset("controlled export fixture failure".into()),
            )),
            _ => Some(Err(ImageCacheError::Asset(
                "unknown embedded export fixture".into(),
            ))),
        }
    }
}
fn empty_style() -> Style {
    Style {
        fill: None,
        ..Default::default()
    }
}
fn document() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("clip", display::Frame::new(210., 145.).rounded(24.))
                .with_transform(Affine::translation(35., 45.))
                .with_style(Style::fill(Color::hex(0x17354e))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("vector", display::Rect::new(250., 85.).rounded(16.))
                .with_transform(Affine::translation(-22., 45.) * Affine::rotation(-18.))
                .with_style(Style::fill(Color::rgba(255, 184, 73, 205))),
            Some("clip".into()),
            None,
        )
        .unwrap();
    let mut image = display::Image::new("ready", SceneSize::new(126., 105.)).rounded(17.);
    image.fit = display::ImageFit::Fill;
    scene
        .insert(
            DisplayObject::new("image", image).with_transform(Affine::translation(275., 47.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("canvas", display::Canvas::new(100., 70.).rounded(15.))
                .with_transform(Affine::translation(291., 175.) * Affine::rotation(8.))
                .with_style(empty_style()),
            None,
            None,
        )
        .unwrap();
    let mut text = display::Text::new("Straße · ffi café\nİ ΟΣ · 漢字", SceneSize::new(252., 104.));
    text.font_family = "DejaVu Sans".into();
    text.font_size = 23.;
    text.line_height = 1.25;
    text.para_spacing = 9.;
    text.text_case = display::TextCase::Upper;
    text.wrap = false;
    scene
        .insert(
            DisplayObject::new("text", text)
                .with_transform(Affine::translation(35., 210.))
                .with_style(Style::fill(Color::hex(0x51b9a9))),
            None,
            None,
        )
        .unwrap();
    scene
}
fn canvas_pattern() -> CanvasPixels {
    let mut rgba = Vec::with_capacity(100 * 70 * 4);
    for y in 0..70 {
        for x in 0..100 {
            rgba.extend_from_slice(if x < 25 && y < 25 {
                &[0, 0, 0, 0]
            } else if (x / 10 + y / 10) % 2 == 0 {
                &[114, 90, 242, 185]
            } else {
                &[55, 219, 201, 255]
            });
        }
    }
    CanvasPixels::from_rgba(100, 70, rgba, CompositeLimits::default()).unwrap()
}
fn fingerprint(bytes: &[u8]) -> String {
    let value = bytes.iter().fold(0xcbf29ce484222325u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!("{value:016x}")
}
fn extension(format: SceneExportFormat) -> &'static str {
    match format {
        SceneExportFormat::Png => "png",
        SceneExportFormat::Jpeg { .. } => "jpg",
        SceneExportFormat::WebPLossless => "webp",
        SceneExportFormat::Svg => "svg",
    }
}
fn decode(output: &SceneExport) -> Result<image::RgbaImage, String> {
    let format = match output.request.format {
        SceneExportFormat::Png => image::ImageFormat::Png,
        SceneExportFormat::Jpeg { .. } => image::ImageFormat::Jpeg,
        SceneExportFormat::WebPLossless => image::ImageFormat::WebP,
        SceneExportFormat::Svg => return decode_svg(output),
    };
    image::load_from_memory_with_format(&output.bytes, format)
        .map(|i| i.to_rgba8())
        .map_err(|e| e.to_string())
}
// Read only this freshly generated bounded export. All file/URL resolution is
// disabled, and the only admitted embedded resource type is PNG.
fn decode_svg(output: &SceneExport) -> Result<image::RgbaImage, String> {
    let options = resvg::usvg::Options {
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|mime, data, _| {
                (mime == "image/png").then_some(resvg::usvg::ImageKind::PNG(data))
            }),
            resolve_string: Box::new(|_, _| None),
        },
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_data(&output.bytes, &options).map_err(|e| e.to_string())?;
    let size = tree.size().to_int_size();
    if (size.width(), size.height()) != (output.width, output.height) {
        return Err("SVG root dimensions differ from the complete export".into());
    }
    let limits = CompositeLimits::default();
    if output.width > limits.max_dimension
        || output.height > limits.max_dimension
        || u64::from(output.width) * u64::from(output.height) > limits.max_pixels
    {
        return Err("SVG preview dimensions exceed fixture readback limits".into());
    }
    let mut pixels = resvg::tiny_skia::Pixmap::new(output.width, output.height)
        .ok_or("SVG readback allocation failed")?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixels.as_mut(),
    );
    let mut rgba = image::RgbaImage::new(output.width, output.height);
    for (destination, source) in rgba.pixels_mut().zip(pixels.pixels()) {
        let color = source.demultiply();
        destination.0 = [color.red(), color.green(), color.blue(), color.alpha()];
    }
    Ok(rgba)
}

fn svg_structure(output: &SceneExport) -> Result<Value, String> {
    let source = std::str::from_utf8(&output.bytes).map_err(|e| e.to_string())?;
    let document = resvg::usvg::roxmltree::Document::parse(source).map_err(|e| e.to_string())?;
    let root = document.root_element();
    if !root.has_tag_name(("http://www.w3.org/2000/svg", "svg")) {
        return Err("export is not an SVG document".into());
    }
    let allowed = [
        "svg",
        "g",
        "defs",
        "clipPath",
        "path",
        "rect",
        "linearGradient",
        "radialGradient",
        "stop",
        "image",
    ];
    for node in document.descendants().filter(|node| node.is_element()) {
        if !allowed.contains(&node.tag_name().name()) {
            return Err(format!(
                "unexpected SVG element: {}",
                node.tag_name().name()
            ));
        }
        for attribute in node.attributes() {
            if attribute.name() == "href"
                && !attribute.value().starts_with("data:image/png;base64,")
            {
                return Err(
                    "SVG fixture unexpectedly references an external/non-PNG resource".into(),
                );
            }
        }
    }
    let vector = document
        .descendants()
        .find(|node| node.attribute("data-flow-id") == Some("vector"))
        .ok_or("SVG fixture lost the canonical vector producer")?;
    let vector_paths = vector
        .descendants()
        .filter(|node| node.has_tag_name("path") && node.attribute("fill").is_some())
        .count();
    if vector_paths == 0 || vector.descendants().any(|node| node.has_tag_name("image")) {
        return Err("canonical vector producer was missing or replaced by raster content".into());
    }
    let mut rasters = Vec::new();
    for node in document
        .descendants()
        .filter(|node| node.has_tag_name("image"))
    {
        let owner = node
            .ancestors()
            .find_map(|node| node.attribute("data-flow-id"))
            .ok_or("SVG image has no canonical producer owner")?;
        let kind = node
            .attribute("data-flow-raster")
            .ok_or("SVG raster kind is missing")?;
        if ![
            ("image", "image"),
            ("canvas", "canvas"),
            ("text", "text-ink"),
        ]
        .contains(&(owner, kind))
        {
            return Err("unexpected per-producer SVG image/ink ownership".into());
        }
        if node
            .attribute(("http://www.w3.org/1999/xlink", "href"))
            .is_none_or(|href| !href.starts_with("data:image/png;base64,"))
        {
            return Err("SVG producer is not a self-contained PNG image".into());
        }
        rasters.push(json!({"producer":owner,"kind":kind,"width":node.attribute("width"),"height":node.attribute("height")}));
    }
    if rasters.len() != 3
        || ["image", "canvas", "text"].into_iter().any(|owner| {
            rasters
                .iter()
                .filter(|node| node["producer"] == owner)
                .count()
                != 1
        })
    {
        return Err("fixture requires exactly one Image, Canvas and Text ink image".into());
    }
    Ok(json!({
        "root_view_box":root.attribute("viewBox"),
        "xml_path_count":document.descendants().filter(|node|node.has_tag_name("path")).count(),
        "clip_count":document.descendants().filter(|node|node.has_tag_name("clipPath")).count(),
        "vector_producer_painted_paths":vector_paths,
        "vector_producer_raster_nodes":0,
        "embedded_png_count":rasters.len(),
        "raster_producers":rasters,
        "editable_text_nodes":0,
        "external_resource_references":0,
        "mixed_svg_not_whole_scene_bitmap":true,
        "text_policy":"same-App shaped ink PNG per Text node; no editable text or glyph outlines"
    }))
}

fn pixel_difference(actual: &image::RgbaImage, reference: &image::RgbaImage) -> Value {
    assert_eq!(actual.dimensions(), reference.dimensions());
    let mut changed_pixels = 0u64;
    let mut changed_channels = 0u64;
    let mut sum = 0u64;
    let mut maximum = 0u8;
    for (a, b) in actual.pixels().zip(reference.pixels()) {
        changed_pixels += u64::from(a != b);
        for (a, b) in a.0.into_iter().zip(b.0) {
            let delta = a.abs_diff(b);
            changed_channels += u64::from(delta != 0);
            sum += u64::from(delta);
            maximum = maximum.max(delta);
        }
    }
    json!({"changed_pixels":changed_pixels,"changed_channels":changed_channels,
        "max_channel_delta":maximum,"sum_absolute_channel_delta":sum,
        "mean_absolute_channel_delta":sum as f64 / actual.as_raw().len() as f64,
        "comparison":"independent SVG renderer versus PNG; statistics only, no pixel-equivalence assertion"})
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|e| e.to_string())?;
    file.write_all(bytes).map_err(|e| e.to_string())
}

struct VerifiedExport {
    output: SceneExport,
    decoded: image::RgbaImage,
    readback: Value,
}
fn verify(view: &SceneView, request: SceneExportRequest) -> Result<VerifiedExport, String> {
    let before = view.editor.scene().to_json().map_err(|e| e.to_string())?;
    let revision = view.editor.engine.revision();
    let selection = view.editor.selection.clone();
    let viewport = view.editor.viewport;
    let handle = view.editor.engine.handle(&"canvas".into()).unwrap();
    let canvas = view
        .resources()
        .and_then(|r| r.canvas_snapshot(&view.editor.engine, handle));
    let outcome = (|| {
        let output = view
            .export_scene(request, CompositeLimits::default())
            .map_err(|e| e.to_string())?;
        let decoded = decode(&output)?;
        let expected = (
            (f64::from(request.world_bounds.size.width)
                * f64::from(request.scale)
                * f64::from(request.pixel_ratio))
            .ceil() as u32,
            (f64::from(request.world_bounds.size.height)
                * f64::from(request.scale)
                * f64::from(request.pixel_ratio))
            .ceil() as u32,
        );
        assert_eq!(
            decoded.dimensions(),
            expected,
            "decoded extent differs from requested crop"
        );
        assert_eq!((output.width, output.height), expected);
        // PNG/WebP retain exact decoded-file equality. SVG uses independent
        // resvg readback with measured differences, never a bit-exact claim.
        let reference_format = if request.format == SceneExportFormat::Png {
            SceneExportFormat::WebPLossless
        } else {
            SceneExportFormat::Png
        };
        let reference = view
            .export_scene(
                SceneExportRequest {
                    format: reference_format,
                    ..request
                },
                CompositeLimits::default(),
            )
            .map_err(|e| e.to_string())?;
        let reference = decode(&reference)?;
        assert_eq!(decoded.dimensions(), reference.dimensions());
        let corner = decoded.get_pixel(0, 0).0;
        let exact = matches!(
            request.format,
            SceneExportFormat::Png | SceneExportFormat::WebPLossless
        );
        let is_svg = request.format == SceneExportFormat::Svg;
        let svg_structure = is_svg.then(|| svg_structure(&output)).transpose()?;
        let svg_difference = is_svg.then(|| pixel_difference(&decoded, &reference));
        if exact {
            assert_eq!(
                decoded.as_raw(),
                reference.as_raw(),
                "PNG/WebP decoded pixel mismatch"
            );
        } else if matches!(request.format, SceneExportFormat::Jpeg { .. }) {
            // Both full and cropped fixtures leave a >8px empty corner.
            for (actual, expected) in
                corner
                    .iter()
                    .zip([BACKGROUND.r, BACKGROUND.g, BACKGROUND.b, 255])
            {
                assert!(
                    actual.abs_diff(expected) <= 3,
                    "JPEG explicit-background probe differs"
                );
            }
            assert!(decoded.pixels().all(|p| p.0[3] == 255));
        } else {
            let background = request.background;
            assert_eq!(
                corner,
                [background.r, background.g, background.b, background.a],
                "SVG explicit-background corner differs"
            );
            assert!(
                decoded.pixels().any(|p| p.0[..3] != corner[..3]),
                "SVG decoded output lost all fixture content"
            );
        }
        let mut alpha = [0usize; 3];
        for p in decoded.pixels() {
            alpha[if p.0[3] == 0 {
                0
            } else if p.0[3] == 255 {
                2
            } else {
                1
            }] += 1;
        }
        let readback = json!({"decoded_dimensions": [decoded.width(), decoded.height()],
            "decoded_rgba_fnv1a64": fingerprint(decoded.as_raw()), "reference_rgba_fnv1a64": fingerprint(reference.as_raw()),
            "reference_format": extension(reference_format),
            "lossless_exact": if is_svg { Value::Null } else { json!(exact) },
            "jpeg_background_probe": if matches!(request.format, SceneExportFormat::Jpeg { .. }) { json!(true) } else { Value::Null },
            "svg_decoder": if is_svg { json!("resvg 0.46.0; PNG-only embedded resources; file/URL resolver disabled") } else { Value::Null },
            "svg_structure": svg_structure, "svg_png_difference": svg_difference,
            "corner_rgba": corner, "alpha_zero_partial_opaque": alpha});
        Ok(VerifiedExport {
            output,
            decoded,
            readback,
        })
    })();
    // Executed on success AND failure. The export method only receives &self,
    // so it cannot push/pop private editor history. Explicit Undo below also
    // verifies the last application edit's saved baseline after exports.
    assert_eq!(
        view.editor.scene().to_json().unwrap(),
        before,
        "export changed canonical JSON"
    );
    assert_eq!(
        view.editor.engine.revision(),
        revision,
        "export changed engine revision"
    );
    assert_eq!(view.editor.selection, selection, "export changed selection");
    assert_eq!(view.editor.viewport, viewport, "export changed viewport");
    let after_canvas = view
        .resources()
        .and_then(|r| r.canvas_snapshot(&view.editor.engine, handle));
    assert!(
        match (&canvas, &after_canvas) {
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        },
        "export changed owned Canvas snapshot"
    );
    outcome
}

struct Lab {
    view: Entity<SceneView>,
    bridge: Arc<GpuiTextRasterizer>,
    preview: Option<Arc<RenderImage>>,
    preview_label: String,
    status: String,
    sequence: u64,
    bridge_enabled: bool,
    dpr: f32,
    cropped: bool,
    opaque: bool,
    edit_baselines: Vec<String>,
    session: String,
}
impl Lab {
    fn new(bridge: Arc<GpuiTextRasterizer>, cx: &mut Context<Self>) -> Self {
        let cache = FixtureCache::new(cx);
        let view = cx.new(|cx| {
            let mut view = SceneView::new(document(), cx).unwrap();
            view.show_grid = true;
            view.set_editable(false, cx);
            view.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
            view.set_image_resolver(
                Rc::new(|source| {
                    matches!(source, "ready" | "pending" | "failed")
                        .then(|| Resource::Embedded(source.to_owned().into()))
                }),
                cx,
            )
            .unwrap();
            view.set_image_cache(Some(cache.into()), cx).unwrap();
            view
        });
        view.update(cx, |v, cx| {
            let h = v.editor.engine.handle(&"canvas".into()).unwrap();
            v.edit_canvas(
                h,
                |s| s.write_pixels(&canvas_pattern(), 0, 0).map(|_| ()),
                cx,
            )
            .unwrap();
        });
        cx.on_release(|this, cx| {
            if let Some(image) = this.preview.take() {
                cx.drop_image(image, None);
            }
        })
        .detach();
        Self {
            view,
            bridge,
            preview: None,
            preview_label: "No export yet".into(),
            status:
                "Ready fixture resolves on paint. Click PNG, JPEG, WebP or SVG to export and verify."
                    .into(),
            sequence: 0,
            bridge_enabled: true,
            dpr: 1.,
            cropped: false,
            opaque: true,
            edit_baselines: vec![],
            session: format!(
                "{}-{}",
                std::process::id(),
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            ),
        }
    }
    fn request(&self, format: SceneExportFormat) -> SceneExportRequest {
        SceneExportRequest {
            world_bounds: if self.cropped {
                SceneBounds::new(ScenePoint::new(20., 30.), SceneSize::new(360., 240.))
            } else {
                SceneBounds::new(ScenePoint::ZERO, SceneSize::new(WIDTH, HEIGHT))
            },
            scale: 1.,
            pixel_ratio: self.dpr,
            background: if self.opaque {
                BACKGROUND
            } else {
                Color::TRANSPARENT
            },
            format,
        }
    }
    fn source(&self, cx: &App) -> String {
        let node = self
            .view
            .read(cx)
            .editor
            .scene()
            .get(&"image".into())
            .unwrap();
        let Shape::Image(image) = &node.object.shape else {
            unreachable!()
        };
        image.source.clone()
    }
    fn export(&mut self, format: SceneExportFormat, cx: &mut Context<Self>) {
        self.sequence += 1;
        let request = self.request(format);
        let before_preview = self.preview.as_ref().map(|p| p.id);
        let mut receipt = json!({"fixture": "scene-export", "session": self.session, "sequence": self.sequence,
            "format": extension(format), "quality": if matches!(format, SceneExportFormat::Jpeg{..}) { json!(90) } else { Value::Null },
            "world_bounds": [request.world_bounds.origin.x,request.world_bounds.origin.y,request.world_bounds.size.width,request.world_bounds.size.height],
            "scale":request.scale, "pixel_ratio":request.pixel_ratio, "background_rgba":[request.background.r,request.background.g,request.background.b,request.background.a],
            "font_bridge":self.bridge_enabled, "source":self.source(cx), "hash_algorithm":"fnv1a64",
            "preview_is_outside_source":true});
        let directory = std::env::var_os("FLOW_SCENE_EXPORT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("gpui-flow-scene-export"));
        let stem = format!("scene-{}-{:03}", self.session, self.sequence);
        let result = verify(self.view.read(cx), request);
        receipt["scene_revision_canvas_selection_viewport_unchanged"] = json!(true);
        receipt["history_access"] =
            json!("immutable SceneView borrow; Undo verifies explicit edit baseline");
        match result {
            Ok(verified) => {
                receipt["encoded_bytes"] = json!(verified.output.bytes.len());
                receipt["encoded_fnv1a64"] = json!(fingerprint(&verified.output.bytes));
                receipt["readback"] = verified.readback;
                let path = directory.join(format!("{stem}.{}", extension(format)));
                let saved = std::fs::create_dir_all(&directory)
                    .map_err(|e| e.to_string())
                    .and_then(|_| write_new(&path, &verified.output.bytes));
                if let Err(error) = saved {
                    receipt["status"] = json!("save_failed");
                    receipt["error"] = json!(error);
                    self.status =
                        format!("Readback passed; file save failed: {error}. Preview retained.");
                } else {
                    let mut bgra = verified.decoded;
                    for p in bgra.pixels_mut() {
                        p.0.swap(0, 2);
                    }
                    let preview = Arc::new(RenderImage::new(vec![image::Frame::new(bgra)]));
                    if let Some(old) = self.preview.replace(preview) {
                        cx.drop_image(old, None);
                    }
                    self.preview_label = format!(
                        "{} · {} × {} · DPR {}",
                        extension(format).to_uppercase(),
                        verified.output.width,
                        verified.output.height,
                        self.dpr
                    );
                    self.status = format!(
                        "PASS: {} × {}, {} bytes. Decoded verification and scene immutability passed. {}",
                        verified.output.width,
                        verified.output.height,
                        verified.output.bytes.len(),
                        path.display()
                    );
                    receipt["status"] = json!("passed");
                    receipt["file"] = json!(path.to_string_lossy());
                }
            }
            Err(error) => {
                assert_eq!(self.preview.as_ref().map(|p| p.id), before_preview);
                receipt["status"] = json!("export_rejected");
                receipt["error"] = json!(error);
                receipt["last_preview_retained"] = json!(true);
                self.status =
                    format!("Rejected atomically: {error}. Last successful preview retained.");
            }
        }
        let receipt_path = directory.join(format!("{stem}.json"));
        if let Err(error) = std::fs::create_dir_all(&directory)
            .map_err(|e| e.to_string())
            .and_then(|_| write_new(&receipt_path, &serde_json::to_vec_pretty(&receipt).unwrap()))
        {
            self.status
                .push_str(&format!(" Receipt save failed: {error}"));
        }
        eprintln!("{receipt}");
        cx.notify();
    }
    fn change_source(&mut self, source: &'static str, cx: &mut Context<Self>) {
        if self.source(cx) == source {
            return;
        }
        self.edit_baselines
            .push(self.view.read(cx).editor.scene().to_json().unwrap());
        self.view.update(cx, |view, cx| {
            view.editor
                .transaction(|scene| {
                    scene.update_object(&"image".into(), |o| {
                        if let Shape::Image(image) = &mut o.shape {
                            image.source = source.into();
                        }
                    })
                })
                .unwrap();
            cx.notify();
        });
        self.status = format!(
            "Image source: {source}. Export reads only the current resource ticket; render first, then export."
        );
        cx.notify();
    }
    fn edit(&mut self, cx: &mut Context<Self>) {
        self.edit_baselines
            .push(self.view.read(cx).editor.scene().to_json().unwrap());
        self.view.update(cx, |view, cx| {
            view.editor
                .transaction(|scene| {
                    scene.update_object(&"vector".into(), |o| {
                        let orange = Some(Color::rgba(255, 184, 73, 205).into());
                        o.style.fill = if o.style.fill == orange {
                            Some(Color::rgba(245, 90, 170, 205).into())
                        } else {
                            orange
                        };
                    })
                })
                .unwrap();
            cx.notify();
        });
        self.status =
            "Canonical vector color edited. Export, then Undo to verify the same history baseline."
                .into();
        cx.notify();
    }
    fn undo(&mut self, cx: &mut Context<Self>) {
        let Some(expected) = self.edit_baselines.pop() else {
            self.status = "No explicit fixture edit to undo".into();
            cx.notify();
            return;
        };
        self.view.update(cx, |view, cx| {
            assert!(view.editor.undo());
            assert_eq!(
                view.editor.scene().to_json().unwrap(),
                expected,
                "export altered editor history baseline"
            );
            cx.notify();
        });
        self.status =
            "Undo restored the exact pre-edit canonical JSON. Frozen export preview retained."
                .into();
        eprintln!(
            "{}",
            json!({"fixture":"scene-export","kind":"undo","baseline_restored":true,"preview_retained":true})
        );
        cx.notify();
    }
    fn font(&mut self, cx: &mut Context<Self>) {
        self.bridge_enabled = !self.bridge_enabled;
        self.view.update(cx, |view, cx| {
            view.set_text_rasterizer(self.bridge_enabled.then(|| self.bridge.clone()), cx)
                .unwrap()
        });
        self.status = format!(
            "Same-App font bridge {}. Missing bridge rejects the complete export.",
            if self.bridge_enabled {
                "enabled"
            } else {
                "disabled"
            }
        );
        cx.notify();
    }
    fn pan_zoom(&mut self, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            let zoomed = view.editor.viewport.zoom != 1.;
            view.editor.viewport.zoom = if zoomed { 1. } else { 0.8 };
            view.editor.viewport.pan = if zoomed {
                ScenePoint::ZERO
            } else {
                ScenePoint::new(30., 25.)
            };
            cx.notify();
        });
        self.status =
            "Source view pan/zoom changed. Fixed-world export and frozen preview are independent."
                .into();
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let options = format!(
            "DPR {} · {} crop · {} background · font {} · image {}",
            self.dpr,
            if self.cropped {
                "360×240 @20,30"
            } else {
                "440×320 @0,0"
            },
            if self.opaque { "opaque" } else { "transparent" },
            if self.bridge_enabled { "on" } else { "off" },
            self.source(cx)
        );
        let mut preview = div()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x283b4f));
        if let Some(image) = &self.preview {
            preview = preview.child(
                img(image.clone())
                    .size_full()
                    .object_fit(ObjectFit::Contain),
            );
        } else {
            preview = preview.child("Exported pixels appear here");
        }
        div().size_full().flex().flex_col().bg(rgb(0x101927)).text_color(rgb(0xe6edf5))
            .child(div().h(px(50.)).px_3().flex().items_center().gap_2()
                .child(ControlButton::new("png","Export PNG").on_click(cx.listener(|s,_,_,cx|s.export(SceneExportFormat::Png,cx))))
                .child(ControlButton::new("jpeg","JPEG 90").on_click(cx.listener(|s,_,_,cx|s.export(SceneExportFormat::Jpeg{quality:90},cx))))
                .child(ControlButton::new("webp","Lossless WebP").on_click(cx.listener(|s,_,_,cx|s.export(SceneExportFormat::WebPLossless,cx))))
                .child(ControlButton::new("svg","Mixed SVG").on_click(cx.listener(|s,_,_,cx|s.export(SceneExportFormat::Svg,cx))))
                .child(ControlButton::new("dpr","DPR 1 / 2").on_click(cx.listener(|s,_,_,cx|{s.dpr=if s.dpr==1. {2.} else {1.};cx.notify();})))
                .child(ControlButton::new("crop","Full / crop").on_click(cx.listener(|s,_,_,cx|{s.cropped = !s.cropped;cx.notify();})))
                .child(ControlButton::new("background","Opaque / alpha").on_click(cx.listener(|s,_,_,cx|{s.opaque = !s.opaque;cx.notify();}))))
            .child(div().h(px(50.)).px_3().flex().items_center().gap_2()
                .child(ControlButton::new("font","Font bridge").on_click(cx.listener(|s,_,_,cx|s.font(cx))))
                .child(ControlButton::new("ready","Image ready").on_click(cx.listener(|s,_,_,cx|s.change_source("ready",cx))))
                .child(ControlButton::new("pending","Pending").on_click(cx.listener(|s,_,_,cx|s.change_source("pending",cx))))
                .child(ControlButton::new("failed","Failed").on_click(cx.listener(|s,_,_,cx|s.change_source("failed",cx))))
                .child(ControlButton::new("edit","Edit color").on_click(cx.listener(|s,_,_,cx|s.edit(cx))))
                .child(ControlButton::new("undo","Undo edit").on_click(cx.listener(|s,_,_,cx|s.undo(cx))))
                .child(ControlButton::new("panzoom","View pan / zoom").on_click(cx.listener(|s,_,_,cx|s.pan_zoom(cx)))))
            .child(div().h(px(34.)).px_4().text_sm().child(options))
            .child(div().flex_1().min_h_0().flex().gap_3().px_3()
                .child(div().flex_1().min_w_0().flex().flex_col()
                    .child(div().h(px(28.)).child("Actual source SceneView · grid is view-only"))
                    .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone())))
                .child(div().flex_1().min_w_0().flex().flex_col()
                    .child(div().h(px(28.)).child(format!("Decoded file preview · {}",self.preview_label)))
                    .child(preview)))
            .child(div().h(px(130.)).px_4().py_2().text_sm()
                .child("Clipped rotated vector · Unicode Upper + paragraph gap · embedded alpha image · owned Canvas pixels.\nPNG/WebP: exact decoded equality. JPEG: dimensions + explicit background.\nSVG: vector shapes + per-node image/Canvas/text ink; independent resvg preview and pixel-difference receipt.\n")
                .child(self.status.clone()))
    }
}
fn main() {
    let (app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(false));
    app.with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1120.), px(710.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · scene raster and mixed SVG export");
                cx.new(|cx| Lab::new(bridge, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    // Override the GPUI glob's test macro: these are ordinary headless tests.
    use core::prelude::v1::test;
    #[test]
    fn receipt_fingerprint_has_stable_known_vectors() {
        assert_eq!(fingerprint(b""), "cbf29ce484222325");
        assert_eq!(fingerprint(b"hello"), "a430d84680aabd0b");
    }
    #[test]
    fn source_fixture_contains_supported_clipped_text_image_and_canvas_inputs() {
        let scene = document();
        scene.validate().unwrap();
        let Shape::Text(text) = &scene.get(&"text".into()).unwrap().object.shape else {
            panic!("missing text fixture")
        };
        assert_eq!(text.text_case, display::TextCase::Upper);
        assert_eq!(text.para_spacing, 9.);
        assert!(text.content.contains("Straße"));
        let crop = SceneBounds::new(ScenePoint::ZERO, SceneSize::new(WIDTH, HEIGHT));
        let plan = gpui_flow::composite::SceneExportPlan::new(
            &scene,
            SceneExportRequest {
                world_bounds: crop,
                scale: 1.,
                pixel_ratio: 2.,
                background: BACKGROUND,
                format: SceneExportFormat::Png,
            },
            CompositeLimits::default(),
        )
        .unwrap();
        assert_eq!(plan.pixel_size(), [880, 640]);
        assert_eq!(plan.source().images().len(), 1);
        assert_eq!(plan.source().canvases().len(), 1);
        assert_eq!(plan.source().texts().len(), 1);
        assert!(scene.get(&"preview".into()).is_none());
        let rgba = canvas_pattern();
        assert_eq!((rgba.width(), rgba.height()), (100, 70));
        assert_eq!(&rgba.rgba()[..4], &[0, 0, 0, 0]);
    }
    #[test]
    fn svg_difference_receipt_reports_real_deltas_without_an_equality_claim() {
        let actual = image::RgbaImage::from_raw(2, 1, vec![10, 0, 0, 255, 0, 20, 0, 255]).unwrap();
        let reference = image::RgbaImage::from_raw(2, 1, vec![0, 0, 0, 255, 0, 0, 0, 255]).unwrap();
        let difference = pixel_difference(&actual, &reference);
        assert_eq!(difference["changed_pixels"], 2);
        assert_eq!(difference["changed_channels"], 2);
        assert_eq!(difference["max_channel_delta"], 20);
        assert_eq!(difference["sum_absolute_channel_delta"], 30);
        assert_eq!(difference["mean_absolute_channel_delta"], 3.75);
        assert!(
            difference["comparison"]
                .as_str()
                .unwrap()
                .contains("no pixel-equivalence")
        );
    }

    fn synthetic_svg_fixture() -> SceneExport {
        use gpui_flow::composite::{
            ImageInputs, SceneExportPlan, SceneExportPreparation, StaticImage, TextImage,
            TextInputs,
        };
        let limits = CompositeLimits::default();
        let mut images = ImageInputs::default();
        images.insert(
            "image".into(),
            Ok(Arc::new(
                StaticImage::from_straight_bgra(1, 1, &[0, 0, 255, 255], limits).unwrap(),
            )),
        );
        images.insert(
            "canvas".into(),
            Ok(Arc::new(
                StaticImage::from_straight_bgra(1, 1, &[0, 255, 0, 255], limits).unwrap(),
            )),
        );
        // This headless decoder test supplies synthetic ink, not a native font
        // acceptance claim. Native clicks exercise the installed same-App bridge.
        let mut text = TextInputs::default();
        text.insert(
            "text".into(),
            Ok(Some(Arc::new(TextImage {
                image: Arc::new(
                    StaticImage::from_straight_bgra(1, 1, &[0, 0, 0, 255], limits).unwrap(),
                ),
                local_bounds: SceneBounds::new(ScenePoint::ZERO, SceneSize::new(252., 104.)),
            }))),
        );
        SceneExportPlan::new(
            &document(),
            SceneExportRequest {
                world_bounds: SceneBounds::new(ScenePoint::ZERO, SceneSize::new(WIDTH, HEIGHT)),
                scale: 1.,
                pixel_ratio: 1.,
                background: BACKGROUND,
                format: SceneExportFormat::Svg,
            },
            limits,
        )
        .unwrap()
        .encode_with_inputs(
            &images,
            &text,
            SceneExportPreparation::default(),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap()
    }

    #[test]
    fn svg_fixture_receipt_proves_vectors_and_separate_raster_producers() {
        let output = synthetic_svg_fixture();
        let receipt = svg_structure(&output).unwrap();
        assert_eq!(extension(output.request.format), "svg");
        assert_eq!(receipt["mixed_svg_not_whole_scene_bitmap"], true);
        assert_eq!(receipt["embedded_png_count"], 3);
        assert_eq!(receipt["vector_producer_painted_paths"], 1);
        assert_eq!(receipt["editable_text_nodes"], 0);
        let decoded = decode(&output).unwrap();
        assert_eq!(decoded.dimensions(), (440, 320));
        assert_eq!(decoded.get_pixel(0, 0).0, [232, 238, 247, 255]);
        assert_eq!(decoded.get_pixel(335, 100).0, [255, 0, 0, 255]);
        assert_eq!(decoded.get_pixel(330, 220).0, [0, 255, 0, 255]);
        assert_eq!(decoded.get_pixel(80, 260).0, [0, 0, 0, 255]);
    }

    #[test]
    fn svg_fixture_rejects_external_resource_or_whole_scene_bitmap_claims() {
        let mut output = synthetic_svg_fixture();
        let original = String::from_utf8(output.bytes.clone()).unwrap();
        output.bytes = original
            .replacen(
                "data:image/png;base64,",
                "file:///unavailable-fixture.png?",
                1,
            )
            .into_bytes();
        assert!(
            svg_structure(&output)
                .unwrap_err()
                .contains("external/non-PNG")
        );
        output.bytes = original
            .replacen("data-flow-id=\"vector\"", "data-flow-id=\"missing\"", 1)
            .into_bytes();
        assert!(
            svg_structure(&output)
                .unwrap_err()
                .contains("canonical vector")
        );
    }
}
