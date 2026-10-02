//! Self-contained mixed SVG: vector geometry, host-owned image/Canvas/text ink.
//! Never fetches source URLs, names a font, or rasterizes the complete scene.
use crate::{
    CompositeError, CompositeLimits, DomainNode, DomainSource, ImageInputs, SceneExport,
    SceneExportPlan, SceneExportPreparation, StaticImage, TextInputs,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use gpui_flow_scene::{
    Affine, BlendMode, Bounds, Color, FillRule, MaskMode, Paint, Shape, Size,
    display::{Path, PathCommand},
};
use std::{
    fmt::{self, Write as _},
    sync::atomic::{AtomicBool, Ordering},
};

// The existing PNG implementation admits this codec/scratch allowance.
const PNG_SCRATCH: u64 = 8 * 1024 * 1024;

/// Check unsupported semantics before a host prepares resources. A hidden mask
/// still changes following siblings, so its marker cannot be silently skipped.
pub(crate) fn validate(source: &DomainSource) -> Result<(), CompositeError> {
    fn node(n: &DomainNode) -> Result<(), CompositeError> {
        let o = &n.object;
        if o.mask != MaskMode::None {
            return Err(CompositeError::UnsupportedShape("SVG alpha masks".into()));
        }
        if !o.visible || o.style.opacity == 0. {
            return Ok(());
        }
        if !matches!(
            o.style.blend_mode,
            BlendMode::Normal | BlendMode::PassThrough
        ) {
            return Err(CompositeError::UnsupportedShape("SVG blend mode".into()));
        }
        if o.style.has_visible_shadows() {
            return Err(CompositeError::UnsupportedShadow(o.id.clone()));
        }
        if o.style.has_visible_inner_shadows() {
            return Err(CompositeError::UnsupportedInnerShadow(o.id.clone()));
        }
        if matches!(o.shape, Shape::Custom(_)) {
            return Err(CompositeError::UnsupportedShape("SVG Custom".into()));
        }
        if matches!(o.shape, Shape::Text(_))
            && (o.style.strokes().next().is_some()
                || o.style.fills().count() > 1
                || o.style.fills().any(|p| !matches!(p, Paint::Solid { .. })))
        {
            return Err(CompositeError::UnsupportedTextStyle(o.id.clone()));
        }
        for child in &n.children {
            node(child)?;
        }
        Ok(())
    }
    node(&source.root)
}

pub(crate) fn encode(
    plan: &SceneExportPlan,
    images: &ImageInputs,
    texts: &TextInputs,
    preparation: SceneExportPreparation,
    cancelled: &AtomicBool,
) -> Result<SceneExport, CompositeError> {
    validate(plan.source())?;
    let retained = preparation
        .retained_bytes
        .saturating_add(images.byte_len())
        .saturating_add(texts.byte_len());
    let mut encoder = Encoder {
        output: Output::new(plan.limits(), retained, preparation.pixel_work, cancelled)?,
        images,
        texts,
        next_id: 0,
        points: 0,
        triangles: 0,
    };
    let request = plan.request();
    let [width, height] = plan.pixel_size();
    encoder.output.emit(format_args!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" width=\"{width}\" height=\"{height}\" viewBox=\"0 0 {width} {height}\" overflow=\"hidden\" color-interpolation=\"sRGB\">"
    ))?;
    encoder.output.emit(format_args!("<g transform=\""))?;
    encoder.matrix(plan.viewport().world_to_screen)?;
    encoder.output.emit(format_args!("\">"))?;
    let clip = encoder.rect_clip(request.world_bounds)?;
    encoder
        .output
        .emit(format_args!("<g clip-path=\"url(#s{clip})\">"))?;
    if request.background.a != 0 {
        encoder.output.emit(format_args!("<rect "))?;
        // The background covers the ceil-rounded surface, then the exact crop
        // clips it once. Painting just the fractional crop rectangle here would
        // apply its antialias coverage twice at the final row/column.
        let scale = f64::from(plan.viewport().world_to_screen.a);
        let background = Bounds::new(
            request.world_bounds.origin,
            Size::new(
                (f64::from(width) / scale) as f32,
                (f64::from(height) / scale) as f32,
            ),
        );
        encoder.rect_attributes(background)?;
        encoder.solid_attributes(request.background)?;
        encoder.output.emit(format_args!("/>"))?;
    }
    for child in &plan.source().root.children {
        encoder.node(child)?;
    }
    encoder.output.emit(format_args!("</g></g></svg>"))?;
    encoder.output.check()?;
    Ok(SceneExport {
        bytes: encoder.output.bytes,
        width,
        height,
        request,
    })
}

struct Encoder<'a> {
    output: Output<'a>,
    images: &'a ImageInputs,
    texts: &'a TextInputs,
    next_id: usize,
    points: usize,
    triangles: usize,
}
impl Encoder<'_> {
    fn id(&mut self) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        id
    }
    fn matrix(&mut self, m: Affine) -> Result<(), CompositeError> {
        if m.inverse().is_none() {
            return Err(CompositeError::UnsupportedTransform);
        }
        self.output.emit(format_args!(
            "matrix({} {} {} {} {} {})",
            number(m.a)?,
            number(m.b)?,
            number(m.c)?,
            number(m.d)?,
            number(m.tx)?,
            number(m.ty)?
        ))
    }
    fn rect_attributes(&mut self, b: Bounds) -> Result<(), CompositeError> {
        if !b.is_valid() || !b.size.is_positive() {
            return Err(CompositeError::InvalidGeometry("SVG rectangle".into()));
        }
        self.output.emit(format_args!(
            "x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"",
            coordinate(b.origin.x)?,
            coordinate(b.origin.y)?,
            coordinate(b.size.width)?,
            coordinate(b.size.height)?
        ))
    }
    fn solid_attributes(&mut self, c: Color) -> Result<(), CompositeError> {
        self.output.emit(format_args!(
            " fill=\"#{:02x}{:02x}{:02x}\" fill-opacity=\"{}\"",
            c.r,
            c.g,
            c.b,
            f32::from(c.a) / 255.
        ))
    }
    fn paint(&mut self, paint: &Paint) -> Result<Option<usize>, CompositeError> {
        let (tag, stops) = match paint {
            Paint::Solid { .. } => return Ok(None),
            Paint::LinearGradient { stops, .. } => ("linearGradient", stops),
            Paint::RadialGradient { stops, .. } => ("radialGradient", stops),
        };
        let id = self.id();
        self.output.emit(format_args!(
            "<defs><{tag} id=\"s{id}\" gradientUnits=\"userSpaceOnUse\" spreadMethod=\"pad\""
        ))?;
        match paint {
            Paint::LinearGradient { start, end, .. } => self.output.emit(format_args!(
                " x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"",
                coordinate(start.x)?,
                coordinate(start.y)?,
                coordinate(end.x)?,
                coordinate(end.y)?
            ))?,
            Paint::RadialGradient { center, radius, .. } => self.output.emit(format_args!(
                " cx=\"{}\" cy=\"{}\" r=\"{}\"",
                coordinate(center.x)?,
                coordinate(center.y)?,
                coordinate(*radius)?
            ))?,
            Paint::Solid { .. } => unreachable!(),
        }
        self.output.emit(format_args!(">"))?;
        for stop in stops {
            let c = stop.color;
            self.output.emit(format_args!(
                "<stop offset=\"{}\" stop-color=\"#{:02x}{:02x}{:02x}\" stop-opacity=\"{}\"/>",
                number(stop.offset)?,
                c.r,
                c.g,
                c.b,
                f32::from(c.a) / 255.
            ))?;
        }
        self.output.emit(format_args!("</{tag}></defs>"))?;
        Ok(Some(id))
    }
    fn paint_attributes(&mut self, paint: &Paint, id: Option<usize>) -> Result<(), CompositeError> {
        match paint {
            Paint::Solid { color } => self.solid_attributes(*color),
            _ => self.output.emit(format_args!(
                " fill=\"url(#s{})\"",
                id.expect("gradient id")
            )),
        }
    }
    fn path_paint(
        &mut self,
        data: &str,
        paint: &Paint,
        rule: FillRule,
    ) -> Result<(), CompositeError> {
        let id = self.paint(paint)?;
        self.output.emit(format_args!("<path d=\"{data}\""))?;
        self.paint_attributes(paint, id)?;
        let rule = if rule == FillRule::EvenOdd {
            "evenodd"
        } else {
            "nonzero"
        };
        self.output.emit(format_args!(" fill-rule=\"{rule}\"/>"))
    }
    fn path_clip(&mut self, data: &str) -> Result<usize, CompositeError> {
        let id = self.id();
        self.output.emit(format_args!(
            "<defs><clipPath id=\"s{id}\" clipPathUnits=\"userSpaceOnUse\"><path d=\"{data}\"/></clipPath></defs>"
        ))?;
        Ok(id)
    }
    fn rect_clip(&mut self, b: Bounds) -> Result<usize, CompositeError> {
        let id = self.id();
        self.output.emit(format_args!(
            "<defs><clipPath id=\"s{id}\" clipPathUnits=\"userSpaceOnUse\"><rect "
        ))?;
        self.rect_attributes(b)?;
        self.output.emit(format_args!("/></clipPath></defs>"))?;
        Ok(id)
    }
    fn node(&mut self, node: &DomainNode) -> Result<(), CompositeError> {
        self.output.check()?;
        let o = &node.object;
        if !o.visible || o.style.opacity == 0. {
            return Ok(());
        }
        self.output.emit(format_args!("<g data-flow-id=\""))?;
        self.output.attribute(o.id.as_str())?;
        self.output.emit(format_args!("\" transform=\""))?;
        self.matrix(o.transform)?;
        self.output
            .emit(format_args!("\" opacity=\"{}\">", number(o.style.opacity)?))?;
        match &o.shape {
            Shape::Text(_) => self.text(node)?,
            Shape::Image(image) => {
                let pixels = self.images.get(&o.id)?;
                let mapping = image
                    .mapping(Size::new(pixels.width() as f32, pixels.height() as f32))
                    .ok_or(CompositeError::InvalidImage)?;
                let path = image
                    .rect()
                    .to_path()
                    .map_err(CompositeError::InvalidGeometry)?;
                let data = self.generated_path_data(path)?;
                let clip = self.path_clip(&data)?;
                let content = self.rect_clip(mapping.destination)?;
                self.output.emit(format_args!(
                    "<g clip-path=\"url(#s{clip})\"><g clip-path=\"url(#s{content})\">"
                ))?;
                self.image(&pixels, mapping.full_source, true, "image")?;
                self.output.emit(format_args!("</g></g>"))?;
                self.output.release(data.capacity() as u64);
            }
            Shape::Group(_) => self.children(node, None)?,
            _ => {
                let data = self.shape_data(&o.shape)?;
                if let Some(data) = &data {
                    for paint in o.style.fills().filter(|p| p.is_visible()) {
                        self.path_paint(data, paint, o.style.fill_rule)?;
                    }
                }
                if let Shape::Canvas(canvas) = &o.shape {
                    let pixels = self.images.get(&o.id)?;
                    let clip = self.path_clip(data.as_deref().expect("Canvas path"))?;
                    self.output
                        .emit(format_args!("<g clip-path=\"url(#s{clip})\">"))?;
                    self.image(&pixels, canvas.bounds(), canvas.smooth, "canvas")?;
                    self.output.emit(format_args!("</g>"))?;
                }
                let after = o.shape.paint_phases().1.is_some();
                if !after {
                    self.strokes(node)?;
                }
                let clip = if o.shape.child_clip().is_some() {
                    Some(self.path_clip(data.as_deref().expect("container path"))?)
                } else {
                    None
                };
                self.children(node, clip)?;
                if after {
                    self.strokes(node)?;
                }
                if let Some(data) = data {
                    self.output.release(data.capacity() as u64);
                }
            }
        }
        self.output.emit(format_args!("</g>"))
    }
    fn children(&mut self, node: &DomainNode, clip: Option<usize>) -> Result<(), CompositeError> {
        if let Some(clip) = clip {
            self.output
                .emit(format_args!("<g clip-path=\"url(#s{clip})\">"))?;
        }
        for child in &node.children {
            self.node(child)?;
        }
        if clip.is_some() {
            self.output.emit(format_args!("</g>"))?;
        }
        Ok(())
    }
    fn shape_data(&mut self, shape: &Shape) -> Result<Option<String>, CompositeError> {
        let path = match shape {
            Shape::Path(path) => return self.path_data(path).map(Some),
            Shape::Pen(pen) => return self.path_data(&pen.path).map(Some),
            Shape::Line(_) | Shape::Group(_) => return Ok(None),
            Shape::Rect(rect) => rect.to_path(),
            Shape::Box(container) => container.to_path(),
            Shape::Frame(container) => container.to_path(),
            Shape::Ellipse(ellipse) => ellipse.to_path(),
            Shape::Canvas(canvas) => canvas.to_path(),
            Shape::Polygon(_) | Shape::Star(_) => {
                let mut commands = Vec::new();
                for contour in shape.flatten(0.1) {
                    if let Some(first) = contour.points.first() {
                        commands.push(PathCommand::MoveTo { to: *first });
                        commands.extend(
                            contour.points[1..]
                                .iter()
                                .map(|p| PathCommand::LineTo { to: *p }),
                        );
                        if contour.closed {
                            commands.push(PathCommand::Close);
                        }
                    }
                }
                Ok(Path::new(commands))
            }
            _ => return Err(CompositeError::UnsupportedShape("SVG geometry".into())),
        }
        .map_err(CompositeError::InvalidGeometry)?;
        self.generated_path_data(path).map(Some)
    }
    fn generated_path_data(&mut self, path: Path) -> Result<String, CompositeError> {
        let retained = path.commands.capacity() as u64 * std::mem::size_of::<PathCommand>() as u64;
        self.output.hold(retained)?;
        let result = self.path_data(&path);
        self.output.release(retained);
        result
    }
    fn path_data(&mut self, path: &Path) -> Result<String, CompositeError> {
        self.points = self.points.saturating_add(path.commands.len());
        if path.commands.len() > self.output.limits.max_points
            || self.points > self.output.limits.max_total_points
        {
            return Err(CompositeError::InputLimit);
        }
        // Worst-case f32 decimal spelling plus command punctuation, and String
        // growth slack. Admission happens before Path::to_svg allocates output.
        let reserve = (path.commands.len() as u64).saturating_mul(640).max(32);
        self.output.hold(reserve)?;
        let data = path.to_svg().map_err(CompositeError::InvalidGeometry)?;
        self.output.release(reserve);
        self.output.hold(data.capacity() as u64)?;
        Ok(data)
    }
    fn strokes(&mut self, node: &DomainNode) -> Result<(), CompositeError> {
        let o = &node.object;
        for stroke in o
            .style
            .strokes()
            .filter(|s| s.width > 0. && s.paint.is_visible())
        {
            self.output.check()?;
            let mesh = o
                .shape
                .stroke_mesh_with_rule(stroke, o.style.fill_rule)
                .map_err(CompositeError::InvalidGeometry)?;
            self.triangles = self.triangles.saturating_add(mesh.triangles().len());
            if self.triangles > self.output.limits.max_triangles {
                return Err(CompositeError::InputLimit);
            }
            // The shared tessellator enforces its own allocation/work ceilings.
            // Retained mesh storage and its Vec growth are charged while writing.
            let retained = std::mem::size_of_val(mesh.triangles()) as u64 * 3;
            self.output.hold(retained)?;
            let id = self.paint(&stroke.paint)?;
            self.output.emit(format_args!("<path d=\""))?;
            for &[a, mut b, mut c] in mesh.triangles() {
                let cross = (b.x as f64 - a.x as f64) * (c.y as f64 - a.y as f64)
                    - (b.y as f64 - a.y as f64) * (c.x as f64 - a.x as f64);
                if cross == 0. {
                    continue;
                }
                if cross < 0. {
                    std::mem::swap(&mut b, &mut c);
                }
                self.output.emit(format_args!(
                    "M{} {}L{} {}L{} {}Z ",
                    coordinate(a.x)?,
                    coordinate(a.y)?,
                    coordinate(b.x)?,
                    coordinate(b.y)?,
                    coordinate(c.x)?,
                    coordinate(c.y)?
                ))?;
            }
            self.output.emit(format_args!("\""))?;
            self.paint_attributes(&stroke.paint, id)?;
            self.output.emit(format_args!(" fill-rule=\"nonzero\"/>"))?;
            self.output.release(retained);
        }
        Ok(())
    }
    fn text(&mut self, node: &DomainNode) -> Result<(), CompositeError> {
        if let Some(text) = self.texts.get(&node.object.id)? {
            let clip = self
                .texts
                .clip(&node.object.id)
                .map(|b| self.rect_clip(b))
                .transpose()?;
            if let Some(clip) = clip {
                self.output
                    .emit(format_args!("<g clip-path=\"url(#s{clip})\">"))?;
            }
            self.image(&text.image, text.local_bounds, true, "text-ink")?;
            if clip.is_some() {
                self.output.emit(format_args!("</g>"))?;
            }
        }
        Ok(())
    }
    fn image(
        &mut self,
        image: &StaticImage,
        bounds: Bounds,
        smooth: bool,
        kind: &str,
    ) -> Result<(), CompositeError> {
        self.output.check()?;
        let work = u64::from(image.width()) * u64::from(image.height()) * 3;
        self.output.charge(work)?;
        let available = self.output.available();
        // Source image already belongs to retained inputs; give the existing
        // PNG encoder that charge once, plus exactly the remaining capacity.
        let limits = CompositeLimits {
            max_surface_bytes: available.saturating_add(image.byte_len()),
            max_surface_writes: work,
            ..self.output.limits
        };
        if available <= PNG_SCRATCH {
            return Err(CompositeError::WorkingSetLimit);
        }
        let png = image.encode_png(limits)?;
        self.output.check()?;
        self.output.hold(png.capacity() as u64)?;
        self.output
            .emit(format_args!("<image data-flow-raster=\"{kind}\" "))?;
        self.rect_attributes(bounds)?;
        let interpolation = if smooth {
            "optimizeQuality"
        } else {
            "optimizeSpeed"
        };
        self.output.emit(format_args!(" preserveAspectRatio=\"none\" image-rendering=\"{interpolation}\" xlink:href=\"data:image/png;base64,"))?;
        // Chunk lengths divisible by three avoid interior base64 padding. The
        // stack buffer is fixed, with no full-size base64 intermediate string.
        self.output.hold(4096)?;
        let mut encoded = [0u8; 4096];
        for chunk in png.chunks(3072) {
            let len = STANDARD
                .encode_slice(chunk, &mut encoded)
                .expect("base64 chunk bound");
            let text = std::str::from_utf8(&encoded[..len]).expect("base64 ASCII");
            self.output.emit(format_args!("{text}"))?;
        }
        self.output.emit(format_args!("\"/>"))?;
        self.output.release(4096);
        self.output.release(png.capacity() as u64);
        Ok(())
    }
}

fn number(n: f32) -> Result<f32, CompositeError> {
    if !n.is_finite() {
        return Err(CompositeError::InvalidGeometry(
            "SVG non-finite number".into(),
        ));
    }
    Ok(if n == 0. { 0. } else { n })
}
fn coordinate(n: f32) -> Result<f32, CompositeError> {
    if n.abs() > 1_000_000. {
        return Err(CompositeError::InvalidGeometry(
            "SVG coordinate exceeds +/-1000000".into(),
        ));
    }
    number(n)
}

/// Accounts for retained input/host buffers, live geometry/PNG temporaries and
/// output capacity. Work is host pixel writes + 3 per embedded PNG pixel + one
/// per XML byte. Internal tessellator/codec allocations retain their own limits;
/// this is deliberately not a process-wide allocator or latency guarantee.
struct Output<'a> {
    bytes: Vec<u8>,
    limits: CompositeLimits,
    retained: u64,
    scratch: u64,
    work: u64,
    cancelled: &'a AtomicBool,
    error: Option<CompositeError>,
}
impl<'a> Output<'a> {
    fn new(
        limits: CompositeLimits,
        retained: u64,
        work: u64,
        cancelled: &'a AtomicBool,
    ) -> Result<Self, CompositeError> {
        let output = Self {
            bytes: Vec::new(),
            limits,
            retained,
            scratch: 0,
            work,
            cancelled,
            error: None,
        };
        output.check()?;
        if retained > limits.max_surface_bytes {
            return Err(CompositeError::WorkingSetLimit);
        }
        if work > limits.max_surface_writes {
            return Err(CompositeError::WorkLimit);
        }
        Ok(output)
    }
    fn check(&self) -> Result<(), CompositeError> {
        if self.cancelled.load(Ordering::Relaxed) {
            Err(CompositeError::Superseded)
        } else {
            Ok(())
        }
    }
    fn available(&self) -> u64 {
        self.limits
            .max_surface_bytes
            .saturating_sub(self.retained)
            .saturating_sub(self.scratch)
            .saturating_sub(self.bytes.capacity() as u64)
    }
    fn hold(&mut self, bytes: u64) -> Result<(), CompositeError> {
        if bytes > self.available() {
            return Err(CompositeError::WorkingSetLimit);
        }
        self.scratch += bytes;
        Ok(())
    }
    fn release(&mut self, bytes: u64) {
        self.scratch -= bytes;
    }
    fn charge(&mut self, work: u64) -> Result<(), CompositeError> {
        self.work = self
            .work
            .checked_add(work)
            .ok_or(CompositeError::WorkLimit)?;
        if self.work > self.limits.max_surface_writes {
            Err(CompositeError::WorkLimit)
        } else {
            Ok(())
        }
    }
    fn emit(&mut self, args: fmt::Arguments<'_>) -> Result<(), CompositeError> {
        self.write_fmt(args).map_err(|_| {
            self.error
                .take()
                .unwrap_or(CompositeError::AllocationFailed)
        })
    }
    fn attribute(&mut self, value: &str) -> Result<(), CompositeError> {
        for c in value.chars() {
            let escape = match c {
                '&' => "&amp;",
                '<' => "&lt;",
                '>' => "&gt;",
                '"' => "&quot;",
                '\'' => "&apos;",
                '\t' => "&#9;",
                '\n' => "&#10;",
                '\r' => "&#13;",
                '\u{0}'..='\u{1f}' | '\u{fffe}' | '\u{ffff}' => {
                    return Err(CompositeError::InvalidGeometry(
                        "SVG invalid XML character".into(),
                    ));
                }
                _ => {
                    self.emit(format_args!("{c}"))?;
                    continue;
                }
            };
            self.emit(format_args!("{escape}"))?;
        }
        Ok(())
    }
    fn append(&mut self, text: &str) -> Result<(), CompositeError> {
        self.check()?;
        self.charge(text.len() as u64)?;
        let needed = self
            .bytes
            .len()
            .checked_add(text.len())
            .ok_or(CompositeError::WorkingSetLimit)?;
        let maximum = self
            .limits
            .max_surface_bytes
            .saturating_sub(self.retained)
            .saturating_sub(self.scratch) as usize;
        if needed > maximum {
            return Err(CompositeError::WorkingSetLimit);
        }
        if needed > self.bytes.capacity() {
            let capacity = needed
                .max(self.bytes.capacity().saturating_mul(2))
                .max(256)
                .min(maximum);
            self.bytes
                .try_reserve_exact(capacity - self.bytes.len())
                .map_err(|_| CompositeError::AllocationFailed)?;
        }
        self.bytes.extend_from_slice(text.as_bytes());
        Ok(())
    }
}
impl fmt::Write for Output<'_> {
    fn write_str(&mut self, text: &str) -> fmt::Result {
        self.append(text).map_err(|e| {
            self.error = Some(e);
            fmt::Error
        })
    }
}
