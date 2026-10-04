use crate::{
    CompositeError, CompositeLimits, CustomInputs, DomainNode, ImageInputs, StaticImage, TextInputs,
};
use gpui_flow_scene::{
    Affine, BlendMode, ClipRegion, EraserMode, FillRule, MaskMode, Paint, Shape,
    display::PathCommand,
};
use std::sync::Arc;
use tiny_skia::{Path, PathBuilder, Transform};

pub(crate) struct PreparedBackground {
    pub amount: f32,
    pub coverage: Option<VectorPaint>,
    pub site: crate::background::Site,
}

pub(crate) struct PreparedNode {
    pub id: gpui_flow_scene::DisplayId,
    pub background: Option<PreparedBackground>,
    pub bounds: Option<[f64; 4]>,
    pub world: Affine,
    pub blur: f32,
    pub device: Option<crate::blur::DeviceNode>,
    pub shadows: Vec<crate::shadow::PreparedShadow>,
    pub inner_shadows: Vec<crate::shadow::PreparedShadow>,
    pub visible: bool,
    pub opacity: f32,
    pub grayscale: f32,
    pub blend: BlendMode,
    pub mask: MaskMode,
    pub eraser: EraserMode,
    pub has_eraser_children: bool,
    pub erase_path: Option<VectorPaint>,
    /// Group Path erasure intersects child contours before clearing its bounds.
    pub erase_clips: Vec<Option<VectorPaint>>,
    /// Sequential geometric intersections; None is an empty leaf contour.
    pub mask_paths: Vec<Option<VectorPaint>>,
    pub paints: Vec<VectorPaint>,
    /// Fill prefix followed by stroke meshes. Geometry is prepared once.
    pub fill_count: usize,
    /// Group and StrokeAfterChildren: children then inner then stroke.
    /// BeforeChildren: inner then stroke then children. Leaf children are empty.
    pub children_before_inner: bool,
    pub image: Option<ImagePaint>,
    pub children: Vec<PreparedNode>,
    pub child_clip: Option<ClipRegion>,
}
#[derive(Clone, Copy)]
pub(crate) enum CompositeOperation {
    Blend(BlendMode),
    DestinationOut,
}
impl PreparedNode {
    pub fn outside(
        &self,
        world_to_layer: Affine,
        origin: [f64; 2],
        width: u32,
        height: u32,
    ) -> bool {
        if let Some(device) = &self.device {
            return device.completed.is_none_or(|b| {
                b[2] <= origin[0]
                    || b[3] <= origin[1]
                    || b[0] >= origin[0] + width as f64
                    || b[1] >= origin[1] + height as f64
            });
        }
        self.bounds.is_none_or(|b| {
            let b = crate::raster::transformed(b, world_to_layer);
            b[2] < -1. || b[3] < -1. || b[0] > width as f64 + 1. || b[1] > height as f64 + 1.
        })
    }

    pub fn operation(&self) -> CompositeOperation {
        if self.eraser == EraserMode::Pixel {
            CompositeOperation::DestinationOut
        } else {
            CompositeOperation::Blend(self.blend)
        }
    }
    pub fn flushes_mask(&self, mask_opacity: f32) -> bool {
        mask_opacity == 1.
            && (self.eraser == EraserMode::Pixel || self.blend != BlendMode::PassThrough)
    }
    pub fn isolated(&self) -> bool {
        self.eraser == EraserMode::Pixel
            || self.has_eraser_children
            || !self.shadows.is_empty()
            || !self.inner_shadows.is_empty()
            || self.grayscale > 0.
            || self.blur > 0.
            || self.opacity < 1.
            || self.blend != BlendMode::PassThrough
    }
}
pub(crate) struct ImagePaint {
    pub own_clip: Option<ClipRegion>,
    pub smooth: bool,
    pub image: Arc<StaticImage>,
    pub path: Path,
    pub texture_to_local: Affine,
    pub world: Affine,
}
pub(crate) struct VectorPaint {
    pub path: Path,
    pub paint: VectorSource,
    pub rule: tiny_skia::FillRule,
    pub world: Affine,
}
/// Keep image ownership separate from tiny-skia's borrowed shader lifetime.
pub(crate) enum VectorSource {
    Scene(tiny_skia::Paint<'static>),
    Pattern {
        image: Arc<StaticImage>,
        texture_to_local: Affine,
        smooth: bool,
    },
}
impl VectorSource {
    fn new(
        paint: &Paint,
        id: &gpui_flow_scene::DisplayId,
        images: &ImageInputs,
    ) -> Result<Self, CompositeError> {
        if let Some(pattern) = images.pattern(id) {
            Ok(Self::Pattern {
                image: images.get(id)?,
                texture_to_local: pattern.transform,
                smooth: pattern.smooth,
            })
        } else {
            Ok(Self::Scene(tiny_skia::Paint {
                shader: shader(paint)?,
                ..Default::default()
            }))
        }
    }
}
#[derive(Default)]
pub(crate) struct Preparation {
    pub triangles: usize,
    pub logical_camera: Option<f32>,
    pub lowered_geometry: Option<gpui_flow_scene::StrokeGeometryBudget>,
    pub has_shadows: bool,
    pub has_grayscale: bool,
    pub has_blur: bool,
    pub background_sites: Vec<(gpui_flow_scene::DisplayId, crate::background::Site)>,
    pub has_inner_shadows: bool,
    pub has_shadow_modes: bool,
    pub has_image_clips: bool,
    pub has_erasers: bool,
    pub has_masks: bool,
    nodes: usize,
    masks: usize,
    source_points: usize,
    pub points: usize,
    pub bounds: Option<[f64; 4]>,
}
pub(crate) fn transform(a: Affine) -> Transform {
    Transform::from_row(a.a, a.b, a.c, a.d, a.tx, a.ty)
}
fn color(c: gpui_flow_scene::Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}
fn shader(paint: &Paint) -> Result<tiny_skia::Shader<'static>, CompositeError> {
    use tiny_skia::{GradientStop, LinearGradient, Point, RadialGradient, Shader, SpreadMode};
    let stops = |values: &[gpui_flow_scene::GradientStop]| {
        values
            .iter()
            .map(|s| GradientStop::new(s.offset, color(s.color)))
            .collect()
    };
    let shader = match paint {
        Paint::Solid { color: c } => Some(Shader::SolidColor(color(*c))),
        Paint::LinearGradient {
            start,
            end,
            stops: s,
        } => LinearGradient::new(
            Point::from_xy(start.x, start.y),
            Point::from_xy(end.x, end.y),
            stops(s),
            SpreadMode::Pad,
            Transform::identity(),
        ),
        Paint::RadialGradient {
            center,
            radius,
            stops: s,
        } => RadialGradient::new(
            Point::from_xy(center.x, center.y),
            Point::from_xy(center.x, center.y),
            *radius,
            stops(s),
            SpreadMode::Pad,
            Transform::identity(),
        ),
    };
    shader.ok_or_else(|| CompositeError::InvalidGeometry("gradient cannot be represented".into()))
}
pub(crate) fn path(shape: &Shape) -> Result<Option<Path>, CompositeError> {
    let geometry_path = match shape {
        Shape::Ellipse(ellipse) if !ellipse.is_default_oval() => {
            Some(ellipse.to_path().map_err(CompositeError::InvalidGeometry)?)
        }
        Shape::Rect(rect) if rect.corner_radii.is_some() => {
            Some(rect.to_path().map_err(CompositeError::InvalidGeometry)?)
        }
        Shape::Box(container) if container.corner_radii.is_some() => Some(
            container
                .to_path()
                .map_err(CompositeError::InvalidGeometry)?,
        ),
        Shape::Frame(container) if container.corner_radii.is_some() => Some(
            container
                .to_path()
                .map_err(CompositeError::InvalidGeometry)?,
        ),
        _ => None,
    };
    let commands = match shape {
        Shape::Canvas(c) => {
            return path(&Shape::Path(
                c.to_path().map_err(CompositeError::InvalidGeometry)?,
            ));
        }
        Shape::Path(p) => Some(&p.commands),
        Shape::Pen(p) => Some(&p.path.commands),
        Shape::Ellipse(_) | Shape::Rect(_) | Shape::Box(_) | Shape::Frame(_) => {
            geometry_path.as_ref().map(|p| &p.commands)
        }
        _ => None,
    };
    if let Some(commands) = commands {
        let mut b = PathBuilder::new();
        for command in commands {
            match *command {
                PathCommand::MoveTo { to } => b.move_to(to.x, to.y),
                PathCommand::LineTo { to } => b.line_to(to.x, to.y),
                PathCommand::QuadraticTo { control, to } => {
                    b.quad_to(control.x, control.y, to.x, to.y)
                }
                PathCommand::CubicTo {
                    control1,
                    control2,
                    to,
                } => b.cubic_to(control1.x, control1.y, control2.x, control2.y, to.x, to.y),
                PathCommand::Close => b.close(),
            }
        }
        return Ok(b.finish());
    }
    if let Shape::Ellipse(e) = shape {
        return tiny_skia::Rect::from_xywh(0., 0., e.size.width, e.size.height)
            .map(PathBuilder::from_oval)
            .ok_or_else(|| CompositeError::InvalidGeometry("invalid ellipse".into()));
    }
    if matches!(shape, Shape::Group(_) | Shape::Line(_)) {
        return Ok(None);
    }
    if !matches!(
        shape,
        Shape::Rect(_) | Shape::Box(_) | Shape::Frame(_) | Shape::Star(_) | Shape::Polygon(_)
    ) {
        return Err(CompositeError::UnsupportedShape(shape.tag().into()));
    }
    // These primitive contours have existing bounded generation (no Bezier
    // flattening). Curve paths above go directly into the mature scan converter.
    Ok(contour_path(&shape.flatten(0.1)))
}

fn contour_path(contours: &[gpui_flow_scene::Contour]) -> Option<Path> {
    let mut b = PathBuilder::new();
    for c in contours {
        if let Some(first) = c.points.first() {
            b.move_to(first.x, first.y);
            for p in &c.points[1..] {
                b.line_to(p.x, p.y);
            }
            if c.closed {
                b.close();
            }
        }
    }
    b.finish()
}

/// Geometry-only operations may use serialized Custom contours, but these are
/// not a pixel producer for the host renderer or a self-painting mask.
fn geometric_path(shape: &Shape) -> Result<Option<Path>, CompositeError> {
    match shape {
        Shape::Custom(custom) => Ok(contour_path(&custom.contours)),
        Shape::Image(image) => path(&Shape::Rect(image.rect())),
        shape => path(shape),
    }
}
impl Preparation {
    pub fn with_lowered_geometry(budget: Option<gpui_flow_scene::StrokeGeometryBudget>) -> Self {
        Self {
            lowered_geometry: budget,
            ..Default::default()
        }
    }
    fn add_path(
        &mut self,
        path: &Path,
        world: Affine,
        limits: CompositeLimits,
    ) -> Result<[f64; 4], CompositeError> {
        self.points = self.points.saturating_add(path.points().len());
        if path.points().len() > limits.max_points.saturating_mul(12)
            || self.points
                > limits
                    .max_total_points
                    .saturating_add(limits.max_triangles.saturating_mul(3))
        {
            return Err(CompositeError::InputLimit);
        }
        let b = path.bounds();
        let mut bounds = [
            f64::INFINITY,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NEG_INFINITY,
        ];
        for (x, y) in [
            (b.left(), b.top()),
            (b.right(), b.top()),
            (b.right(), b.bottom()),
            (b.left(), b.bottom()),
        ] {
            let xw = world.a as f64 * x as f64 + world.c as f64 * y as f64 + world.tx as f64;
            let yw = world.b as f64 * x as f64 + world.d as f64 * y as f64 + world.ty as f64;
            if !xw.is_finite() || !yw.is_finite() {
                return Err(CompositeError::UnsupportedTransform);
            }
            bounds[0] = bounds[0].min(xw);
            bounds[1] = bounds[1].min(yw);
            bounds[2] = bounds[2].max(xw);
            bounds[3] = bounds[3].max(yw);
        }
        self.bounds = Some(match self.bounds {
            None => bounds,
            Some(b) => [
                b[0].min(bounds[0]),
                b[1].min(bounds[1]),
                b[2].max(bounds[2]),
                b[3].max(bounds[3]),
            ],
        });
        Ok(bounds)
    }
    /// Reference-only hidden geometry is re-admitted against this raster
    /// request's limits, which may be tighter than the original capture limits.
    fn reference_geometry(
        &mut self,
        source: &DomainNode,
        limits: CompositeLimits,
        depth: usize,
        root_counted: bool,
    ) -> Result<(), CompositeError> {
        if !root_counted {
            self.nodes += 1;
        }
        if self.nodes > limits.max_nodes {
            return Err(CompositeError::InputLimit);
        }
        if depth > limits.max_depth {
            return Err(CompositeError::NestingLimit);
        }
        let count = match &source.object.shape {
            Shape::Path(p) => p.commands.len(),
            Shape::Pen(p) => p.path.commands.len(),
            Shape::Polygon(p) => p.points.len(),
            Shape::Star(p) => p.points.saturating_mul(2),
            Shape::Custom(c) => c.contours.iter().fold(0usize, |count, contour| {
                count.saturating_add(contour.points.len())
            }),
            _ => 0,
        };
        self.source_points = self.source_points.saturating_add(count);
        if count > limits.max_points || self.source_points > limits.max_total_points {
            return Err(CompositeError::InputLimit);
        }
        for child in &source.children {
            self.reference_geometry(child, limits, depth + 1, false)?;
        }
        Ok(())
    }
    fn geometric_clips(
        &mut self,
        source: &DomainNode,
        limits: CompositeLimits,
        out: &mut Vec<Option<VectorPaint>>,
        unsupported: fn(gpui_flow_scene::DisplayId) -> CompositeError,
    ) -> Result<(), CompositeError> {
        let object = &source.object;
        if !object.visible || object.style.opacity == 0. {
            return Ok(());
        }
        if matches!(object.shape, Shape::Group(_)) {
            // Leafer Branch.__clip intersects child contours, rather than
            // treating the branch's bounding rectangle as a filled path.
            for child in &source.children {
                self.geometric_clips(child, limits, out, unsupported)?;
            }
            return Ok(());
        }
        let contour = match &object.shape {
            Shape::Text(_) => {
                return Err(unsupported(object.id.clone()));
            }
            shape => geometric_path(shape)?,
        };
        let paint = if let Some(path) = contour {
            self.add_path(&path, source.world_transform, limits)?;
            Some(VectorPaint {
                path,
                paint: VectorSource::Scene(tiny_skia::Paint::default()),
                rule: match object.style.fill_rule {
                    FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
                    FillRule::NonZero => tiny_skia::FillRule::Winding,
                },
                world: source.world_transform,
            })
        } else {
            None
        };
        out.push(paint);
        Ok(())
    }
    #[cfg(test)]
    pub fn node(
        &mut self,
        source: &DomainNode,
        limits: CompositeLimits,
        images: &ImageInputs,
        texts: &TextInputs,
    ) -> Result<PreparedNode, CompositeError> {
        self.node_with_custom(source, limits, images, texts, &CustomInputs::default())
    }
    pub fn node_with_custom(
        &mut self,
        source: &DomainNode,
        limits: CompositeLimits,
        images: &ImageInputs,
        texts: &TextInputs,
        customs: &CustomInputs,
    ) -> Result<PreparedNode, CompositeError> {
        self.node_at(source, limits, images, texts, customs, 0, false)
    }
    #[allow(clippy::too_many_arguments)]
    fn node_at(
        &mut self,
        source: &DomainNode,
        limits: CompositeLimits,
        images: &ImageInputs,
        texts: &TextInputs,
        customs: &CustomInputs,
        depth: usize,
        reference_required: bool,
    ) -> Result<PreparedNode, CompositeError> {
        self.nodes += 1;
        if self.nodes > limits.max_nodes {
            return Err(CompositeError::InputLimit);
        }
        if depth > limits.max_depth {
            return Err(CompositeError::NestingLimit);
        }
        let resolved;
        let object = if source.object.mask != MaskMode::Path
            && source.object.eraser != EraserMode::Path
            && source.object.style.has_scale_fixed_strokes()
        {
            resolved = source
                .object
                .resolved_strokes(source.stroke_scale, self.logical_camera.unwrap_or(1.))
                .map_err(CompositeError::InvalidGeometry)?;
            resolved
                .validate()
                .map_err(CompositeError::InvalidGeometry)?;
            &resolved
        } else {
            &source.object
        };
        crate::plan::validate_eraser(object)?;
        let has_eraser_children = source
            .children
            .iter()
            .any(|child| child.object.eraser != EraserMode::None);
        self.has_erasers |= object.eraser != EraserMode::None || has_eraser_children;
        self.has_masks |= object.mask != MaskMode::None;
        // Preserve mask-run markers even when their contents contribute zero.
        // Invisible/zero-opacity subtrees do not require loaded image pixels.
        if !object.visible || object.style.opacity == 0. {
            if reference_required {
                self.reference_geometry(source, limits, depth, true)?;
            }
            return Ok(PreparedNode {
                id: object.id.clone(),
                background: None,
                bounds: None,
                world: source.world_transform,
                blur: 0.,
                device: None,
                shadows: Vec::new(),
                inner_shadows: Vec::new(),
                visible: object.visible,
                opacity: object.style.opacity,
                grayscale: 0.,
                blend: object.style.blend_mode,
                mask: object.mask,
                eraser: object.eraser,
                has_eraser_children,
                erase_path: None,
                erase_clips: vec![],
                mask_paths: vec![],
                paints: vec![],
                fill_count: 0,
                children_before_inner: true,
                image: None,
                children: vec![],
                child_clip: None,
            });
        }
        if object.mask == MaskMode::Path {
            self.masks += 1;
            if self.masks > limits.max_mask_runs {
                return Err(CompositeError::InputLimit);
            }
            self.reference_geometry(source, limits, depth, true)?;
            let prior_bounds = self.bounds.take();
            let mut mask_paths = Vec::new();
            self.geometric_clips(
                source,
                limits,
                &mut mask_paths,
                CompositeError::UnsupportedPathMask,
            )?;
            let bounds = self.bounds.take();
            self.bounds = prior_bounds; // a nonpainting mask cannot enlarge ink
            return Ok(PreparedNode {
                id: object.id.clone(),
                background: None,
                bounds,
                world: source.world_transform,
                blur: 0.,
                device: None,
                shadows: vec![],
                inner_shadows: vec![],
                visible: object.visible,
                opacity: object.style.opacity,
                grayscale: 0.,
                blend: BlendMode::PassThrough,
                mask: object.mask,
                eraser: EraserMode::None,
                has_eraser_children: false,
                erase_path: None,
                erase_clips: vec![],
                mask_paths,
                paints: vec![],
                fill_count: 0,
                children_before_inner: true,
                image: None,
                children: vec![],
                child_clip: None,
            });
        }
        if object.mask != MaskMode::None {
            self.masks += 1;
        }
        let count = match &object.shape {
            Shape::Path(p) => p.commands.len(),
            Shape::Pen(p) => p.path.commands.len(),
            Shape::Polygon(p) => p.points.len(),
            Shape::Star(p) => p.points.saturating_mul(2),
            Shape::Custom(c) => c.contours.iter().fold(0usize, |count, contour| {
                count.saturating_add(contour.points.len())
            }),
            _ => 0,
        };
        self.source_points = self.source_points.saturating_add(count);
        if count > limits.max_points
            || self.source_points > limits.max_total_points
            || self.masks > limits.max_mask_runs
        {
            return Err(CompositeError::InputLimit);
        }
        let previous_bounds = self.bounds.take();
        let mut mask_paths = Vec::new();
        if object.mask == MaskMode::ClippingPath {
            self.geometric_clips(
                source,
                limits,
                &mut mask_paths,
                CompositeError::UnsupportedPathMask,
            )?;
            self.bounds = None; // clip geometry is not an alpha producer
        }
        if object.eraser == EraserMode::Path {
            let mut erase_clips = Vec::new();
            for child in &source.children {
                self.reference_geometry(child, limits, depth + 1, false)?;
            }
            let (contour, world) = if matches!(object.shape, Shape::Group(_)) {
                self.geometric_clips(
                    source,
                    limits,
                    &mut erase_clips,
                    CompositeError::UnsupportedPathEraser,
                )?;
                let contour = reference_bounds(source, self.logical_camera.unwrap_or(1.), true)?;
                if let Some(bounds) = contour {
                    (
                        path(&Shape::Rect(gpui_flow_scene::display::Rect::new(
                            bounds.size.width,
                            bounds.size.height,
                        )))?,
                        source.world_transform
                            * Affine::translation(bounds.origin.x, bounds.origin.y),
                    )
                } else {
                    (None, source.world_transform)
                }
            } else {
                let contour = geometric_path(&object.shape)?;
                (contour, source.world_transform)
            };
            let erase_path = if let Some(path) = contour {
                self.add_path(&path, world, limits)?;
                Some(VectorPaint {
                    path,
                    paint: VectorSource::Scene(tiny_skia::Paint::default()),
                    rule: match object.style.fill_rule {
                        FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
                        FillRule::NonZero => tiny_skia::FillRule::Winding,
                    },
                    world,
                })
            } else {
                None
            };
            let bounds = self.bounds.take();
            // Keep operation bounds for culling, but an eraser cannot enlarge
            // its parent's positive contribution or the domain allocation.
            self.bounds = previous_bounds;
            return Ok(PreparedNode {
                id: object.id.clone(),
                background: None,
                bounds,
                world: source.world_transform,
                blur: 0.,
                device: None,
                shadows: vec![],
                inner_shadows: vec![],
                visible: object.visible,
                opacity: object.style.opacity,
                grayscale: 0.,
                blend: object.style.blend_mode,
                mask: object.mask,
                eraser: object.eraser,
                has_eraser_children: false,
                erase_path,
                erase_clips,
                mask_paths,
                paints: vec![],
                fill_count: 0,
                children_before_inner: true,
                image: None,
                children: vec![],
                child_clip: None,
            });
        }
        // Coverage is independent of visible paint and never enters the own
        // foreground/shadow alpha source. add_path still charges its geometry.
        let mut coverage_bounds = None;
        let background = if object.style.has_background_blur() {
            let coverage = if let Some(path) = path(&object.shape)? {
                coverage_bounds = Some(self.add_path(&path, source.world_transform, limits)?);
                self.bounds = None;
                Some(VectorPaint {
                    path,
                    paint: VectorSource::Scene(tiny_skia::Paint::default()),
                    rule: match object.style.fill_rule {
                        FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
                        FillRule::NonZero => tiny_skia::FillRule::Winding,
                    },
                    world: source.world_transform,
                })
            } else {
                None
            };
            let site = self
                .background_sites
                .iter()
                .find(|(id, _)| id == &object.id)
                .map(|(_, site)| site.clone())
                .ok_or(CompositeError::UnsupportedExternalBackdrop)?;
            Some(PreparedBackground {
                amount: object.style.background_blur,
                coverage,
                site,
            })
        } else {
            None
        };
        self.has_grayscale |= object.style.has_grayscale();
        self.has_blur |= object.style.has_blur();
        let mut paints = Vec::new();
        let mut fill_count = 0;
        let mut image_paint = None;
        if let Shape::Custom(custom) = &object.shape {
            if object.style.has_blur() || object.style.has_scale_fixed_strokes() {
                return Err(CompositeError::UnsupportedCustomEffect(object.id.clone()));
            }
            if let Some(ink) = customs.get(&object.id)? {
                ink.validate(custom.bounds, limits)?;
                let b = ink.local_bounds;
                let rect =
                    tiny_skia::Rect::from_xywh(b.origin.x, b.origin.y, b.size.width, b.size.height)
                        .ok_or_else(|| {
                            CompositeError::InvalidGeometry("custom ink bounds".into())
                        })?;
                let path = PathBuilder::from_rect(rect);
                self.add_path(&path, source.world_transform, limits)?;
                image_paint = Some(ImagePaint {
                    own_clip: None,
                    smooth: true,
                    texture_to_local: Affine::translation(b.origin.x, b.origin.y)
                        * Affine::scale(
                            b.size.width / ink.image.width() as f32,
                            b.size.height / ink.image.height() as f32,
                        ),
                    image: ink.image.clone(),
                    path,
                    world: source.world_transform,
                });
            }
        } else if matches!(object.shape, Shape::Text(_)) {
            // A complete narrow subset. Never silently ignore unsupported paint.
            if object.style.strokes().next().is_some()
                || object.style.fills().count() > 1
                || object
                    .style
                    .fills()
                    .any(|p| !matches!(p, Paint::Solid { .. }))
            {
                return Err(CompositeError::UnsupportedTextStyle(object.id.clone()));
            }
            if let Some(text) = texts.get(&object.id)? {
                let b = text.local_bounds;
                let rect =
                    tiny_skia::Rect::from_xywh(b.origin.x, b.origin.y, b.size.width, b.size.height)
                        .ok_or_else(|| CompositeError::InvalidGeometry("text ink bounds".into()))?;
                let path = PathBuilder::from_rect(rect);
                self.add_path(&path, source.world_transform, limits)?;
                let local_clip = texts.clip(&object.id);
                if local_clip.is_some_and(|b| !b.is_valid() || !b.size.is_positive()) {
                    return Err(CompositeError::InvalidGeometry(
                        "text local clip bounds".into(),
                    ));
                }
                self.has_image_clips |= local_clip.is_some();
                image_paint = Some(ImagePaint {
                    own_clip: local_clip.map(|local_bounds| ClipRegion {
                        local_bounds,
                        corner_radius: 0.,
                        corner_radii: None,
                        world_transform: source.world_transform,
                    }),
                    smooth: true,
                    texture_to_local: Affine::translation(b.origin.x, b.origin.y)
                        * Affine::scale(
                            b.size.width / text.image.width() as f32,
                            b.size.height / text.image.height() as f32,
                        ),
                    image: text.image.clone(),
                    path,
                    world: source.world_transform,
                });
            }
        } else if let Shape::Image(image) = &object.shape {
            self.has_image_clips |= image.is_rounded();
            let pixels = images.get(&object.id)?;
            let mapping = image
                .mapping(gpui_flow_scene::Size::new(
                    pixels.width() as f32,
                    pixels.height() as f32,
                ))
                .ok_or(CompositeError::InvalidImage)?;
            let content = mapping.destination;
            let fitted_rect = tiny_skia::Rect::from_xywh(
                content.origin.x,
                content.origin.y,
                content.size.width,
                content.size.height,
            );
            let box_rect = tiny_skia::Rect::from_xywh(0., 0., image.size.width, image.size.height);
            if let Some(rect) = fitted_rect.zip(box_rect).and_then(|(a, b)| a.intersect(&b)) {
                let path = PathBuilder::from_rect(rect);
                self.add_path(&path, source.world_transform, limits)?;
                image_paint = Some(ImagePaint {
                    own_clip: image.is_rounded().then(|| ClipRegion {
                        local_bounds: image.bounds(),
                        corner_radius: image.corner_radius,
                        corner_radii: image.corner_radii,
                        world_transform: source.world_transform,
                    }),
                    smooth: true,
                    texture_to_local: Affine::translation(
                        mapping.full_source.origin.x,
                        mapping.full_source.origin.y,
                    ) * Affine::scale(
                        mapping.full_source.size.width / pixels.width() as f32,
                        mapping.full_source.size.height / pixels.height() as f32,
                    ),
                    image: pixels,
                    path,
                    world: source.world_transform,
                });
            }
        } else if !matches!(object.shape, Shape::Group(_)) {
            // Stroke-only geometry goes straight into the bounded shared mesh;
            // avoid a duplicate unused scan-converter curve path allocation.
            if object.style.fills().any(|paint| paint.is_visible())
                && let Some(path) = path(&object.shape)?
            {
                for fill in object.style.fills().filter(|p| p.is_visible()) {
                    self.add_path(&path, source.world_transform, limits)?;
                    paints.push(VectorPaint {
                        path: path.clone(),
                        paint: VectorSource::new(fill, &object.id, images)?,
                        rule: match object.style.fill_rule {
                            FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
                            FillRule::NonZero => tiny_skia::FillRule::Winding,
                        },
                        world: source.world_transform,
                    });
                }
            }
            if let Shape::Canvas(canvas) = &object.shape {
                let pixels = images.get(&object.id)?;
                if let Some(path) = path(&object.shape)? {
                    self.add_path(&path, source.world_transform, limits)?;
                    image_paint = Some(ImagePaint {
                        own_clip: None,
                        smooth: canvas.smooth,
                        texture_to_local: Affine::scale(
                            canvas.size.width / pixels.width() as f32,
                            canvas.size.height / pixels.height() as f32,
                        ),
                        image: pixels,
                        path,
                        world: source.world_transform,
                    });
                }
            }
            fill_count = paints.len();
            for stroke in object
                .style
                .strokes()
                .filter(|s| s.width > 0. && s.paint.is_visible())
            {
                let mesh = if let Some(budget) = &mut self.lowered_geometry {
                    let mesh = object
                        .shape
                        .stroke_mesh_with_rule_bounded(stroke, object.style.fill_rule, budget)
                        .map_err(lowered_geometry_error)?;
                    budget
                        .charge_render_path(mesh.triangles().len())
                        .map_err(lowered_geometry_error)?;
                    mesh
                } else {
                    object
                        .shape
                        .stroke_mesh_with_rule(stroke, object.style.fill_rule)
                        .map_err(CompositeError::InvalidGeometry)?
                };
                self.triangles = self.triangles.saturating_add(mesh.triangles().len());
                if self.triangles > limits.max_triangles {
                    return Err(CompositeError::InputLimit);
                }
                let mut b = PathBuilder::new();
                for &[a, mut c, mut d] in mesh.triangles() {
                    let cross = (c.x as f64 - a.x as f64) * (d.y as f64 - a.y as f64)
                        - (c.y as f64 - a.y as f64) * (d.x as f64 - a.x as f64);
                    if cross == 0. {
                        continue;
                    }
                    if cross < 0. {
                        std::mem::swap(&mut c, &mut d);
                    }
                    b.move_to(a.x, a.y);
                    b.line_to(c.x, c.y);
                    b.line_to(d.x, d.y);
                    b.close();
                }
                if let Some(path) = b.finish() {
                    self.add_path(&path, source.world_transform, limits)?;
                    // Fill the consistently wound triangle union once. Drawing
                    // each triangle source-over would double-alpha shared edges.
                    paints.push(VectorPaint {
                        path,
                        paint: VectorSource::new(&stroke.paint, &object.id, images)?,
                        rule: tiny_skia::FillRule::Winding,
                        world: source.world_transform,
                    });
                }
            }
        }
        let own_bounds = self.bounds.take();
        let child_clip =
            object
                .shape
                .child_clip()
                .map(|(local_bounds, corner_radius, corner_radii)| ClipRegion {
                    corner_radii,
                    local_bounds,
                    corner_radius,
                    world_transform: source.world_transform,
                });
        let reference_required = reference_required
            || (matches!(object.shape, Shape::Group(_))
                && object
                    .style
                    .shadows()
                    .chain(object.style.inner_shadow_layers.iter())
                    .any(|s| s.is_visible() && s.spread != 0.));
        let children = source
            .children
            .iter()
            .map(|child| {
                self.node_at(
                    child,
                    limits,
                    images,
                    texts,
                    customs,
                    depth + 1,
                    reference_required,
                )
            })
            .collect::<Result<_, _>>()?;
        let mut children_bounds = self.bounds.take();
        if child_clip.is_some() {
            // Count clip geometry, retaining f64 world bounds so a large
            // translation cancelled by the viewport cannot erase small clips.
            let clip_bounds = if let Some(path) = path(&object.shape)? {
                let bounds = self.add_path(&path, source.world_transform, limits)?;
                self.bounds = None;
                Some(bounds)
            } else {
                None
            };
            children_bounds = children_bounds.zip(clip_bounds).and_then(|(c, b)| {
                let clipped = [
                    c[0].max(b[0]),
                    c[1].max(b[1]),
                    c[2].min(b[2]),
                    c[3].min(b[3]),
                ];
                (clipped[2] > clipped[0] && clipped[3] > clipped[1]).then_some(clipped)
            });
        }
        let mut node_bounds = [own_bounds, children_bounds]
            .into_iter()
            .flatten()
            .reduce(|a, b| {
                [
                    a[0].min(b[0]),
                    a[1].min(b[1]),
                    a[2].max(b[2]),
                    a[3].max(b[3]),
                ]
            });
        // Every layer sees this immutable pre-effect extent. Do not feed an
        // earlier layer's expanded bounds or pixels into a later projection.
        let ink_bounds = node_bounds;
        let needs_reference = object
            .style
            .shadows()
            .chain(object.style.inner_shadow_layers.iter())
            .any(|s| s.is_visible() && s.spread != 0.);
        let reference = if needs_reference {
            reference_bounds(
                source,
                self.logical_camera.unwrap_or(1.),
                matches!(object.shape, Shape::Group(_)),
            )?
        } else {
            None
        };
        self.has_shadow_modes |= object
            .style
            .shadows()
            .any(|s| s.is_visible() && (s.spread != 0. || s.r#box || s.scale_fixed))
            || object
                .style
                .inner_shadow_layers
                .iter()
                .any(|s| s.is_visible() && (s.spread != 0. || s.scale_fixed));
        let mut shadows = Vec::new();
        if object.style.has_shadows() {
            crate::shadow::validate_shape(&object.id, &object.shape)?;
        }
        for &effect in object.style.shadows().filter(|s| s.is_visible()) {
            self.has_shadows = true;
            if let Some(ink_bounds) = ink_bounds {
                let world = source.world_transform;
                let dx = world.a as f64 * effect.x as f64 + world.c as f64 * effect.y as f64;
                let dy = world.b as f64 * effect.x as f64 + world.d as f64 * effect.y as f64;
                let halo = effect.blur as f64 * 2. * world.max_scale() as f64;
                let current = node_bounds.expect("source bounds");
                node_bounds = Some([
                    current[0].min(ink_bounds[0] + dx - halo),
                    current[1].min(ink_bounds[1] + dy - halo),
                    current[2].max(ink_bounds[2] + dx + halo),
                    current[3].max(ink_bounds[3] + dy + halo),
                ]);
            }
            // Retain the effect even if legacy world clipping found no ink.
            // A child's device blur halo can enter this node's child clip.
            shadows.push(crate::shadow::PreparedShadow {
                effect,
                world: source.world_transform,
                ink_bounds,
                reference,
            });
        }
        let mut inner_shadows = Vec::new();
        if object.style.has_inner_shadows() {
            crate::shadow::validate_inner_shape(&object.id, &object.shape)?;
        }
        for &effect in object
            .style
            .inner_shadow_layers
            .iter()
            .filter(|s| s.is_visible())
        {
            self.has_inner_shadows = true;
            inner_shadows.push(crate::shadow::PreparedShadow {
                effect,
                world: source.world_transform,
                ink_bounds,
                reference,
            });
        }
        node_bounds = crate::blur::union(node_bounds, coverage_bounds);
        self.bounds = match (
            previous_bounds,
            if object.eraser == EraserMode::None {
                node_bounds
            } else {
                None
            },
        ) {
            (Some(a), Some(b)) => Some([
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]),
            (a, b) => a.or(b),
        };
        Ok(PreparedNode {
            id: object.id.clone(),
            background,
            bounds: node_bounds,
            world: source.world_transform,
            blur: object.style.blur,
            device: None,
            shadows,
            inner_shadows,
            visible: object.visible,
            opacity: object.style.opacity,
            grayscale: object.style.grayscale,
            blend: object.style.blend_mode,
            mask: object.mask,
            eraser: object.eraser,
            has_eraser_children,
            erase_path: None,
            erase_clips: vec![],
            mask_paths,
            fill_count,
            children_before_inner: !matches!(object.shape, Shape::Box(_) | Shape::Frame(_))
                || object.shape.paint_phases().1.is_some(),
            paints,
            image: image_paint,
            children,
            child_clip,
        })
    }
}

fn lowered_geometry_error(reason: String) -> CompositeError {
    if reason.contains("allocation budget") {
        CompositeError::WorkingSetLimit
    } else if reason.contains("budget") || reason.contains("TooManyVertices") {
        CompositeError::InputLimit
    } else {
        CompositeError::InvalidGeometry(reason)
    }
}

/// Retained pre-effect geometry follows Scene::local_bounds, including hidden
/// children and Box overflow. It is separate from actual composed-alpha support.
fn reference_bounds(
    source: &DomainNode,
    camera: f32,
    recursive: bool,
) -> Result<Option<gpui_flow_scene::Bounds>, CompositeError> {
    let resolved;
    let object = if source.object.style.has_scale_fixed_strokes() {
        resolved = source
            .object
            .resolved_strokes(source.stroke_scale, camera)
            .map_err(CompositeError::InvalidGeometry)?;
        &resolved
    } else {
        &source.object
    };
    let mut result = object.paint_bounds();
    if recursive {
        let active_masks = crate::plan::has_active_masks(source);
        let end = if active_masks {
            source
                .children
                .iter()
                .position(|c| c.object.mask != MaskMode::None)
                .map_or(source.children.len(), |i| i + 1)
        } else {
            source.children.len()
        };
        for child in &source.children[..end] {
            if !active_masks && child.object.eraser != EraserMode::None {
                continue;
            }
            if let Some(b) = reference_bounds(child, camera, true)? {
                let b = child.object.transform.transform_bounds(b);
                let b = if let Some((clip, _, _)) = object.shape.child_clip() {
                    let x = b.min_x().max(clip.min_x());
                    let y = b.min_y().max(clip.min_y());
                    let w = b.max_x().min(clip.max_x()) - x;
                    let h = b.max_y().min(clip.max_y()) - y;
                    if w < 0. || h < 0. {
                        continue;
                    }
                    gpui_flow_scene::Bounds::new(
                        gpui_flow_scene::Point::new(x, y),
                        gpui_flow_scene::Size::new(w, h),
                    )
                } else {
                    b
                };
                result = Some(result.map_or(b, |r| r.union(b)));
            }
        }
    }
    if result.is_some_and(|b| !b.is_valid()) {
        return Err(CompositeError::InvalidShadowReference);
    }
    Ok(result)
}
