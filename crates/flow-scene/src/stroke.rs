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

/// Shared allocation/work allowance for Canvas-lowered strokes. One instance
/// spans all commands; geometry-only failures never return a partial mesh.
#[derive(Debug, Clone, PartialEq)]
pub struct StrokeGeometryBudget {
    per_path_points: usize,
    points: usize,
    events: usize,
    triangles: usize,
    bytes: u64,
}
impl StrokeGeometryBudget {
    pub fn new(per_path_points: usize, total_points: usize, triangles: usize, bytes: u64) -> Self {
        Self {
            per_path_points: per_path_points.min(MAX_STROKE_POINTS),
            points: total_points,
            events: total_points.saturating_mul(4),
            triangles,
            bytes,
        }
    }
    fn charge_bytes(&mut self, bytes: u64) -> Result<(), String> {
        self.bytes = self
            .bytes
            .checked_sub(bytes)
            .ok_or("stroke allocation budget exceeded")?;
        Ok(())
    }
    /// Reserve the scan-converter's triangle-union path before building it.
    /// Includes geometric growth and temporary old/new vector allocations.
    pub fn charge_render_path(&mut self, triangles: usize) -> Result<(), String> {
        self.charge_bytes(
            (triangles as u64).saturating_mul(4 * (3 * size_of::<Point>() as u64 + 4)),
        )
    }
}

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
    /// Centered Canvas lowering with a shared batch budget. Reduced caller
    /// limits apply before flattening, dash generation and mesh growth.
    pub fn stroke_mesh_with_rule_bounded(
        &self,
        stroke: &Stroke,
        _fill_rule: FillRule,
        budget: &mut StrokeGeometryBudget,
    ) -> Result<StrokeMesh, String> {
        stroke.validate()?;
        check_source_budget(self)?;
        self.validate()?;
        if !stroke.scale_fixed.is_disabled() {
            return Err("bounded Canvas strokes do not support scale-fixed widths".into());
        }
        if stroke.alignment != StrokeAlignment::Center {
            return Err("bounded strokes require centered geometry".into());
        }
        if stroke.width == 0. || !stroke.paint.is_visible() {
            return Ok(StrokeMesh::default());
        }
        // Four times point+contour storage bounds geometric Vec growth and a
        // temporary allocation during growth, even for one-point subpaths.
        let point_bytes = 4 * (size_of::<Point>() + size_of::<Contour>()) as u64;
        let cap = budget
            .per_path_points
            .min(budget.points)
            .min((budget.bytes / point_bytes) as usize);
        let options = FlattenOptions {
            tolerance: STROKE_GEOMETRY_TOLERANCE,
            ..Default::default()
        };
        let contours = match self {
            Shape::Path(path) => path.flatten_bounded(options, cap)?,
            Shape::Pen(pen) => pen.path.flatten_bounded(options, cap)?,
            _ => return Err("bounded strokes require path geometry".into()),
        };
        let count: usize = contours.iter().map(|c| c.points.len()).sum();
        budget.points = budget
            .points
            .checked_sub(count)
            .ok_or("stroke flattened point budget exceeded")?;
        let bytes = contours.capacity() as u64 * size_of::<Contour>() as u64
            + contours
                .iter()
                .map(|c| c.points.capacity() as u64 * size_of::<Point>() as u64)
                .sum::<u64>();
        budget.charge_bytes(bytes)?;
        if contours
            .iter()
            .flat_map(|c| &c.points)
            .any(|p| !p.is_finite())
        {
            return Err("stroke centerline overflow".into());
        }
        let (path, _) = stroke_path_impl(&contours, stroke, Some(budget))?;
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
        let mut output = MeshBuilder {
            max_triangles: budget.triangles.min(MAX_STROKE_MESH_ITEMS),
            max_bytes: budget.bytes,
            ..Default::default()
        };
        StrokeTessellator::new()
            .tessellate(path.iter(), &options, &mut output)
            .map_err(|error| format!("stroke tessellation failed: {error}"))?;
        if output.exceeded {
            return Err("stroke mesh budget exceeded".into());
        }
        budget.triangles = budget
            .triangles
            .checked_sub(output.triangles.len())
            .ok_or("stroke mesh budget exceeded")?;
        budget.charge_bytes(output.allocated_bytes)?;
        let mesh = StrokeMesh {
            triangles: output.triangles,
        };
        if mesh.bounds().is_some_and(|bounds| !bounds.is_valid()) {
            return Err("stroke mesh bounds overflow".into());
        }
        Ok(mesh)
    }
    /// Build the shared local stroke triangle union. Inside/Outside clip a
    /// doubled centered stroke against the geometry's filled region, regardless
    /// of whether the object has a fill paint. Text/Image/Group are unsupported.
    pub fn stroke_mesh_with_rule(
        &self,
        stroke: &Stroke,
        fill_rule: FillRule,
    ) -> Result<StrokeMesh, String> {
        let resolved = stroke.resolved(crate::StrokeScaleData::IDENTITY, 1.0)?;
        let stroke = &resolved;
        stroke.validate()?;
        check_source_budget(self)?;
        self.validate()?;
        let aligned = stroke.alignment != StrokeAlignment::Center;
        // Keep legacy inactive centered strokes free of flatten/tessellation
        // requirements. Aligned inputs still validate their vector domain.
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
        let (path, expects_ink) = stroke_path(&contours, stroke)?;
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
            align_mesh(
                output.triangles,
                &contours,
                stroke.alignment,
                fill_rule,
                expects_ink,
            )?
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
        // These primitives validate size/count and emit at most 8192
        // points (star and bounded ellipse/rectangle curve adapters).
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
fn stroke_path(contours: &[Contour], stroke: &Stroke) -> Result<(Path, bool), String> {
    stroke_path_impl(contours, stroke, None)
}
fn stroke_path_impl(
    contours: &[Contour],
    stroke: &Stroke,
    budget: Option<&mut StrokeGeometryBudget>,
) -> Result<(Path, bool), String> {
    // Compute the same dash work bound before allocating its input or asking
    // Kurbo for output. The reservation also covers Lyon path-builder storage.
    if let Some(budget) = budget {
        let count: usize = contours
            .iter()
            .map(|c| c.points.len() + usize::from(c.closed))
            .sum();
        let length: f64 = contours
            .iter()
            .map(|c| {
                let lines: f64 = c
                    .points
                    .windows(2)
                    .map(|p| kurbo_point(p[0]).distance(kurbo_point(p[1])))
                    .sum();
                lines
                    + if c.closed && !c.points.is_empty() {
                        kurbo_point(c.points[0]).distance(kurbo_point(*c.points.last().unwrap()))
                    } else {
                        0.
                    }
            })
            .sum();
        let events = if stroke.dash_pattern.is_empty() {
            count
        } else {
            let minimum = stroke
                .dash_pattern
                .iter()
                .map(|v| *v as f64)
                .fold(f64::INFINITY, f64::min);
            let work = 2. * (length / minimum).ceil() + 4. * count as f64;
            if !work.is_finite() || work > MAX_STROKE_EVENTS as f64 {
                return Err("stroke dash work budget exceeded".into());
            }
            work as usize
        };
        if events > MAX_STROKE_EVENTS {
            return Err("stroke dash event budget exceeded".into());
        }
        budget.events = budget
            .events
            .checked_sub(events)
            .ok_or("stroke dash work budget exceeded")?;
        // Four-times growth allowance covers input/output/event and point Vecs,
        // including Kurbo's first-dash stash and Lyon's owned path copy.
        let bytes = (count as u64)
            .saturating_mul(4 * size_of::<PathEl>() as u64)
            .saturating_add((events as u64).saturating_mul(
                4 * (2 * size_of::<PathEl>() as u64 + size_of::<Point>() as u64 + 16),
            ));
        budget.charge_bytes(bytes.saturating_add(
            (stroke.dash_pattern.len() as u64).saturating_mul(4 * size_of::<f64>() as u64),
        ))?;
    }
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
    // Record expected ink AFTER dash splitting but BEFORE f32 conversion and
    // Lyon merging. Empty/move-only/all-gap paths are legal; losing a real
    // segment or a requested zero-length cap during extrusion is not.
    let mut expects_ink = false;
    let mut current = kurbo::Point::ZERO;
    let mut start = current;
    for event in events {
        match event {
            PathEl::MoveTo(p) => {
                if open {
                    builder.end(false);
                }
                builder.begin(lyon_point(p)?);
                current = p;
                start = p;
                open = true;
            }
            PathEl::LineTo(p) => {
                if !open {
                    return Err("stroke dash output has no subpath start".into());
                }
                expects_ink |= current != p || stroke.cap != StrokeCap::Butt;
                current = p;
                builder.line_to(lyon_point(p)?);
            }
            PathEl::ClosePath => {
                if open {
                    expects_ink |= current != start || stroke.cap != StrokeCap::Butt;
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
    Ok((builder.build(), expects_ink))
}

struct MeshBuilder {
    vertices: Vec<Point>,
    triangles: Vec<[Point; 3]>,
    exceeded: bool,
    max_triangles: usize,
    max_bytes: u64,
    allocated_bytes: u64,
}
impl Default for MeshBuilder {
    fn default() -> Self {
        Self {
            vertices: vec![],
            triangles: vec![],
            exceeded: false,
            max_triangles: MAX_STROKE_MESH_ITEMS,
            max_bytes: u64::MAX,
            allocated_bytes: 0,
        }
    }
}
fn reserve_mesh<T>(
    values: &mut Vec<T>,
    allocated: &mut u64,
    max_bytes: u64,
    max_items: usize,
) -> bool {
    if values.len() >= max_items {
        return false;
    }
    if values.len() == values.capacity() {
        let capacity = values.capacity().saturating_mul(2).max(4).min(max_items);
        let bytes = (capacity as u64).saturating_mul(size_of::<T>() as u64);
        if allocated.saturating_add(bytes) > max_bytes {
            return false;
        }
        *allocated += bytes;
        if values.try_reserve_exact(capacity - values.len()).is_err() {
            return false;
        }
    }
    true
}
impl GeometryBuilder for MeshBuilder {
    fn add_triangle(&mut self, a: VertexId, b: VertexId, c: VertexId) {
        if self.exceeded
            || !reserve_mesh(
                &mut self.triangles,
                &mut self.allocated_bytes,
                self.max_bytes,
                self.max_triangles,
            )
        {
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
        if self.exceeded
            || !reserve_mesh(
                &mut self.vertices,
                &mut self.allocated_bytes,
                self.max_bytes,
                MAX_STROKE_MESH_ITEMS,
            )
        {
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
        if self.exceeded
            || !reserve_mesh(
                &mut self.vertices,
                &mut self.allocated_bytes,
                self.max_bytes,
                MAX_STROKE_MESH_ITEMS,
            )
        {
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
    // One potential fill edge per point, including each open contour's
    // implicit closing edge. Empty and move-only paths remain bounded too.
    let edges = contours.iter().map(|c| c.points.len()).sum::<usize>();
    if edges > MAX_ALIGNED_STROKE_EDGES {
        return Err("aligned stroke Boolean input edge budget exceeded".into());
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
        // Closed degenerate contours retain the existing admission boundary.
        // Open collinear contours have an empty fill mask, but their original
        // centerlines (including zero-length caps) must still be stroked.
        if c.closed && !noncollinear(&c.points) {
            return Err("aligned stroke closed contour is degenerate".into());
        }
    }
    Ok(())
}
// Do not use signed area: bow ties can have zero signed area but a nonempty
// fill region. Choose a distinct anchor so repeated/backtracked points cannot
// hide a later non-collinear point.
fn noncollinear(points: &[Point]) -> bool {
    let Some(&a) = points.first() else {
        return false;
    };
    let Some(&b) = points.iter().find(|&&p| p != a) else {
        return false;
    };
    points.iter().any(|p| {
        (b.x as f64 - a.x as f64) * (p.y as f64 - a.y as f64)
            != (b.y as f64 - a.y as f64) * (p.x as f64 - a.x as f64)
    })
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
    expects_ink: bool,
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
    let quantized = contours
        .iter()
        .map(|c| c.points.iter().copied().map(&convert).collect())
        .collect::<Result<IntContours, String>>()?;
    let mut budget = OverlayBudget(MAX_ALIGNED_STROKE_INTERSECTION_WORK);
    // Precision guard is quadratic too, and is charged before examining pairs.
    budget.0 = budget
        .0
        .checked_sub(source_edges.saturating_mul(source_edges))
        .ok_or("aligned stroke precision work budget exceeded")?;
    check_fill_precision(contours, &quantized)?;
    // Fill/clip implicitly closes open contours, without ever closing their
    // stroke centerlines. Mathematically collinear open contours contribute no
    // region. Still validate and charge their source edges above; a nonempty
    // region that collapsed on the grid must never be mistaken for this case.
    let fill: IntContours = contours
        .iter()
        .zip(quantized)
        .filter_map(|(c, ints)| noncollinear(&c.points).then_some(ints))
        .collect();
    budget.charge(source_edges)?;
    // Canonicalize the fill first: its winding may differ from the stroke union.
    let region = if fill.is_empty() {
        Vec::new()
    } else {
        Overlay::from_subj(&fill).overlay(
            OverlayRule::Subject,
            match fill_rule {
                FillRule::EvenOdd => OverlayFillRule::EvenOdd,
                FillRule::NonZero => OverlayFillRule::NonZero,
            },
        )
    };
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
        // Empty/move-only subpaths and butt-capped zero-length segments have
        // genuinely no ink. The post-dash event contract also catches strokes
        // lost BEFORE integer conversion, including all-degenerate f32 meshes.
        if expects_ink {
            return Err("aligned stroke extrusion is below precision limit".into());
        }
        return Ok(StrokeMesh::default());
    }
    let region_edges = region.iter().flatten().map(Vec::len).sum::<usize>();
    budget.charge(stroke.len() * 3 + region_edges)?;
    // All triangles have the same winding. NonZero unions overlap instead of
    // punching EvenOdd holes at joins, dash caps and self-intersections.
    let result = if region.is_empty() {
        match alignment {
            StrokeAlignment::Inside => return Ok(StrokeMesh::default()),
            StrokeAlignment::Outside => {
                Overlay::from_subj(&stroke).overlay(OverlayRule::Subject, OverlayFillRule::NonZero)
            }
            StrokeAlignment::Center => {
                unreachable!("only aligned meshes enter the Boolean adapter")
            }
        }
    } else {
        Overlay::from_subj_and_clip(&stroke, &region).overlay(
            match alignment {
                StrokeAlignment::Inside => OverlayRule::Intersect,
                StrokeAlignment::Outside => OverlayRule::Difference,
                StrokeAlignment::Center => {
                    unreachable!("only aligned meshes enter the Boolean adapter")
                }
            },
            OverlayFillRule::NonZero,
        )
    };
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
        if noncollinear(&source.points) {
            let a = ints[0];
            let b = *ints
                .iter()
                .find(|&&p| p != a)
                .ok_or("aligned stroke contour collapses below grid precision")?;
            if !ints.iter().any(|p| {
                (b.x as i64 - a.x as i64) * (p.y as i64 - a.y as i64)
                    != (b.y as i64 - a.y as i64) * (p.x as i64 - a.x as i64)
            }) {
                return Err("aligned stroke contour collapses below grid precision".into());
            }
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
                // Exact on-edge points are legal. Avoid a tiny residual from
                // projection/division falsely rejecting a collinear centerline.
                let (dx, dy) = (b.x as f64 - a.x as f64, b.y as f64 - a.y as f64);
                let (px, py) = (p.x as f64 - a.x as f64, p.y as f64 - a.y as f64);
                let dot = px * dx + py * dy;
                let squared_length = dx * dx + dy * dy;
                if squared_length > 0.0 && px * dy == py * dx && dot >= 0.0 && dot <= squared_length
                {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canvas_lowered_stroke_matches_unbounded_curved_dash_mesh() {
        let shape = Shape::Path(
            crate::display::Path::from_svg("M 0 0 Q 10 20 20 0 C 30 -5 30 15 20 20 Z").unwrap(),
        );
        let stroke = Stroke::new(crate::Color::WHITE, 2.)
            .with_dashes(vec![3., 2., 1.], -2.)
            .with_cap(StrokeCap::Square)
            .with_join(StrokeJoin::Miter);
        let expected = shape.stroke_mesh(&stroke).unwrap();
        let mut budget = StrokeGeometryBudget::new(65536, 262144, 262144, 16 * 1024 * 1024);
        let actual = shape
            .stroke_mesh_with_rule_bounded(&stroke, FillRule::NonZero, &mut budget)
            .unwrap();
        assert_eq!(actual, expected);
        assert!(
            budget.points < 262144 && budget.events < 262144 * 4 && budget.bytes < 16 * 1024 * 1024
        );
    }
    #[test]
    fn canvas_lowered_stroke_shared_triangle_budget_never_resets_between_jobs() {
        let shape = Shape::Path(crate::display::Path::from_svg("M 1 1 L 9 1").unwrap());
        let stroke = Stroke::new(crate::Color::WHITE, 2.);
        let count = shape.stroke_mesh(&stroke).unwrap().triangles().len();
        let mut budget = StrokeGeometryBudget::new(100, 100, count, 1_000_000);
        shape
            .stroke_mesh_with_rule_bounded(&stroke, FillRule::NonZero, &mut budget)
            .unwrap();
        assert_eq!(budget.triangles, 0);
        assert!(
            shape
                .stroke_mesh_with_rule_bounded(&stroke, FillRule::NonZero, &mut budget)
                .is_err()
        );
    }
    #[test]
    fn canvas_lowered_stroke_reduced_flatten_dash_and_allocation_budgets_fail_closed() {
        let curved =
            Shape::Path(crate::display::Path::from_svg("M 0 0 C 100 100 -100 100 0 0").unwrap());
        let stroke = Stroke::new(crate::Color::WHITE, 2.);
        assert!(
            curved
                .stroke_mesh_with_rule_bounded(
                    &stroke,
                    FillRule::NonZero,
                    &mut StrokeGeometryBudget::new(4, 4, 100, 1_000_000)
                )
                .is_err()
        );
        assert!(
            curved
                .stroke_mesh_with_rule_bounded(
                    &stroke,
                    FillRule::NonZero,
                    &mut StrokeGeometryBudget::new(1000, 1000, 1000, 16)
                )
                .is_err()
        );
        let path = Shape::Path(crate::display::Path::from_svg("M 0 0 L 100 0").unwrap());
        let dashed = stroke.with_dashes(vec![0.001, 0.001], 0.);
        assert!(
            path.stroke_mesh_with_rule_bounded(
                &dashed,
                FillRule::NonZero,
                &mut StrokeGeometryBudget::new(100, 100, 1000, 1_000_000)
            )
            .is_err()
        );
    }

    #[test]
    fn lost_ink_is_rejected_for_empty_and_all_degenerate_extrusion() {
        let contours = [Contour {
            points: vec![
                Point::new(60000., 60000.),
                Point::new(60_000.0 + 1.0 / 256.0, 60000.),
            ],
            closed: false,
        }];
        for triangles in [Vec::new(), vec![[Point::new(60000., 60000.); 3]]] {
            for alignment in [StrokeAlignment::Inside, StrokeAlignment::Outside] {
                assert!(
                    align_mesh(
                        triangles.clone(),
                        &contours,
                        alignment,
                        FillRule::NonZero,
                        true
                    )
                    .unwrap_err()
                    .contains("precision")
                );
                // This flag comes from post-dash events; an all-gap path has
                // no expected ink even when its original centerline is nonempty.
                assert!(
                    align_mesh(
                        triangles.clone(),
                        &contours,
                        alignment,
                        FillRule::NonZero,
                        false
                    )
                    .unwrap()
                    .triangles()
                    .is_empty()
                );
            }
        }
    }
}
