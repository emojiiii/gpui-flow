//! Shared centered-stroke geometry for non-default cap/join/dash styles.
//!
//! Kurbo 0.11.3 supplies dash splitting; Lyon 1.0.22 supplies all stroke
//! expansion/tessellation. Both are MIT OR Apache-2.0. This adapter only owns
//! shape conversion, limits, finite checks, and picking of the resulting mesh.
use crate::{Bounds, Contour, FlattenOptions, Point, Shape, Stroke, StrokeCap, StrokeJoin};
use kurbo::{PathEl, Shape as _};
use lyon_tessellation::{
    GeometryBuilder, GeometryBuilderError, StrokeGeometryBuilder, StrokeOptions, StrokeTessellator,
    StrokeVertex, VertexId,
    math::point,
    path::{LineCap, LineJoin, Path},
};

/// Canonical local approximation used by advanced-stroke render and hit alike.
/// It does not change with viewport zoom or device scale. The legacy solid
/// round renderer retains its adaptive screen-space flattening path.
pub const STROKE_GEOMETRY_TOLERANCE: f32 = 0.1;
/// Per-stroke source commands and flattened centerline points.
pub const MAX_STROKE_POINTS: usize = 65_536;
/// Per-stroke dash work estimate and generated path events.
pub const MAX_STROKE_EVENTS: usize = 262_144;
/// Per-stroke tessellated vertices and triangles (each independently bounded).
pub const MAX_STROKE_MESH_ITEMS: usize = 262_144;

/// Complete local-space triangle union, shared by picking and vector rendering.
/// Geometry exceeding the source, dash, or tessellation limits returns an error;
/// partial geometry is never returned. Shape/Style validation at scene commit
/// checks advanced visible strokes before any document mutation is accepted.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StrokeMesh {
    triangles: Vec<[Point; 3]>,
}
impl StrokeMesh {
    pub fn triangles(&self) -> &[[Point; 3]] {
        &self.triangles
    }
    pub fn bounds(&self) -> Option<Bounds> {
        crate::geometry::point_bounds(self.triangles.iter().flatten().copied())
    }
    /// Test the triangle union, dilated by a non-negative local hit tolerance.
    /// Tolerance is applied after dash splitting; it does not shift dash phase.
    pub fn hit_test(&self, at: Point, tolerance: f32) -> bool {
        if !at.is_finite() || !tolerance.is_finite() || tolerance < 0.0 {
            return false;
        }
        self.triangles.iter().any(|triangle| {
            let [a, b, c] = triangle.map(kurbo_point);
            let triangle_shape = kurbo::Triangle::new(a, b, c);
            // Degenerate triangles have no painted area, including on an edge.
            !triangle_shape.is_zero_area()
                && (triangle_shape.contains(kurbo_point(at))
                    || (0..3).any(|i| {
                        crate::geometry::distance_to_segment(at, triangle[i], triangle[(i + 1) % 3])
                            <= tolerance
                    }))
        })
    }
}
impl Shape {
    /// Build a bounded centered-stroke mesh. Text, image and group do not have
    /// vector stroke semantics and are explicitly unsupported here.
    pub fn stroke_mesh(&self, stroke: &Stroke) -> Result<StrokeMesh, String> {
        stroke.validate()?;
        check_source_budget(self)?;
        self.validate()?;
        if stroke.width == 0.0 || !stroke.paint.is_visible() {
            return Ok(StrokeMesh::default());
        }
        let contours = centerline(self)?;
        let path = stroke_path(&contours, stroke)?;
        let options = StrokeOptions::default()
            .with_line_width(stroke.width)
            .with_line_cap(match stroke.cap {
                StrokeCap::Butt => LineCap::Butt,
                StrokeCap::Square => LineCap::Square,
                StrokeCap::Round => LineCap::Round,
            })
            .with_line_join(match stroke.join {
                StrokeJoin::Bevel => LineJoin::Bevel,
                StrokeJoin::Miter => LineJoin::Miter,
                StrokeJoin::Round => LineJoin::Round,
            })
            .with_miter_limit(stroke.miter_limit)
            .with_tolerance(STROKE_GEOMETRY_TOLERANCE);
        let mut output = MeshBuilder::default();
        StrokeTessellator::new()
            .tessellate(path.iter(), &options, &mut output)
            .map_err(|error| format!("stroke tessellation failed: {error}"))?;
        if output.exceeded {
            return Err("stroke mesh budget exceeded".into());
        }
        let mesh = StrokeMesh {
            triangles: output.triangles,
        };
        if mesh.bounds().is_some_and(|bounds| !bounds.is_valid()) {
            return Err("stroke mesh bounds overflow".into());
        }
        Ok(mesh)
    }
}

fn check_source_budget(shape: &Shape) -> Result<(), String> {
    let count = match shape {
        Shape::Path(path) => path.commands.len(),
        Shape::Pen(pen) => pen.path.commands.len(),
        Shape::Polygon(polygon) => polygon.points.len(),
        Shape::Custom(custom) => {
            // Empty contours count too, so many empty subpaths cannot evade the
            // source-work limit before Custom::validate rejects them.
            let mut count = custom.contours.len();
            for contour in &custom.contours {
                count = count.saturating_add(contour.points.len());
                if count > MAX_STROKE_POINTS {
                    break;
                }
            }
            count
        }
        Shape::Text(_) | Shape::Image(_) | Shape::Group(_) => {
            return Err("advanced strokes require vector geometry".into());
        }
        // These primitives validate their size/count and emit at most 8192
        // vertices (star) or 4100 (rounded rectangle).
        _ => 0,
    };
    if count > MAX_STROKE_POINTS {
        return Err("stroke source budget exceeded".into());
    }
    Ok(())
}
fn centerline(shape: &Shape) -> Result<Vec<Contour>, String> {
    let options = FlattenOptions {
        tolerance: STROKE_GEOMETRY_TOLERANCE,
        ..FlattenOptions::default()
    };
    let contours = match shape {
        Shape::Path(path) => path.flatten_bounded(options, MAX_STROKE_POINTS)?,
        Shape::Pen(pen) => pen.path.flatten_bounded(options, MAX_STROKE_POINTS)?,
        _ => shape.flatten(STROKE_GEOMETRY_TOLERANCE),
    };
    let count: usize = contours.iter().map(|contour| contour.points.len()).sum();
    if count > MAX_STROKE_POINTS {
        return Err("stroke flattened point budget exceeded".into());
    }
    if contours
        .iter()
        .flat_map(|c| &c.points)
        .any(|p| !p.is_finite())
    {
        return Err("stroke centerline overflow".into());
    }
    Ok(contours)
}
fn kurbo_point(p: Point) -> kurbo::Point {
    kurbo::Point::new(p.x as f64, p.y as f64)
}
fn lyon_point(p: kurbo::Point) -> Result<lyon_tessellation::math::Point, String> {
    let p = point(p.x as f32, p.y as f32);
    if !p.x.is_finite() || !p.y.is_finite() {
        return Err("stroke dash coordinates overflow".into());
    }
    Ok(p)
}
fn stroke_path(contours: &[Contour], stroke: &Stroke) -> Result<Path, String> {
    let mut input = Vec::new();
    let mut length = 0.0f64;
    for contour in contours {
        let Some(first) = contour.points.first().copied() else {
            continue;
        };
        input.push(PathEl::MoveTo(kurbo_point(first)));
        let mut previous = first;
        for p in contour.points.iter().skip(1).copied() {
            length += kurbo_point(previous).distance(kurbo_point(p));
            input.push(PathEl::LineTo(kurbo_point(p)));
            previous = p;
        }
        if contour.closed {
            length += kurbo_point(previous).distance(kurbo_point(first));
            input.push(PathEl::ClosePath);
        }
    }
    if input.len() > MAX_STROKE_EVENTS {
        return Err("stroke path event budget exceeded".into());
    }
    let mut pattern: Vec<f64> = stroke.dash_pattern.iter().map(|d| *d as f64).collect();
    // SVG odd dash arrays repeat twice. Kurbo 0.11.3's dash_impl normalizes
    // offsets by sum(dashes), so give it the even array explicitly. In
    // particular, offset == sum(odd_array) must invert on/off, not reset phase.
    if pattern.len() % 2 == 1 {
        pattern.extend_from_within(..);
    }
    let events: Vec<PathEl> = if pattern.is_empty() {
        input
    } else {
        let minimum = pattern.iter().copied().fold(f64::INFINITY, f64::min);
        // Every interior dash transition consumes at least minimum local units.
        // Each subpath can additionally start on a partial interval. The factor
        // two and per-input slack bound both output and Kurbo's first-dash stash
        // work, BEFORE asking the iterator for any output. Only line segments
        // enter Kurbo: length/inversion are constant-time, with no curve solver.
        let work = 2.0 * (length / minimum).ceil() + 4.0 * input.len() as f64;
        if !work.is_finite() || work > MAX_STROKE_EVENTS as f64 {
            return Err("stroke dash work budget exceeded".into());
        }
        let events: Vec<_> = kurbo::dash(input.into_iter(), stroke.dash_offset as f64, &pattern)
            .take(MAX_STROKE_EVENTS + 1)
            .collect();
        if events.len() > MAX_STROKE_EVENTS {
            return Err("stroke dash event budget exceeded".into());
        }
        events
    };
    let mut builder = Path::builder();
    let mut open = false;
    for event in events {
        match event {
            PathEl::MoveTo(p) => {
                if open {
                    builder.end(false);
                }
                builder.begin(lyon_point(p)?);
                open = true;
            }
            PathEl::LineTo(p) => {
                if !open {
                    return Err("stroke dash output has no subpath start".into());
                }
                builder.line_to(lyon_point(p)?);
            }
            PathEl::ClosePath => {
                if open {
                    builder.end(true);
                    open = false;
                }
            }
            // Input is already a bounded polyline; dashing cannot add curves.
            PathEl::QuadTo(..) | PathEl::CurveTo(..) => {
                return Err("unexpected curved dash output".into());
            }
        }
    }
    if open {
        builder.end(false);
    }
    Ok(builder.build())
}

#[derive(Default)]
struct MeshBuilder {
    vertices: Vec<Point>,
    triangles: Vec<[Point; 3]>,
    exceeded: bool,
}
impl GeometryBuilder for MeshBuilder {
    fn add_triangle(&mut self, a: VertexId, b: VertexId, c: VertexId) {
        if self.triangles.len() == MAX_STROKE_MESH_ITEMS {
            self.exceeded = true;
            return;
        }
        self.triangles.push([
            self.vertices[a.0 as usize],
            self.vertices[b.0 as usize],
            self.vertices[c.0 as usize],
        ]);
    }
    fn abort_geometry(&mut self) {
        self.vertices.clear();
        self.triangles.clear();
    }
}
impl StrokeGeometryBuilder for MeshBuilder {
    fn add_stroke_vertex(
        &mut self,
        vertex: StrokeVertex,
    ) -> Result<VertexId, GeometryBuilderError> {
        if self.exceeded || self.vertices.len() == MAX_STROKE_MESH_ITEMS {
            self.exceeded = true;
            return Err(GeometryBuilderError::TooManyVertices);
        }
        let position = vertex.position();
        let point = Point::new(position.x, position.y);
        if !point.is_finite() {
            return Err(GeometryBuilderError::InvalidVertex);
        }
        let id = VertexId(self.vertices.len() as u32);
        self.vertices.push(point);
        Ok(id)
    }
}
