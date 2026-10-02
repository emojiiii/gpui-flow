use crate::{CompositeError, CompositeLimits, DomainNode, ImageInputs, StaticImage, TextInputs};
use gpui_flow_scene::{
    Affine, BlendMode, ClipRegion, FillRule, MaskMode, Paint, Shape, display::PathCommand,
};
use std::sync::Arc;
use tiny_skia::{Path, PathBuilder, Transform};

pub(crate) struct PreparedNode {
    pub bounds: Option<[f64; 4]>,
    pub shadows: Vec<crate::shadow::PreparedShadow>,
    pub inner_shadows: Vec<crate::shadow::PreparedShadow>,
    pub visible: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    pub mask: MaskMode,
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
pub(crate) struct ImagePaint {
    pub image: Arc<StaticImage>,
    pub path: Path,
    pub texture_to_local: Affine,
    pub world: Affine,
}
pub(crate) struct VectorPaint {
    pub path: Path,
    pub paint: tiny_skia::Paint<'static>,
    pub rule: tiny_skia::FillRule,
    pub world: Affine,
}
#[derive(Default)]
pub(crate) struct Preparation {
    pub triangles: usize,
    pub has_shadows: bool,
    pub has_inner_shadows: bool,
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
    let mut b = PathBuilder::new();
    // These primitive contours have existing bounded generation (no Bezier
    // flattening). Curve paths above go directly into the mature scan converter.
    for c in shape.flatten(0.1) {
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
    Ok(b.finish())
}
impl Preparation {
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
    pub fn node(
        &mut self,
        source: &DomainNode,
        limits: CompositeLimits,
        images: &ImageInputs,
        texts: &TextInputs,
    ) -> Result<PreparedNode, CompositeError> {
        self.node_at(source, limits, images, texts, 0)
    }
    fn node_at(
        &mut self,
        source: &DomainNode,
        limits: CompositeLimits,
        images: &ImageInputs,
        texts: &TextInputs,
        depth: usize,
    ) -> Result<PreparedNode, CompositeError> {
        self.nodes += 1;
        if self.nodes > limits.max_nodes {
            return Err(CompositeError::InputLimit);
        }
        if depth > limits.max_depth {
            return Err(CompositeError::NestingLimit);
        }
        let object = &source.object;
        // Preserve mask-run markers even when their contents contribute zero.
        // Invisible/zero-opacity subtrees do not require loaded image pixels.
        if !object.visible || object.style.opacity == 0. {
            return Ok(PreparedNode {
                bounds: None,
                shadows: Vec::new(),
                inner_shadows: Vec::new(),
                visible: object.visible,
                opacity: object.style.opacity,
                blend: object.style.blend_mode,
                mask: object.mask,
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
        let mut paints = Vec::new();
        let mut fill_count = 0;
        let mut image_paint = None;
        if matches!(object.shape, Shape::Text(_)) {
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
                image_paint = Some(ImagePaint {
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
            let pixels = images.get(&object.id)?;
            // Native GPUI object-fit uses decoded dimensions, not optional
            // document metadata. Reuse the scene fit geometry with those facts.
            let mut fitted = image.clone();
            fitted.natural_size = Some(gpui_flow_scene::Size::new(
                pixels.width() as f32,
                pixels.height() as f32,
            ));
            let content = fitted.content_bounds();
            if !content.is_valid() {
                return Err(CompositeError::InvalidImage);
            }
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
                    texture_to_local: Affine::translation(content.origin.x, content.origin.y)
                        * Affine::scale(
                            content.size.width / pixels.width() as f32,
                            content.size.height / pixels.height() as f32,
                        ),
                    image: pixels,
                    path,
                    world: source.world_transform,
                });
            }
        } else if !matches!(object.shape, Shape::Group(_)) {
            if let Some(path) = path(&object.shape)? {
                for fill in object.style.fills().filter(|p| p.is_visible()) {
                    self.add_path(&path, source.world_transform, limits)?;
                    paints.push(VectorPaint {
                        path: path.clone(),
                        paint: tiny_skia::Paint {
                            shader: shader(fill)?,
                            ..Default::default()
                        },
                        rule: match object.style.fill_rule {
                            FillRule::EvenOdd => tiny_skia::FillRule::EvenOdd,
                            FillRule::NonZero => tiny_skia::FillRule::Winding,
                        },
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
                let mesh = object
                    .shape
                    .stroke_mesh_with_rule(stroke, object.style.fill_rule)
                    .map_err(CompositeError::InvalidGeometry)?;
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
                        paint: tiny_skia::Paint {
                            shader: shader(&stroke.paint)?,
                            ..Default::default()
                        },
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
        let children = source
            .children
            .iter()
            .map(|child| self.node_at(child, limits, images, texts, depth + 1))
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
                shadows.push(crate::shadow::PreparedShadow {
                    effect,
                    world,
                    ink_bounds,
                });
            }
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
            if let Some(ink_bounds) = ink_bounds {
                inner_shadows.push(crate::shadow::PreparedShadow {
                    effect,
                    world: source.world_transform,
                    ink_bounds,
                });
            }
        }
        self.bounds = match (previous_bounds, node_bounds) {
            (Some(a), Some(b)) => Some([
                a[0].min(b[0]),
                a[1].min(b[1]),
                a[2].max(b[2]),
                a[3].max(b[3]),
            ]),
            (a, b) => a.or(b),
        };
        Ok(PreparedNode {
            bounds: node_bounds,
            shadows,
            inner_shadows,
            visible: object.visible,
            opacity: object.style.opacity,
            blend: object.style.blend_mode,
            mask: object.mask,
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
