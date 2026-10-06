//! Application-owned bridge to Scene primitives and its existing compositor.
//! Geometry stays in node-local world units; only presentation uses zoom.
use super::*;
use crate::shapes::{
    MAX_SHAPE_SIZE, MAX_SHAPE_STROKE_WIDTH, SHAPE_NODE_KIND, ShapeData, ShapeKind,
};
use gpui_flow::{
    composite::{self, CompositeLimits, CpuFrame, PixelBudget, RasterViewport},
    display::{DisplayElement, RenderObject},
};
use gpui_flow_scene::{self as scene, Affine, Color, DisplayObject, Shape, Stroke, Style};
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_PREVIEWS: usize = 32;
const MAX_RASTER_EDGE: u32 = 1024;
const MAX_RASTER_PIXELS: u64 = MAX_RASTER_EDGE as u64 * MAX_RASTER_EDGE as u64;

/// At most thirty-two retained 4 MiB images and one 16 MiB compositor working set.
/// There is one worker per owner, not one blocked worker per request. Pending
/// requests occupy the same fixed cache slots and replace their predecessor.
#[derive(Default)]
pub(super) struct ShapeViews(Arc<Mutex<PreviewCache>>);

impl ShapeViews {
    pub(super) fn retain(&self, graph: &gpui_flow_core::Graph) {
        let hidden = gpui_flow_core::hidden_node_ids(graph);
        let mut cache = self.0.lock().unwrap();
        cache.entries.retain(|id, _| {
            !hidden.contains(id)
                && graph.node(id).is_some_and(|node| match node.kind.as_str() {
                    SHAPE_NODE_KIND => ShapeData::from_node(node).is_ok_and(needs_composite),
                    crate::pencil::PENCIL_NODE_KIND => crate::pencil::PencilData::from_node(node)
                        .is_ok_and(|d| needs_pencil_composite(&d)),
                    _ => false,
                })
        });
        if let Some(job) = &cache.running
            && !cache.entries.contains_key(&job.id)
        {
            job.cancelled.store(true, Ordering::Release);
        }
    }

    pub(super) fn render(&self, node: &Node, zoom: f32, cx: &mut App) -> AnyElement {
        let shape = match ShapeData::from_node(node) {
            Ok(shape) => shape,
            Err(error) => {
                self.remove(&node.id);
                return message(format!("形状数据无效：{error}"));
            }
        };
        if !zoom.is_finite()
            || zoom <= 0.
            || !(node.size.width * zoom).is_finite()
            || !(node.size.height * zoom).is_finite()
        {
            self.remove(&node.id);
            return message("形状缩放无效");
        }
        let object = display_object(node, shape);
        if shape.style.opacity == 0.
            || (object.style.fill.is_none() && object.style.stroke.is_none())
        {
            self.remove(&node.id);
            return div().size_full().into_any_element();
        }
        if !needs_composite(shape) {
            self.remove(&node.id);
            return div()
                .relative()
                .size_full()
                .child(DisplayElement::new(native_object(object, zoom)))
                .into_any_element();
        }

        self.render_composited(node, object, zoom, cx)
    }

    pub(super) fn render_pencil(&self, node: &Node, zoom: f32, cx: &mut App) -> AnyElement {
        let Ok(data) = crate::pencil::PencilData::from_node(node) else {
            return message("笔迹数据无效");
        };
        if !needs_pencil_composite(&data) {
            self.remove(&node.id);
            return pencil_preview::render(node, zoom);
        }
        let Ok(object) = pencil_preview::object_for_node(node) else {
            return message("笔迹几何无效");
        };
        self.render_composited(node, object, zoom, cx)
    }

    fn render_composited(
        &self,
        node: &Node,
        object: DisplayObject,
        zoom: f32,
        cx: &mut App,
    ) -> AnyElement {
        let request = PreviewRequest::new(object, zoom);
        let (view, clear, changed) = {
            let mut cache = self.0.lock().unwrap();
            if !cache.entries.contains_key(&node.id) && cache.entries.len() >= MAX_PREVIEWS {
                return message("最多同时预览 32 个半透明矢量对象；隐藏其他对象可释放预览");
            }
            let changed = cache
                .entries
                .get(&node.id)
                .is_none_or(|entry| entry.desired != request);
            let clear = cache
                .entries
                .get(&node.id)
                .is_none_or(|entry| entry.desired.object != request.object);
            if changed {
                cache.generation = cache.generation.wrapping_add(1);
                let generation = cache.generation;
                if let Some(job) = &cache.running
                    && job.id == node.id
                {
                    job.cancelled.store(true, Ordering::Release);
                }
                let entry = cache
                    .entries
                    .entry(node.id.clone())
                    .or_insert_with(|| Entry {
                        view: cx.new(ShapePreview::new),
                        desired: request.clone(),
                        generation,
                        completed: None,
                    });
                entry.desired = request;
                entry.generation = generation;
            }
            (cache.entries[&node.id].view.clone(), clear, changed)
        };
        view.update(cx, |view, cx| {
            let moved = view.zoom != zoom;
            view.zoom = zoom;
            // A zoom-only refresh can reuse the old complete image at its exact
            // world placement. A content change must not expose an old alpha.
            if clear {
                view.clear(cx);
            }
            if changed {
                view.error = None;
            }
            if moved || changed {
                cx.notify();
            }
        });
        start(&self.0, cx);
        view.into_any_element()
    }

    fn remove(&self, id: &NodeId) {
        let mut cache = self.0.lock().unwrap();
        cache.entries.remove(id);
        if let Some(job) = &cache.running
            && &job.id == id
        {
            job.cancelled.store(true, Ordering::Release);
        }
    }
}

fn needs_pencil_composite(data: &crate::pencil::PencilData) -> bool {
    data.style.opacity > 0.
        && data.style.opacity < 1.
        && data.points.windows(2).any(|points| points[0] != points[1])
}

fn needs_composite(shape: ShapeData) -> bool {
    shape.style.opacity > 0.
        && shape.style.opacity < 1.
        && shape.kind != ShapeKind::Line
        && shape.style.fill.is_some()
        && shape.style.stroke.is_some()
        && shape.style.stroke_width > 0.
}

/// The host supplies node-local world coordinates and a world-unit tolerance.
/// A cheap conservative envelope rejects distant nodes before payload parsing.
pub(super) fn hit_shape(node: &Node, local: WorldPoint, tolerance: f32) -> bool {
    if node.kind != SHAPE_NODE_KIND
        || !local.is_finite()
        || !node.size.is_positive()
        || !tolerance.is_finite()
        || tolerance < 0.
    {
        return false;
    }
    let stroke_width = node
        .data
        .get("shape")
        .and_then(|shape| shape.get("style"))
        .and_then(|style| style.get("stroke_width"))
        .and_then(Value::as_f64)
        .filter(|width| {
            width.is_finite() && (0. ..=f64::from(MAX_SHAPE_STROKE_WIDTH)).contains(width)
        })
        .unwrap_or(f64::from(MAX_SHAPE_STROKE_WIDTH)) as f32;
    let envelope =
        scene::Bounds::new(WorldPoint::ZERO, node.size).expanded(stroke_width * 0.5 + tolerance);
    if !envelope.is_valid() || !envelope.contains(local) {
        return false;
    }
    let Ok(shape) = ShapeData::from_node(node) else {
        return false;
    };
    if shape.style.opacity == 0. {
        return false;
    }
    let object = display_object(node, shape);
    object.shape.hit_test(local, &object.style, tolerance)
}

fn display_object(node: &Node, shape: ShapeData) -> DisplayObject {
    let size = node.size;
    let primitive: Shape = match shape.kind {
        ShapeKind::Rectangle => scene::display::Rect::new(size.width, size.height)
            .rounded(
                shape
                    .style
                    .radius
                    .min(size.width * 0.5)
                    .min(size.height * 0.5),
            )
            .into(),
        ShapeKind::Ellipse => scene::display::Ellipse::new(size.width, size.height).into(),
        ShapeKind::Line => {
            let line = shape.line.expect("validated normalized line endpoints");
            let local =
                |point: WorldPoint| WorldPoint::new(point.x * size.width, point.y * size.height);
            scene::display::Line::new(local(line.start), local(line.end)).into()
        }
    };
    DisplayObject::new(node.id.as_str(), primitive).with_style(Style {
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

fn native_object(object: DisplayObject, zoom: f32) -> RenderObject {
    let transform = Affine::scale(zoom, zoom);
    RenderObject {
        // Native painting consumes resolved opacity; retain the canonical style
        // so diagnostics still reject a multi-paint compositing domain.
        opacity: object.style.opacity,
        object,
        screen_transform: transform,
        viewport_transform: transform,
        clips: Vec::new(),
    }
}

#[derive(Clone, PartialEq)]
struct PreviewRequest {
    object: DisplayObject,
    raster_scale: f32,
}
impl PreviewRequest {
    fn new(object: DisplayObject, zoom: f32) -> Self {
        let bounds = object.paint_bounds().expect("validated primitive bounds");
        let edge = bounds.size.width.max(bounds.size.height);
        // Two pixels per logical pixel when space allows. Leave four physical
        // pixels for outward rounding and the compositor's antialias guard.
        let raster_scale = (zoom as f64 * 2.)
            .max(1. / f64::from(MAX_SHAPE_SIZE))
            .min(f64::from(MAX_RASTER_EDGE - 4) / f64::from(edge))
            as f32;
        Self {
            object,
            raster_scale,
        }
    }
}

struct Entry {
    view: Entity<ShapePreview>,
    desired: PreviewRequest,
    generation: u64,
    completed: Option<u64>,
}
#[derive(Default)]
struct PreviewCache {
    entries: BTreeMap<NodeId, Entry>,
    generation: u64,
    running: Option<Job>,
}
impl Drop for PreviewCache {
    fn drop(&mut self) {
        if let Some(job) = &self.running {
            job.cancelled.store(true, Ordering::Release);
        }
    }
}
#[derive(Clone)]
struct Job {
    id: NodeId,
    generation: u64,
    cancelled: Arc<AtomicBool>,
}

fn start(owner: &Arc<Mutex<PreviewCache>>, cx: &mut App) {
    let (job, request) = {
        let mut cache = owner.lock().unwrap();
        if cache.running.is_some() {
            return;
        }
        let Some((id, entry)) = cache
            .entries
            .iter()
            .filter(|(_, entry)| entry.completed != Some(entry.generation))
            .min_by_key(|(_, entry)| entry.generation)
        else {
            return;
        };
        let job = Job {
            id: id.clone(),
            generation: entry.generation,
            cancelled: Arc::new(AtomicBool::new(false)),
        };
        let request = entry.desired.clone();
        cache.running = Some(job.clone());
        (job, request)
    };
    let owner = Arc::downgrade(owner);
    let cancelled = job.cancelled.clone();
    cx.spawn(async move |cx| {
        let result = cx
            .background_executor()
            .spawn(async move {
                raster_frame(&request, &cancelled)?
                    .map(|frame| into_pixels(frame, request.raster_scale))
                    .transpose()
            })
            .await;
        let Some(owner) = owner.upgrade() else {
            return;
        };
        cx.update(|cx| {
            let view = {
                let mut cache = owner.lock().unwrap();
                cache.running = None;
                cache.entries.get_mut(&job.id).and_then(|entry| {
                    // Generation, not value equality: A/B/A edits and a removed
                    // then re-created ID cannot accept the old A job.
                    if entry.generation == job.generation && !job.cancelled.load(Ordering::Acquire)
                    {
                        entry.completed = Some(job.generation);
                        Some(entry.view.clone())
                    } else {
                        None
                    }
                })
            };
            if let Some(view) = view {
                view.update(cx, |view, cx| {
                    view.clear(cx);
                    match result {
                        Ok(Some(pixels)) => {
                            view.image = Some((
                                pixels.world_bounds,
                                Arc::new(RenderImage::new(vec![image::Frame::new(pixels.bgra)])),
                            ));
                            view.error = None;
                        }
                        Ok(None) => view.error = None,
                        Err(error) => view.error = Some(error),
                    }
                    cx.notify();
                });
            }
            start(&owner, cx);
        });
    })
    .detach();
}

fn limits() -> CompositeLimits {
    CompositeLimits {
        max_dimension: MAX_RASTER_EDGE,
        max_pixels: MAX_RASTER_PIXELS,
        max_surface_bytes: 16 * 1024 * 1024,
        max_surface_writes: MAX_RASTER_PIXELS * 8,
        max_nodes: 1,
        max_domains: 1,
        ..Default::default()
    }
}

fn raster_frame(
    request: &PreviewRequest,
    cancelled: &AtomicBool,
) -> Result<Option<CpuFrame>, String> {
    if cancelled.load(Ordering::Acquire) {
        return Err(composite::CompositeError::Superseded.to_string());
    }
    let mut scene = scene::Scene::new();
    scene
        .insert(request.object.clone(), None, None)
        .map_err(|error| error.to_string())?;
    let source = composite::capture_subtree(&scene, &request.object.id, limits())
        .map_err(|error| error.to_string())?;
    let camera = Affine::scale(request.raster_scale, request.raster_scale);
    let bounds = camera
        .transform_bounds(
            request
                .object
                .paint_bounds()
                .expect("validated primitive bounds"),
        )
        .expanded(1.);
    composite::rasterize(
        &source,
        RasterViewport {
            world_to_screen: camera,
            canvas_bounds: bounds,
            content_mask: bounds,
            device_scale: 1.,
        },
        &mut PixelBudget::new(limits(), 0).map_err(|error| error.to_string())?,
        cancelled,
    )
    .map_err(|error| error.to_string())
}

struct PreviewPixels {
    world_bounds: scene::Bounds,
    bgra: image::RgbaImage,
}
fn into_pixels(frame: CpuFrame, scale: f32) -> Result<PreviewPixels, String> {
    if frame.width > MAX_RASTER_EDGE
        || frame.height > MAX_RASTER_EDGE
        || frame.premul_rgba.len() != frame.width as usize * frame.height as usize * 4
    {
        return Err("形状预览尺寸无效".into());
    }
    let world_bounds = scene::Bounds::new(
        frame.screen_bounds.origin / scale,
        WorldSize::new(
            frame.screen_bounds.size.width / scale,
            frame.screen_bounds.size.height / scale,
        ),
    );
    let mut pixels = frame.premul_rgba;
    // Same boundary conversion as FlowUI's compositor: premultiplied RGBA to
    // straight BGRA exactly once, without a second alpha multiplication.
    for pixel in pixels.as_chunks_mut::<4>().0 {
        gpui::swap_rgba_pa_to_bgra(pixel);
    }
    let bgra = image::RgbaImage::from_raw(frame.width, frame.height, pixels)
        .ok_or_else(|| "形状预览像素无效".to_string())?;
    Ok(PreviewPixels { world_bounds, bgra })
}

struct ShapePreview {
    zoom: f32,
    image: Option<(scene::Bounds, Arc<RenderImage>)>,
    error: Option<String>,
}
impl ShapePreview {
    fn new(cx: &mut Context<Self>) -> Self {
        cx.on_release(|view: &mut Self, cx| view.clear(cx)).detach();
        Self {
            zoom: 1.,
            image: None,
            error: None,
        }
    }
    fn clear(&mut self, cx: &mut App) {
        if let Some((_, image)) = self.image.take() {
            cx.defer(move |cx| cx.drop_image(image, None));
        }
    }
}
impl Render for ShapePreview {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .relative()
            .size_full()
            .when_some(self.image.clone(), |body, (bounds, image)| {
                body.child(
                    img(image)
                        .absolute()
                        .left(px(bounds.origin.x * self.zoom))
                        .top(px(bounds.origin.y * self.zoom))
                        .w(px(bounds.size.width * self.zoom))
                        .h(px(bounds.size.height * self.zoom)),
                )
            })
            .when_some(self.error.clone(), |body, error| {
                body.child(message(format!("形状预览失败：{error}")))
            })
    }
}

fn message(text: impl Into<SharedString>) -> AnyElement {
    div()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .p_2()
        .text_xs()
        .text_color(rgb(0x8d7891))
        .child(text.into())
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shapes::{ShapeLine, create_shape_node};
    use ::core::prelude::v1::test;

    fn node(shape: ShapeData, size: WorldSize) -> Node {
        create_shape_node("shape", "形状", WorldPoint::new(80., 120.), size, shape).unwrap()
    }

    #[test]
    fn pencil_self_overlap_composites_opacity_once() {
        let node = crate::pencil::create_pencil_node_from_points(
            "stroke",
            "stroke",
            &[
                WorldPoint::new(0., 0.),
                WorldPoint::new(80., 80.),
                WorldPoint::new(0., 80.),
                WorldPoint::new(80., 0.),
            ],
            crate::pencil::PencilStyle {
                color: 0x3b82f6,
                stroke_width: 12.,
                opacity: 0.5,
            },
        )
        .unwrap();
        let data = crate::pencil::PencilData::from_node(&node).unwrap();
        assert!(needs_pencil_composite(&data));
        let request = PreviewRequest::new(pencil_preview::object_for_node(&node).unwrap(), 1.);
        let frame = raster_frame(&request, &AtomicBool::new(false))
            .unwrap()
            .unwrap();
        assert_eq!(
            frame
                .premul_rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| p[3])
                .max(),
            Some(128)
        );
        assert!(
            frame
                .premul_rgba
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] == 128)
                .count()
                > 100
        );
    }

    #[test]
    fn geometry_preserves_world_size_radius_and_line_directions() {
        let size = WorldSize::new(240., 80.);
        let mut shape = ShapeData::new(ShapeKind::Rectangle);
        shape.style.radius = 8192.;
        let rectangle = display_object(&node(shape, size), shape);
        let Shape::Rect(rectangle) = rectangle.shape else {
            panic!("rectangle")
        };
        assert_eq!(rectangle.size, size);
        assert_eq!(rectangle.corner_radius, 40.);
        shape.kind = ShapeKind::Ellipse;
        let ellipse_node = node(shape, size);
        assert!(!hit_shape(&ellipse_node, WorldPoint::ZERO, 0.));
        assert!(hit_shape(&ellipse_node, WorldPoint::new(120., 40.), 0.));
        let ellipse = display_object(&ellipse_node, shape);
        let Shape::Ellipse(ellipse) = ellipse.shape else {
            panic!("ellipse")
        };
        assert_eq!(ellipse.size, size);
        assert!(ellipse.is_default_oval());

        shape.kind = ShapeKind::Line;
        for (start, end, expected_start, expected_end) in [
            ((0., 0.5), (1., 0.5), (0., 40.), (240., 40.)),
            ((0.5, 0.), (0.5, 1.), (120., 0.), (120., 80.)),
            ((1., 1.), (0., 0.), (240., 80.), (0., 0.)),
        ] {
            shape.line = Some(ShapeLine {
                start: WorldPoint::new(start.0, start.1),
                end: WorldPoint::new(end.0, end.1),
            });
            let line_node = node(shape, size);
            assert!(hit_shape(&line_node, WorldPoint::new(120., 40.), 0.));
            assert!(!hit_shape(&line_node, WorldPoint::new(20., 70.), 0.));
            assert!(!hit_shape(
                &line_node,
                WorldPoint::new(10_000., 10_000.),
                4.
            ));
            let rendered = native_object(display_object(&line_node, shape), 3.);
            assert!(rendered.object.style.fill.is_none());
            assert!(rendered.diagnostics().is_empty());
            assert_eq!(rendered.screen_transform, Affine::scale(3., 3.));
            let Shape::Line(line) = rendered.object.shape else {
                panic!("line")
            };
            assert_eq!(
                line.start,
                WorldPoint::new(expected_start.0, expected_start.1)
            );
            assert_eq!(line.end, WorldPoint::new(expected_end.0, expected_end.1));
        }
    }

    fn at(frame: &CpuFrame, x: f32, y: f32) -> [u8; 4] {
        let x = (x - frame.screen_bounds.origin.x) as u32;
        let y = (y - frame.screen_bounds.origin.y) as u32;
        let offset = ((y * frame.width + x) * 4) as usize;
        frame.premul_rgba[offset..offset + 4].try_into().unwrap()
    }

    #[test]
    fn translucent_fill_and_stroke_overlap_gets_opacity_only_once() {
        for kind in [ShapeKind::Rectangle, ShapeKind::Ellipse] {
            let mut shape = ShapeData::new(kind);
            shape.style.fill = Some(0xff0000);
            shape.style.stroke = Some(0x0000ff);
            shape.style.stroke_width = 12.;
            shape.style.opacity = 0.5;
            let object = display_object(&node(shape, WorldSize::new(64., 48.)), shape);
            assert!(needs_composite(shape));
            assert!(
                native_object(object.clone(), 1.)
                    .diagnostics()
                    .iter()
                    .any(|issue| {
                        issue.feature == gpui_flow::display::UnsupportedFeature::CompositingDomain
                    })
            );
            let request = PreviewRequest::new(object, 0.5);
            assert_eq!(request.raster_scale, 1.);
            let frame = raster_frame(&request, &AtomicBool::new(false))
                .unwrap()
                .unwrap();
            assert_eq!(at(&frame, 32., 24.), [128, 0, 0, 128]);
            assert_eq!(at(&frame, 2., 24.), [0, 0, 128, 128]);
            assert_eq!(at(&frame, -3., 24.), [0, 0, 128, 128]);
            let origin = frame.screen_bounds.origin;
            let pixels = into_pixels(frame, request.raster_scale).unwrap();
            let overlap = pixels
                .bgra
                .get_pixel((2. - origin.x) as u32, (24. - origin.y) as u32);
            // GPUI's established integer unpremultiply may round 255 to 254.
            assert!(overlap.0[0].abs_diff(255) <= 1);
            assert_eq!(&overlap.0[1..], &[0, 0, 128]);
        }
    }

    #[test]
    fn largest_shape_keeps_world_extent_with_bounded_pixels_and_cancellation() {
        let mut shape = ShapeData::new(ShapeKind::Rectangle);
        shape.style.opacity = 0.5;
        shape.style.stroke_width = 256.;
        let object = display_object(&node(shape, WorldSize::new(16_384., 16_384.)), shape);
        let bounds = object.paint_bounds().unwrap();
        let request = PreviewRequest::new(object, 64.);
        let frame = raster_frame(&request, &AtomicBool::new(false))
            .unwrap()
            .unwrap();
        assert!(frame.width <= MAX_RASTER_EDGE && frame.height <= MAX_RASTER_EDGE);
        assert!(frame.premul_rgba.len() as u64 <= MAX_RASTER_PIXELS * 4);
        let pixels = into_pixels(frame, request.raster_scale).unwrap();
        assert!(pixels.world_bounds.contains_rect(bounds));
        assert_eq!(bounds.origin, WorldPoint::new(-128., -128.));
        assert_eq!(bounds.size, WorldSize::new(16_640., 16_640.));
        let cancelled = AtomicBool::new(true);
        assert_eq!(
            raster_frame(&request, &cancelled).unwrap_err(),
            "Superseded"
        );
    }
}
