use crate::{CompositeError, CompositeLimits, DomainNode};
use gpui_flow_scene::{Affine, BlendMode, FillRule, MaskMode, Paint, Shape, display::PathCommand};
use tiny_skia::{Path, PathBuilder, Transform};

pub(crate) struct PreparedNode {
    pub visible: bool,
    pub opacity: f32,
    pub blend: BlendMode,
    pub mask: MaskMode,
    pub paints: Vec<VectorPaint>,
    pub children: Vec<PreparedNode>,
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
fn path(shape: &Shape) -> Result<Option<Path>, CompositeError> {
    let commands = match shape {
        Shape::Path(p) => Some(&p.commands),
        Shape::Pen(p) => Some(&p.path.commands),
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
    if !matches!(shape, Shape::Rect(_) | Shape::Star(_) | Shape::Polygon(_)) {
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
    ) -> Result<(), CompositeError> {
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
        Ok(())
    }
    pub fn node(
        &mut self,
        source: &DomainNode,
        limits: CompositeLimits,
    ) -> Result<PreparedNode, CompositeError> {
        self.node_at(source, limits, 0)
    }
    fn node_at(
        &mut self,
        source: &DomainNode,
        limits: CompositeLimits,
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
        let mut paints = Vec::new();
        if !matches!(object.shape, Shape::Group(_)) {
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
            for stroke in object
                .style
                .strokes()
                .filter(|s| s.width > 0. && s.paint.is_visible())
            {
                let mesh = object
                    .shape
                    .stroke_mesh(stroke)
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
        let children = source
            .children
            .iter()
            .map(|child| self.node_at(child, limits, depth + 1))
            .collect::<Result<_, _>>()?;
        Ok(PreparedNode {
            visible: object.visible,
            opacity: object.style.opacity,
            blend: object.style.blend_mode,
            mask: object.mask,
            paints,
            children,
        })
    }
}
