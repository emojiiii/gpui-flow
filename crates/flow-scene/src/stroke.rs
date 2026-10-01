//! Shared stroke geometry for alignment and non-default cap/join/dash styles.
//!
//! Kurbo 0.11.3 supplies dash splitting; Lyon 1.0.22 supplies all stroke
//! expansion/tessellation. Both are MIT OR Apache-2.0. This adapter only owns
//! shape conversion, limits, finite checks, and picking of the resulting mesh.
use crate::{
    Bounds, Contour, FillRule, FlattenOptions, Point, Shape, Stroke, StrokeAlignment, StrokeCap,
    StrokeJoin,
};
use i_overlay::{
    core::{fill_rule::FillRule as OverlayFillRule, overlay::Overlay, overlay_rule::OverlayRule},
    i_float::{adapter::FloatPointAdapter, float::rect::FloatRect, int::point::IntPoint},
};
use kurbo::{PathEl, Shape as _};
use lyon_tessellation::{
    FillGeometryBuilder, FillOptions, FillTessellator, FillVertex, GeometryBuilder,
    GeometryBuilderError, StrokeGeometryBuilder, StrokeOptions, StrokeTessellator, StrokeVertex,
    VertexId,
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
/// Aligned strokes additionally bound Boolean input edges and output vertices.
pub const MAX_ALIGNED_STROKE_EDGES: usize = 4096;
pub const MAX_ALIGNED_STROKE_OUTPUT_POINTS: usize = 16_384;
/// Combined precision point/edge tests, worst-case Boolean pair tests
/// n*(n-1)/2 and edge work, checked BEFORE each operation (not output-only).
pub const MAX_ALIGNED_STROKE_INTERSECTION_WORK: usize = 2_097_152;
/// Coordinates of source and expanded stroke must stay in this local range.
/// The 1/4096 Boolean grid plus f32 conversion error is below 0.004 local units.
pub const MAX_ALIGNED_STROKE_COORDINATE: f32 = 65_536.0;
/// Smaller nonzero aligned widths cannot reliably survive f32 extrusion at
/// the supported coordinate limit. Centered strokes keep their old contract.
pub const MIN_ALIGNED_STROKE_WIDTH: f32 = 1.0 / 64.0;
const ALIGNED_STROKE_GRID_SCALE: f64 = 4096.0;

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
    /// Backward-compatible entry point using NonZero fill semantics. Scene
    /// consumers must use `stroke_mesh_with_rule` with their Style's fill rule.
    pub fn stroke_mesh(&self, stroke: &Stroke) -> Result<StrokeMesh, String> {
        self.stroke_mesh_with_rule(stroke, FillRule::NonZero)
    }
    /// Build the shared local stroke triangle union. Inside/Outside clip a
    /// doubled centered stroke against the geometry's filled region, regardless
    /// of whether the object has a fill paint. Text/Image/Group are unsupported.
    pub fn stroke_mesh_with_rule(
        &self,
        stroke: &Stroke,
        fill_rule: FillRule,
    ) -> Result<StrokeMesh, String> {
        stroke.validate()?;
        check_source_budget(self)?;
        self.validate()?;
        let aligned = stroke.alignment != StrokeAlignment::Center;
        // Keep legacy inactive centered strokes free of flatten/tessellation
        // requirements. Aligned inputs still validate their closed domain.
        if !aligned && (stroke.width == 0.0 || !stroke.paint.is_visible()) {
            return Ok(StrokeMesh::default());
        }
        let contours = centerline(self)?;
        if aligned {
            check_aligned_contours(&contours, stroke)?;
        }
        if stroke.width == 0.0 || !stroke.paint.is_visible() {
            return Ok(StrokeMesh::default());
        }
        let path = stroke_path(&contours, stroke)?;
        let options = StrokeOptions::default()
            .with_line_width(if aligned {
                stroke.width * 2.0
            } else {
                stroke.width
            })
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
        let mesh = if aligned {
            align_mesh(output.triangles, &contours, stroke.alignment, fill_rule)?
        } else {
            StrokeMesh {
                triangles: output.triangles,
            }
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

impl FillGeometryBuilder for MeshBuilder {
    fn add_fill_vertex(&mut self, vertex: FillVertex) -> Result<VertexId, GeometryBuilderError> {
        if self.exceeded || self.vertices.len() == MAX_STROKE_MESH_ITEMS {
            self.exceeded = true;
            return Err(GeometryBuilderError::TooManyVertices);
        }
        let p = vertex.position();
        let point = Point::new(p.x, p.y);
        if !point.is_finite() {
            return Err(GeometryBuilderError::InvalidVertex);
        }
        let id = VertexId(self.vertices.len() as u32);
        self.vertices.push(point);
        Ok(id)
    }
}

fn check_aligned_point(p: Point) -> Result<(), String> {
    if !p.is_finite() || p.x.abs().max(p.y.abs()) > MAX_ALIGNED_STROKE_COORDINATE {
        return Err("aligned stroke coordinate precision limit exceeded".into());
    }
    Ok(())
}
fn check_aligned_contours(contours: &[Contour], stroke: &Stroke) -> Result<(), String> {
    let edges = contours.iter().map(|c| c.points.len()).sum::<usize>();
    if edges > MAX_ALIGNED_STROKE_EDGES {
        return Err("aligned stroke Boolean input edge budget exceeded".into());
    }
    if contours.is_empty() || contours.iter().any(|c| !c.closed || c.points.len() < 3) {
        return Err("aligned strokes require nonempty closed vector contours".into());
    }
    if stroke.width > MAX_ALIGNED_STROKE_COORDINATE {
        return Err("aligned stroke width exceeds the coordinate precision limit".into());
    }
    if stroke.width > 0.0 && stroke.width < MIN_ALIGNED_STROKE_WIDTH {
        return Err("aligned stroke width is below the precision limit".into());
    }
    for c in contours {
        for &p in &c.points {
            check_aligned_point(p)?;
        }
        // Signed area alone would incorrectly reject a self-intersecting bow tie.
        let a = c.points[0];
        let noncollinear = c.points.windows(2).any(|p| {
            let (b, d) = (p[0], p[1]);
            (b.x as f64 - a.x as f64) * (d.y as f64 - a.y as f64)
                != (b.y as f64 - a.y as f64) * (d.x as f64 - a.x as f64)
        });
        if !noncollinear {
            return Err("aligned stroke contour is degenerate".into());
        }
    }
    Ok(())
}
struct OverlayBudget(usize);
impl OverlayBudget {
    fn charge(&mut self, edges: usize) -> Result<(), String> {
        if edges > MAX_ALIGNED_STROKE_EDGES {
            return Err("aligned stroke Boolean input edge budget exceeded".into());
        }
        let work = edges.saturating_mul(edges.saturating_sub(1)) / 2 + edges;
        self.0 = self
            .0
            .checked_sub(work)
            .ok_or("aligned stroke Boolean intersection work budget exceeded")?;
        Ok(())
    }
}
type IntContours = Vec<Vec<IntPoint<i32>>>;
fn output_budget(shapes: &[IntContours]) -> Result<(), String> {
    let count = shapes.iter().flatten().map(Vec::len).sum::<usize>();
    if count > MAX_ALIGNED_STROKE_OUTPUT_POINTS {
        Err("aligned stroke Boolean output point budget exceeded".into())
    } else {
        Ok(())
    }
}
fn align_mesh(
    triangles: Vec<[Point; 3]>,
    contours: &[Contour],
    alignment: StrokeAlignment,
    fill_rule: FillRule,
) -> Result<StrokeMesh, String> {
    // Bound conversion and overlay allocations before collecting another copy.
    let source_edges = contours.iter().map(|c| c.points.len()).sum::<usize>();
    let stroke_edges = triangles.len().saturating_mul(3);
    if source_edges.saturating_add(stroke_edges) > MAX_ALIGNED_STROKE_EDGES {
        return Err("aligned stroke Boolean input edge budget exceeded".into());
    }
    for &p in triangles.iter().flatten() {
        check_aligned_point(p)?;
    }
    // The grid origin is fixed too: independently aligned layers must not
    // acquire different filled boundaries merely because their widths differ.
    let limit = MAX_ALIGNED_STROKE_COORDINATE as f64;
    let rect = FloatRect::new(-limit, limit, -limit, limit)
        .map_err(|e| format!("aligned stroke precision bounds failed: {e:?}"))?;
    let adapter = FloatPointAdapter::<[f64; 2], i32>::try_with_scale_conservative(
        rect,
        ALIGNED_STROKE_GRID_SCALE,
    )
    .map_err(|e| format!("aligned stroke precision conversion failed: {e:?}"))?;
    let convert = |p: Point| {
        adapter
            .try_float_to_int(&[p.x as f64, p.y as f64])
            .map_err(|e| format!("aligned stroke coordinate conversion failed: {e:?}"))
    };
    let fill = contours
        .iter()
        .map(|c| c.points.iter().copied().map(&convert).collect())
        .collect::<Result<IntContours, String>>()?;
    let mut budget = OverlayBudget(MAX_ALIGNED_STROKE_INTERSECTION_WORK);
    // Precision guard is quadratic too, and is charged before examining pairs.
    budget.0 = budget
        .0
        .checked_sub(source_edges.saturating_mul(source_edges))
        .ok_or("aligned stroke precision work budget exceeded")?;
    check_fill_precision(contours, &fill)?;
    budget.charge(source_edges)?;
    // Canonicalize the fill first: its winding may differ from the stroke union.
    let region = Overlay::from_subj(&fill).overlay(
        OverlayRule::Subject,
        match fill_rule {
            FillRule::EvenOdd => OverlayFillRule::EvenOdd,
            FillRule::NonZero => OverlayFillRule::NonZero,
        },
    );
    output_budget(&region)?;
    let mut stroke = Vec::with_capacity(triangles.len());
    for triangle in triangles {
        let [a, mut b, mut c] = [
            convert(triangle[0])?,
            convert(triangle[1])?,
            convert(triangle[2])?,
        ];
        let area = (b.x as i64 - a.x as i64) * (c.y as i64 - a.y as i64)
            - (b.y as i64 - a.y as i64) * (c.x as i64 - a.x as i64);
        if area == 0 {
            let [x, y, z] = triangle;
            let original_area = (y.x as f64 - x.x as f64) * (z.y as f64 - x.y as f64)
                - (y.y as f64 - x.y as f64) * (z.x as f64 - x.x as f64);
            if original_area != 0.0 {
                return Err("aligned stroke triangle collapses below grid precision".into());
            }
            continue;
        }
        if area < 0 {
            std::mem::swap(&mut b, &mut c);
        }
        stroke.push(vec![a, b, c]);
    }
    if stroke.is_empty() {
        return Err("aligned stroke extrusion is below precision limit".into());
    }
    let region_edges = region.iter().flatten().map(Vec::len).sum::<usize>();
    budget.charge(stroke.len() * 3 + region_edges)?;
    // All triangles have the same winding. NonZero unions overlap instead of
    // punching EvenOdd holes at joins, dash caps and self-intersections.
    let result = Overlay::from_subj_and_clip(&stroke, &region).overlay(
        match alignment {
            StrokeAlignment::Inside => OverlayRule::Intersect,
            StrokeAlignment::Outside => OverlayRule::Difference,
            StrokeAlignment::Center => {
                unreachable!("only aligned meshes enter the Boolean adapter")
            }
        },
        OverlayFillRule::NonZero,
    );
    output_budget(&result)?;
    let mut builder = Path::builder();
    for contour in result.iter().flatten() {
        let converted = contour
            .iter()
            .map(|p| {
                let f = adapter
                    .try_int_to_float(p)
                    .map_err(|e| format!("aligned stroke output conversion failed: {e:?}"))?;
                let p = Point::new(f[0] as f32, f[1] as f32);
                check_aligned_point(p)?;
                Ok(p)
            })
            .collect::<Result<Vec<_>, String>>()?;
        if converted.len() < 3 {
            return Err("aligned stroke output contour is degenerate".into());
        }
        for i in 0..converted.len() {
            let next = (i + 1) % converted.len();
            if converted[i] == converted[next] && contour[i] != contour[next] {
                return Err("aligned stroke output edge collapses below f32 precision".into());
            }
        }
        let a = converted[0];
        if !converted.windows(2).any(|p| {
            (p[0].x as f64 - a.x as f64) * (p[1].y as f64 - a.y as f64)
                != (p[0].y as f64 - a.y as f64) * (p[1].x as f64 - a.x as f64)
        }) {
            return Err("aligned stroke output contour collapses below f32 precision".into());
        }
        builder.begin(point(a.x, a.y));
        for p in &converted[1..] {
            builder.line_to(point(p.x, p.y));
        }
        builder.end(true);
    }
    let mut output = MeshBuilder::default();
    FillTessellator::new()
        .tessellate_path(
            &builder.build(),
            &FillOptions::default()
                .with_fill_rule(lyon_tessellation::FillRule::NonZero)
                .with_tolerance(STROKE_GEOMETRY_TOLERANCE),
            &mut output,
        )
        .map_err(|e| format!("aligned stroke tessellation failed: {e}"))?;
    if output.exceeded {
        return Err("aligned stroke mesh budget exceeded".into());
    }
    Ok(StrokeMesh {
        triangles: output.triangles,
    })
}

/// Refuse input features close enough to merge on the fixed Boolean grid.
/// Exact coincident points/edges and real crossings remain legal (fill rules
/// decide their meaning). Curve flattening still uses the documented 0.1 unit
/// approximation; topology below that curve approximation is not guaranteed.
fn check_fill_precision(contours: &[Contour], quantized: &IntContours) -> Result<(), String> {
    for (source, ints) in contours.iter().zip(quantized) {
        for i in 0..ints.len() {
            let next = (i + 1) % ints.len();
            if source.points[i] != source.points[next] && ints[i] == ints[next] {
                return Err("aligned stroke contour edge collapses below grid precision".into());
            }
        }
        let a = ints[0];
        if !ints.windows(2).any(|p| {
            (p[0].x as i64 - a.x as i64) * (p[1].y as i64 - a.y as i64)
                != (p[0].y as i64 - a.y as i64) * (p[1].x as i64 - a.x as i64)
        }) {
            return Err("aligned stroke contour collapses below grid precision".into());
        }
    }
    let clearance = (4.0 / ALIGNED_STROKE_GRID_SCALE) as f32;
    for &p in contours.iter().flat_map(|c| &c.points) {
        for c in contours {
            for i in 0..c.points.len() {
                let (a, b) = (c.points[i], c.points[(i + 1) % c.points.len()]);
                if p == a || p == b {
                    continue;
                }
                let distance = crate::geometry::distance_to_segment(p, a, b);
                if distance > 0.0 && distance < clearance {
                    return Err("aligned stroke contour clearance is below grid precision".into());
                }
            }
        }
    }
    Ok(())
}
