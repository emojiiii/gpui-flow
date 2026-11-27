//! Immutable, bounded SVG snapshots of native shape and pencil selections or a
//! single Frame's visible vector members, clipped to its exact bounds.
//!
//! Only validated paint and geometry are serialized. Host metadata, node IDs,
//! external resources, and arbitrary SVG/CSS never become document markup.
//! This module performs no file I/O and never changes the graph or selection.

use crate::pencil::{PENCIL_NODE_KIND, PencilData};
use crate::shapes::{SHAPE_NODE_KIND, ShapeData, ShapeKind};
use crate::tr;
use gpui_flow_core::{
    Graph, GroupId, NodeId, Point, Rect, Selection, Size, hidden_node_ids, ordered_node_ids,
};
use std::fmt::{self, Write};

pub const MAX_VECTOR_EXPORT_OBJECTS: usize = 128;
pub const MAX_VECTOR_EXPORT_POINTS: usize = 65_536;
pub const MAX_VECTOR_EXPORT_SPAN: f32 = 32_768.0;
pub const MAX_VECTOR_EXPORT_BYTES: usize = 4 * 1024 * 1024;

/// A validated export request, isolated from subsequent editor changes.
/// Equality covers selected identities, paint, geometry, titles and draw order,
/// plus the source Frame and its visible membership. Hidden member content,
/// unrelated metadata and locks do not change the exported picture.
#[derive(Debug, Clone, PartialEq)]
pub struct VectorExport {
    objects: Vec<VectorObject>,
    bounds: Rect,
    suggested_name: String,
    frame: Option<VectorFrame>,
}

#[derive(Debug, Clone, PartialEq)]
struct VectorFrame {
    id: GroupId,
    title: String,
    white_background: bool,
}

#[derive(Debug, Clone, PartialEq)]
struct VectorObject {
    id: NodeId,
    title: String,
    position: Point,
    size: Size,
    primitive: Primitive,
}

#[derive(Debug, Clone, PartialEq)]
enum Primitive {
    Shape(ShapeData),
    Pencil(PencilData),
}

impl VectorExport {
    /// Exact world-space Frame bounds, or direct-node paint bounds rounded
    /// outward with one unit of extra padding.
    pub fn bounds(&self) -> Rect {
        self.bounds
    }

    pub fn suggested_name(&self) -> &str {
        &self.suggested_name
    }

    pub fn object_count(&self) -> usize {
        self.objects.len()
    }

    pub fn is_frame(&self) -> bool {
        self.frame.is_some()
    }

    /// Matches the native renderer's Creative Frame marker. Legacy groups have
    /// a transparent background; neither group borders nor titles are painted.
    pub fn has_white_background(&self) -> bool {
        self.frame
            .as_ref()
            .is_some_and(|frame| frame.white_background)
    }

    /// Reject a stale save request if selection, visibility or exported content
    /// changed while the destination picker was open.
    pub fn matches(&self, graph: &Graph, selection: &Selection) -> bool {
        prepare_vector_export(graph, selection).is_ok_and(|current| current == *self)
    }

    /// Standalone UTF-8 SVG. All coordinates are translated into the bounded
    /// viewport; opacity composites each object's fill and stroke exactly once.
    pub fn to_svg(&self) -> Result<Vec<u8>, String> {
        let mut svg = SvgWriter::default();
        self.write_svg(&mut svg)
            .map_err(|_| "SVG export exceeds the 4 MiB byte limit".to_string())?;
        Ok(svg.text.into_bytes())
    }

    fn write_svg(&self, svg: &mut SvgWriter) -> fmt::Result {
        writeln!(svg, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>")?;
        writeln!(
            svg,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{}\" height=\"{}\" viewBox=\"0 0 {} {}\">",
            self.bounds.size.width,
            self.bounds.size.height,
            self.bounds.size.width,
            self.bounds.size.height,
        )?;
        if self.is_frame() {
            // This fixed local fragment is generated here, never read from
            // graph metadata. Clip in viewport coordinates before translating
            // each member, including stroke paint crossing any frame edge.
            writeln!(
                svg,
                "  <defs><clipPath id=\"frame-clip\" clipPathUnits=\"userSpaceOnUse\"><rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\"/></clipPath></defs>",
                self.bounds.size.width, self.bounds.size.height,
            )?;
            svg.write_str("  <g clip-path=\"url(#frame-clip)\">\n")?;
            if self.has_white_background() {
                writeln!(
                    svg,
                    "    <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" fill=\"#ffffff\" stroke=\"none\"/>",
                    self.bounds.size.width, self.bounds.size.height,
                )?;
            }
        }
        for object in &self.objects {
            let x = f64::from(object.position.x) - f64::from(self.bounds.origin.x);
            let y = f64::from(object.position.y) - f64::from(self.bounds.origin.y);
            let opacity = match &object.primitive {
                Primitive::Shape(shape) => shape.style.opacity,
                Primitive::Pencil(pencil) => pencil.style.opacity,
            };
            writeln!(
                svg,
                "  <g transform=\"translate({x} {y})\" opacity=\"{opacity}\">"
            )?;
            svg.write_str("    <title>")?;
            write_xml_text(svg, &object.title)?;
            svg.write_str("</title>\n")?;
            match &object.primitive {
                Primitive::Shape(shape) => write_shape(svg, object.size, *shape)?,
                Primitive::Pencil(pencil) => write_pencil(svg, object.size, pencil)?,
            }
            svg.write_str("  </g>\n")?;
        }
        if self.is_frame() {
            svg.write_str("  </g>\n")?;
        }
        svg.write_str("</svg>\n")
    }
}

/// Prepare every directly selected node, or the visible members of exactly one
/// selected Frame. Unsupported visible kinds, edges, mixed selections and
/// missing members reject the whole request. Hidden direct selections reject;
/// hidden Frame members are skipped just as they are by the native renderer.
/// Locks protect editing, so locked Frames and visible members remain exportable.
pub fn prepare_vector_export(graph: &Graph, selection: &Selection) -> Result<VectorExport, String> {
    if !selection.edges.is_empty() {
        return Err(tr("仅支持形状和笔迹或单个纯矢量画框，请取消选择连接", "Select shapes, strokes, or a single vector-only frame. Deselect the connections before exporting.").into());
    }
    if selection.groups.len() > 1 || (!selection.groups.is_empty() && !selection.nodes.is_empty()) {
        return Err("Export one Frame by itself, without other Frames or direct objects".into());
    }
    let selected_frame = selection
        .groups
        .iter()
        .next()
        .map(|id| {
            let frame = graph
                .groups
                .get(id)
                .ok_or("The selected SVG export Frame no longer exists")?;
            if &frame.id != id || frame.id.as_str().trim().is_empty() || frame.title.len() > 512 {
                return Err("SVG Frame requires a matching nonempty ID and title of at most 512 bytes");
            }
            let bounds = frame.bounds;
            // Preserve the original origin and dimensions, without padding,
            // snapping or the editing normalizer's minimum Frame dimensions.
            if !bounds.is_valid()
                || !bounds.size.is_positive()
                || bounds.size.width > MAX_VECTOR_EXPORT_SPAN
                || bounds.size.height > MAX_VECTOR_EXPORT_SPAN
                || bounds.max_x() <= bounds.min_x()
                || bounds.max_y() <= bounds.min_y()
            {
                return Err("SVG Frame bounds must be finite, positive and no larger than 32768 units per axis");
            }
            Ok(frame)
        })
        .transpose()?;
    let node_ids = selected_frame.map_or(&selection.nodes, |frame| &frame.node_ids);
    if selected_frame.is_none() && node_ids.is_empty() {
        return Err("Select at least one shape or pencil stroke to export".into());
    }
    let hidden = hidden_node_ids(graph);
    let mut visible_count = 0usize;
    for id in node_ids {
        if !graph.nodes.contains_key(id) {
            return Err("A selected SVG export object or Frame member no longer exists".into());
        }
        if hidden.contains(id) {
            if selected_frame.is_none() {
                return Err(tr(
                    "所选对象包含隐藏图层，请先显示该图层或取消选择",
                    "The selection contains a hidden layer. Show or deselect it before exporting.",
                )
                .into());
            }
        } else {
            visible_count += 1;
            if visible_count > MAX_VECTOR_EXPORT_OBJECTS {
                return Err("SVG export supports at most 128 visible objects".into());
            }
        }
    }
    if selected_frame.is_some() && visible_count == 0 {
        return Err("The selected Frame has no visible shape or pencil members to export".into());
    }

    let mut objects = Vec::with_capacity(visible_count);
    let mut points = 0usize;
    let mut paint_bounds: Option<PaintBounds> = None;
    for id in ordered_node_ids(graph) {
        if !node_ids.contains(&id) || hidden.contains(&id) {
            continue;
        }
        let node = &graph.nodes[&id];
        if node.id != id {
            return Err("SVG export object identity does not match the graph".into());
        }
        let primitive = match node.kind.as_str() {
            SHAPE_NODE_KIND => Primitive::Shape(ShapeData::from_node(node)?),
            PENCIL_NODE_KIND => {
                // Check the aggregate before allocating another decoded array.
                let count = node.data["pencil"]["points"].as_array().map_or(0, Vec::len);
                points = points.saturating_add(count);
                if points > MAX_VECTOR_EXPORT_POINTS {
                    return Err("SVG export supports at most 65536 pencil points".into());
                }
                Primitive::Pencil(PencilData::from_node(node)?)
            }
            _ => {
                return Err(
                    tr("仅支持形状和笔迹，所选对象或画框含有不支持的可见文字、媒体或生成节点", "Export supports shapes and strokes. Remove visible text, media, or generation nodes from the selection or frame.").into(),
                );
            }
        };
        // Both parsers validate the title's 512-byte budget before this clone.
        let object = VectorObject {
            id,
            title: node.title.clone(),
            position: node.position,
            size: node.size,
            primitive,
        };
        let bounds = object.paint_bounds();
        paint_bounds = Some(paint_bounds.map_or(bounds, |previous| previous.union(bounds)));
        objects.push(object);
    }
    let bounds = if let Some(frame) = selected_frame {
        frame.bounds
    } else {
        paint_bounds
            .ok_or("Select at least one shape or pencil stroke to export")?
            .padded_rect()?
    };
    let suggested_name = if let Some(frame) = selected_frame {
        suggested_name(&frame.title)
    } else if objects.len() == 1 {
        suggested_name(&objects[0].title)
    } else {
        "Vector-selection.svg".into()
    };
    Ok(VectorExport {
        objects,
        bounds,
        suggested_name,
        frame: selected_frame.map(|frame| VectorFrame {
            id: frame.id.clone(),
            title: frame.title.clone(),
            // Keep precisely the same marker interpretation as render.rs.
            white_background: frame.data["flow_creative_frame"].as_u64() == Some(1),
        }),
    })
}

impl VectorObject {
    fn paint_bounds(&self) -> PaintBounds {
        let (mut bounds, stroke_radius) = match &self.primitive {
            Primitive::Shape(shape) => {
                let bounds = if let Some(line) = shape.line {
                    PaintBounds::from_points(
                        local_point(line.start, self.size),
                        local_point(line.end, self.size),
                    )
                } else {
                    PaintBounds::from_points(
                        Point::ZERO,
                        Point::new(self.size.width, self.size.height),
                    )
                };
                let radius = if shape.style.stroke.is_some() {
                    shape.style.stroke_width / 2.0
                } else {
                    0.0
                };
                (bounds, radius)
            }
            Primitive::Pencil(pencil) => {
                let first = local_point(pencil.points[0], self.size);
                let bounds = pencil.points.iter().skip(1).fold(
                    PaintBounds::from_points(first, first),
                    |bounds, point| {
                        let point = local_point(*point, self.size);
                        bounds.union(PaintBounds::from_points(point, point))
                    },
                );
                (bounds, pencil.style.stroke_width / 2.0)
            }
        };
        let radius = f64::from(stroke_radius);
        bounds.min_x += f64::from(self.position.x) - radius;
        bounds.min_y += f64::from(self.position.y) - radius;
        bounds.max_x += f64::from(self.position.x) + radius;
        bounds.max_y += f64::from(self.position.y) + radius;
        bounds
    }
}

#[derive(Clone, Copy)]
struct PaintBounds {
    min_x: f64,
    min_y: f64,
    max_x: f64,
    max_y: f64,
}

impl PaintBounds {
    fn from_points(a: Point, b: Point) -> Self {
        Self {
            min_x: f64::from(a.x.min(b.x)),
            min_y: f64::from(a.y.min(b.y)),
            max_x: f64::from(a.x.max(b.x)),
            max_y: f64::from(a.y.max(b.y)),
        }
    }

    fn union(self, other: Self) -> Self {
        Self {
            min_x: self.min_x.min(other.min_x),
            min_y: self.min_y.min(other.min_y),
            max_x: self.max_x.max(other.max_x),
            max_y: self.max_y.max(other.max_y),
        }
    }

    fn padded_rect(self) -> Result<Rect, String> {
        let min_x = self.min_x.floor() - 1.0;
        let min_y = self.min_y.floor() - 1.0;
        let max_x = self.max_x.ceil() + 1.0;
        let max_y = self.max_y.ceil() + 1.0;
        let width = max_x - min_x;
        let height = max_y - min_y;
        if [min_x, min_y, max_x, max_y, width, height]
            .iter()
            .any(|value| !value.is_finite())
            || width <= 0.0
            || height <= 0.0
            || width > f64::from(MAX_VECTOR_EXPORT_SPAN)
            || height > f64::from(MAX_VECTOR_EXPORT_SPAN)
        {
            return Err(
                "SVG paint bounds must be finite and no larger than 32768 units per axis".into(),
            );
        }
        let rect = Rect::new(
            Point::new(min_x as f32, min_y as f32),
            Size::new(width as f32, height as f32),
        );
        // A distant f32 origin may swallow small offsets or stroke overshoot.
        // Do not silently crop or collapse the public bounds in that case.
        if !rect.is_valid()
            || f64::from(rect.min_x()) != min_x
            || f64::from(rect.min_y()) != min_y
            || f64::from(rect.max_x()) != max_x
            || f64::from(rect.max_y()) != max_y
        {
            return Err("SVG paint bounds exceed supported coordinate precision".into());
        }
        Ok(rect)
    }
}

fn local_point(point: Point, size: Size) -> Point {
    // Keep the same f32 multiplication as the native shape/pencil previews.
    Point::new(point.x * size.width, point.y * size.height)
}

fn write_shape(svg: &mut SvgWriter, size: Size, shape: ShapeData) -> fmt::Result {
    match shape.kind {
        ShapeKind::Rectangle => {
            let radius = shape
                .style
                .radius
                .min(size.width / 2.0)
                .min(size.height / 2.0);
            write!(
                svg,
                "    <rect x=\"0\" y=\"0\" width=\"{}\" height=\"{}\" rx=\"{radius}\" ry=\"{radius}\"",
                size.width, size.height
            )?;
        }
        ShapeKind::Ellipse => {
            let rx = size.width / 2.0;
            let ry = size.height / 2.0;
            write!(
                svg,
                "    <ellipse cx=\"{rx}\" cy=\"{ry}\" rx=\"{rx}\" ry=\"{ry}\""
            )?;
        }
        ShapeKind::Line => {
            let line = shape.line.expect("validated line endpoints");
            let start = local_point(line.start, size);
            let end = local_point(line.end, size);
            write!(
                svg,
                "    <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\"",
                start.x, start.y, end.x, end.y
            )?;
        }
    };
    svg.write_str(" fill=\"")?;
    write_color(
        svg,
        if shape.kind == ShapeKind::Line {
            None
        } else {
            shape.style.fill
        },
    )?;
    svg.write_str("\" stroke=\"")?;
    write_color(
        svg,
        shape
            .style
            .stroke
            .filter(|_| shape.style.stroke_width > 0.0),
    )?;
    writeln!(
        svg,
        "\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
        shape.style.stroke_width
    )
}

fn write_pencil(svg: &mut SvgWriter, size: Size, pencil: &PencilData) -> fmt::Result {
    let first = local_point(pencil.points[0], size);
    if pencil
        .points
        .iter()
        .all(|point| local_point(*point, size) == first)
    {
        // SVG zero-length paths differ between renderers. Match the native dot.
        return writeln!(
            svg,
            "    <circle cx=\"{}\" cy=\"{}\" r=\"{}\" fill=\"#{:06x}\" stroke=\"none\"/>",
            first.x,
            first.y,
            pencil.style.stroke_width / 2.0,
            pencil.style.color
        );
    }
    write!(svg, "    <path d=\"M {} {}", first.x, first.y)?;
    for point in pencil.points.iter().skip(1) {
        let point = local_point(*point, size);
        write!(svg, " L {} {}", point.x, point.y)?;
    }
    writeln!(
        svg,
        "\" fill=\"none\" stroke=\"#{:06x}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"/>",
        pencil.style.color, pencil.style.stroke_width
    )
}

fn write_color(svg: &mut SvgWriter, color: Option<u32>) -> fmt::Result {
    if let Some(color) = color {
        write!(svg, "#{color:06x}")
    } else {
        svg.write_str("none")
    }
}

fn write_xml_text(svg: &mut SvgWriter, text: &str) -> fmt::Result {
    for character in text.chars() {
        match character {
            '&' => svg.write_str("&amp;")?,
            '<' => svg.write_str("&lt;")?,
            '>' => svg.write_str("&gt;")?,
            '"' => svg.write_str("&quot;")?,
            '\'' => svg.write_str("&apos;")?,
            '\t'
            | '\n'
            | '\r'
            | '\u{20}'..='\u{d7ff}'
            | '\u{e000}'..='\u{fffd}'
            | '\u{10000}'..='\u{10ffff}' => svg.write_char(character)?,
            _ => svg.write_char('\u{fffd}')?,
        }
    }
    Ok(())
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
        .is_some_and(|suffix| suffix.eq_ignore_ascii_case(".svg"))
    {
        &stem[..stem.len() - 4]
    } else {
        stem
    };
    if stem.trim().is_empty() {
        "Vector.svg".into()
    } else {
        format!("{stem}.svg")
    }
}

#[derive(Default)]
struct SvgWriter {
    text: String,
}

impl Write for SvgWriter {
    fn write_str(&mut self, value: &str) -> fmt::Result {
        if value.len() > MAX_VECTOR_EXPORT_BYTES.saturating_sub(self.text.len()) {
            return Err(fmt::Error);
        }
        self.text.push_str(value);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pencil::{PencilStyle, create_pencil_node};
    use crate::shapes::{ShapeLine, ShapeStyle, create_shape_node};
    use gpui_flow_core::{Group, Node, set_node_visible, write_node_order};

    fn rectangle(id: &str) -> Node {
        create_shape_node(
            id,
            id,
            Point::new(10.25, -20.5),
            Size::new(100.0, 40.0),
            ShapeData::new(ShapeKind::Rectangle),
        )
        .unwrap()
    }

    fn selected(nodes: Vec<Node>) -> (Graph, Selection) {
        let mut graph = Graph::new();
        let mut selection = Selection::default();
        for node in nodes {
            selection.nodes.insert(node.id.clone());
            graph.add_node(node).unwrap();
        }
        (graph, selection)
    }

    fn svg(export: &VectorExport) -> String {
        String::from_utf8(export.to_svg().unwrap()).unwrap()
    }

    fn framed(nodes: Vec<Node>) -> (Graph, Selection) {
        let (mut graph, mut selection) = selected(nodes);
        let mut frame = Group::new(
            "frame",
            "Frame artboard",
            Rect::new(Point::new(20.25, -10.5), Size::new(63.5, 47.25)),
        )
        .with_nodes(selection.nodes.iter().cloned());
        frame.data = serde_json::json!({"flow_creative_frame": 1});
        graph.add_group(frame).unwrap();
        selection.select_group("frame", false);
        (graph, selection)
    }

    #[test]
    fn frame_preserves_exact_viewport_clips_crossing_paint_and_keeps_graph_order() {
        let mut front = rectangle("z");
        front.position = Point::new(70.75, 30.75);
        let (mut graph, selection) = framed(vec![rectangle("a"), front]);
        graph.add_node(rectangle("outside-membership")).unwrap();
        write_node_order(&mut graph, [NodeId::from("z"), NodeId::from("a")]).unwrap();
        let before = (graph.clone(), selection.clone());
        let export = prepare_vector_export(&graph, &selection).unwrap();
        assert_eq!(
            export.bounds(),
            graph.groups[&GroupId::from("frame")].bounds
        );
        assert_eq!(export.object_count(), 2);
        assert_eq!(export.suggested_name(), "Frame artboard.svg");
        assert!(export.is_frame());
        assert!(export.has_white_background());
        let text = svg(&export);
        assert!(text.contains("width=\"63.5\" height=\"47.25\" viewBox=\"0 0 63.5 47.25\""));
        assert!(text.contains("<clipPath id=\"frame-clip\" clipPathUnits=\"userSpaceOnUse\"><rect x=\"0\" y=\"0\" width=\"63.5\" height=\"47.25\"/></clipPath>"));
        let clip = text.find("<g clip-path=\"url(#frame-clip)\">").unwrap();
        let background = text.find("fill=\"#ffffff\" stroke=\"none\"").unwrap();
        let back = text.find("<title>z</title>").unwrap();
        let front = text.find("<title>a</title>").unwrap();
        assert!(clip < background && background < back && back < front);
        assert!(text.contains("translate(50.5 41.25)"));
        assert!(text.contains("translate(-10 -10)"));
        assert!(text.contains("width=\"100\" height=\"40\""));
        assert!(text.ends_with("  </g>\n  </g>\n</svg>\n"));
        assert!(!text.contains("Frame artboard"));
        assert!(!text.contains("outside-membership"));
        assert_eq!((graph.clone(), selection.clone()), before);

        graph.groups.get_mut(&GroupId::from("frame")).unwrap().data =
            serde_json::json!({"background": "url(https://example.invalid)", "onclick": "bad"});
        let legacy = prepare_vector_export(&graph, &selection).unwrap();
        assert!(legacy.is_frame());
        assert!(!legacy.has_white_background());
        assert!(!svg(&legacy).contains("#ffffff"));
        assert!(!svg(&legacy).contains("example.invalid"));
        assert!(!export.matches(&graph, &selection));
    }

    #[test]
    fn frame_skips_hidden_members_but_rejects_every_visible_unsupported_kind() {
        let (mut graph, selection) = framed(vec![rectangle("visible"), rectangle("hidden")]);
        set_node_visible(&mut graph, &NodeId::from("hidden"), false).unwrap();
        graph.nodes.get_mut(&NodeId::from("hidden")).unwrap().data = serde_json::Value::Null;
        // Hidden content is not decoded and does not consume the object budget.
        for index in 0..MAX_VECTOR_EXPORT_OBJECTS {
            let id = NodeId::from(format!("hidden-{index}"));
            graph
                .add_node(Node::new(id.clone(), "Hidden media", Point::ZERO).with_kind("image"))
                .unwrap();
            graph
                .groups
                .get_mut(&GroupId::from("frame"))
                .unwrap()
                .node_ids
                .insert(id.clone());
            set_node_visible(&mut graph, &id, false).unwrap();
        }
        let export = prepare_vector_export(&graph, &selection).unwrap();
        assert_eq!(export.object_count(), 1);
        assert!(!svg(&export).contains("<title>hidden</title>"));
        for kind in ["image", "text", "video", "model", "generate_image"] {
            graph.nodes.get_mut(&NodeId::from("hidden")).unwrap().kind = kind.into();
            assert!(export.matches(&graph, &selection));
            set_node_visible(&mut graph, &NodeId::from("hidden"), true).unwrap();
            assert!(
                prepare_vector_export(&graph, &selection)
                    .unwrap_err()
                    .contains("仅支持形状和笔迹")
            );
            assert!(!export.matches(&graph, &selection));
            set_node_visible(&mut graph, &NodeId::from("hidden"), false).unwrap();
        }
    }

    #[test]
    fn frame_empty_mixed_missing_and_invalid_requests_fail_without_mutation() {
        let (empty, selection) = framed(vec![]);
        assert!(
            prepare_vector_export(&empty, &selection)
                .unwrap_err()
                .contains("no visible")
        );
        let (mut graph, selection) = framed(vec![rectangle("a")]);
        set_node_visible(&mut graph, &NodeId::from("a"), false).unwrap();
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("no visible")
        );
        set_node_visible(&mut graph, &NodeId::from("a"), true).unwrap();
        let before = (graph.clone(), selection.clone());
        let mut mixed = selection.clone();
        mixed.select_node("a", true);
        assert!(prepare_vector_export(&graph, &mixed).is_err());
        mixed = selection.clone();
        mixed.select_edge("edge", true);
        assert!(prepare_vector_export(&graph, &mixed).is_err());
        mixed = selection.clone();
        mixed.select_group("other", true);
        assert!(prepare_vector_export(&graph, &mixed).is_err());
        mixed.select_group("missing", false);
        assert!(
            prepare_vector_export(&graph, &mixed)
                .unwrap_err()
                .contains("no longer exists")
        );
        assert_eq!((graph.clone(), selection.clone()), before);
        graph
            .groups
            .get_mut(&GroupId::from("frame"))
            .unwrap()
            .node_ids
            .insert(NodeId::from("missing"));
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("no longer exists")
        );
        graph
            .groups
            .get_mut(&GroupId::from("frame"))
            .unwrap()
            .node_ids
            .remove(&NodeId::from("missing"));
        for bounds in [
            Rect::new(Point::ZERO, Size::new(0.0, 1.0)),
            Rect::new(Point::new(f32::NAN, 0.0), Size::new(1.0, 1.0)),
            Rect::new(Point::new(f32::MAX, 0.0), Size::new(1.0, 1.0)),
            Rect::new(Point::ZERO, Size::new(MAX_VECTOR_EXPORT_SPAN + 1.0, 1.0)),
        ] {
            graph
                .groups
                .get_mut(&GroupId::from("frame"))
                .unwrap()
                .bounds = bounds;
            assert!(prepare_vector_export(&graph, &selection).is_err());
        }
        graph
            .groups
            .get_mut(&GroupId::from("frame"))
            .unwrap()
            .bounds = Rect::new(Point::ZERO, Size::new(MAX_VECTOR_EXPORT_SPAN, 0.5));
        assert_eq!(
            prepare_vector_export(&graph, &selection)
                .unwrap()
                .bounds()
                .size
                .height,
            0.5
        );
        graph.groups.get_mut(&GroupId::from("frame")).unwrap().title = "x".repeat(513);
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("512")
        );
    }

    #[test]
    fn frame_snapshot_tracks_source_and_visible_members_while_ignoring_locks() {
        let (mut graph, selection) =
            framed(vec![rectangle("a"), rectangle("z"), rectangle("hidden")]);
        set_node_visible(&mut graph, &NodeId::from("hidden"), false).unwrap();
        let export = prepare_vector_export(&graph, &selection).unwrap();
        let original = svg(&export);
        graph
            .groups
            .get_mut(&GroupId::from("frame"))
            .unwrap()
            .locked = true;
        graph.nodes.get_mut(&NodeId::from("a")).unwrap().locked = true;
        graph.groups.get_mut(&GroupId::from("frame")).unwrap().data["host"] =
            serde_json::json!({"ignored": true});
        graph
            .nodes
            .get_mut(&NodeId::from("hidden"))
            .unwrap()
            .position
            .x += 50.0;
        assert!(export.matches(&graph, &selection));
        for change in 0..10 {
            let mut changed = graph.clone();
            match change {
                0 => changed
                    .groups
                    .get_mut(&GroupId::from("frame"))
                    .unwrap()
                    .title
                    .push('!'),
                1 => {
                    changed
                        .groups
                        .get_mut(&GroupId::from("frame"))
                        .unwrap()
                        .bounds
                        .origin
                        .x += 1.0
                }
                2 => {
                    changed
                        .groups
                        .get_mut(&GroupId::from("frame"))
                        .unwrap()
                        .bounds
                        .size
                        .width += 1.0
                }
                3 => {
                    changed
                        .groups
                        .get_mut(&GroupId::from("frame"))
                        .unwrap()
                        .data["flow_creative_frame"] = serde_json::json!(0)
                }
                4 => {
                    changed
                        .groups
                        .get_mut(&GroupId::from("frame"))
                        .unwrap()
                        .node_ids
                        .remove(&NodeId::from("a"));
                }
                5 => {
                    set_node_visible(&mut changed, &NodeId::from("a"), false).unwrap();
                }
                6 => {
                    write_node_order(&mut changed, [NodeId::from("z"), NodeId::from("a")]).unwrap();
                }
                7 => {
                    changed.nodes.get_mut(&NodeId::from("a")).unwrap().data["shape"]["style"]["opacity"] =
                        serde_json::json!(0.5)
                }
                8 => {
                    changed
                        .nodes
                        .get_mut(&NodeId::from("a"))
                        .unwrap()
                        .position
                        .x += 1.0
                }
                _ => {
                    changed.nodes.remove(&NodeId::from("hidden"));
                }
            }
            assert!(
                !export.matches(&changed, &selection),
                "stale frame change {change}"
            );
        }
        let mut other = graph.groups[&GroupId::from("frame")].clone();
        other.id = GroupId::from("other");
        graph.groups.remove(&GroupId::from("frame"));
        graph.groups.insert(other.id.clone(), other);
        let mut other_selection = selection.clone();
        other_selection.select_group("other", false);
        assert!(!export.matches(&graph, &other_selection));
        assert_eq!(svg(&export), original);
    }

    #[test]
    fn frame_visible_objects_and_pencil_points_keep_existing_budgets() {
        let (mut graph, selection) = framed(
            (0..=MAX_VECTOR_EXPORT_OBJECTS)
                .map(|index| rectangle(&index.to_string()))
                .collect(),
        );
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("128")
        );
        set_node_visible(&mut graph, &NodeId::from("128"), false).unwrap();
        assert_eq!(
            prepare_vector_export(&graph, &selection)
                .unwrap()
                .object_count(),
            MAX_VECTOR_EXPORT_OBJECTS
        );
        let (mut graph, selection) = framed(
            (0..8)
                .map(|index| pencil(&index.to_string(), vec![Point::ZERO; 8192]))
                .collect(),
        );
        assert!(prepare_vector_export(&graph, &selection).is_ok());
        graph.add_node(pencil("extra", vec![Point::ZERO])).unwrap();
        graph
            .groups
            .get_mut(&GroupId::from("frame"))
            .unwrap()
            .node_ids
            .insert(NodeId::from("extra"));
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("65536")
        );
        set_node_visible(&mut graph, &NodeId::from("extra"), false).unwrap();
        assert!(prepare_vector_export(&graph, &selection).is_ok());
    }

    #[test]
    fn rectangle_preserves_paint_clamped_radius_and_single_object_opacity() {
        let shape = ShapeData {
            style: ShapeStyle {
                fill: Some(0x123456),
                stroke: Some(0xabcdef),
                stroke_width: 5.0,
                radius: 99.0,
                opacity: 0.5,
            },
            ..ShapeData::new(ShapeKind::Rectangle)
        };
        let node = create_shape_node(
            "r",
            "Rounded",
            Point::new(10.25, -20.5),
            Size::new(100.0, 40.0),
            shape,
        )
        .unwrap();
        let (graph, selection) = selected(vec![node]);
        let before = (graph.clone(), selection.clone());
        let export = prepare_vector_export(&graph, &selection).unwrap();
        assert_eq!(
            export.bounds(),
            Rect::new(Point::new(6.0, -24.0), Size::new(108.0, 47.0))
        );
        assert_eq!(export.object_count(), 1);
        assert_eq!(export.suggested_name(), "Rounded.svg");
        let text = svg(&export);
        assert!(text.contains("viewBox=\"0 0 108 47\""));
        assert!(text.contains("translate(4.25 3.5)\" opacity=\"0.5\""));
        assert!(text.contains("rx=\"20\" ry=\"20\""));
        assert!(text.contains("fill=\"#123456\" stroke=\"#abcdef\" stroke-width=\"5\""));
        assert_eq!(text.matches("opacity=").count(), 1);
        assert!(!text.contains("fill-opacity"));
        assert!(!text.contains("stroke-opacity"));
        assert_eq!((graph, selection), before);
    }

    #[test]
    fn ellipse_and_reverse_line_keep_actual_local_geometry() {
        let ellipse = create_shape_node(
            "ellipse",
            "Ellipse",
            Point::ZERO,
            Size::new(80.0, 40.0),
            ShapeData::new(ShapeKind::Ellipse),
        )
        .unwrap();
        let line = ShapeData {
            line: Some(ShapeLine {
                start: Point::new(1.0, 0.0),
                end: Point::new(0.0, 1.0),
            }),
            style: ShapeStyle {
                fill: Some(0xff0000),
                stroke_width: 10.0,
                ..Default::default()
            },
            ..ShapeData::new(ShapeKind::Line)
        };
        let line = create_shape_node(
            "line",
            "Reverse",
            Point::new(50.0, 60.0),
            Size::new(100.0, 80.0),
            line,
        )
        .unwrap();
        let (graph, selection) = selected(vec![ellipse, line]);
        let export = prepare_vector_export(&graph, &selection).unwrap();
        let text = svg(&export);
        assert!(text.contains("<ellipse cx=\"40\" cy=\"20\" rx=\"40\" ry=\"20\""));
        assert!(text.contains("<line x1=\"100\" y1=\"0\" x2=\"0\" y2=\"80\" fill=\"none\""));
        assert!(text.contains("stroke-linecap=\"round\" stroke-linejoin=\"round\""));
        assert_eq!(
            export.bounds(),
            Rect::new(Point::new(-2.0, -2.0), Size::new(158.0, 148.0))
        );
    }

    #[test]
    fn line_bounds_use_endpoints_and_only_enabled_stroke() {
        let shape = ShapeData {
            line: Some(ShapeLine {
                start: Point::new(0.8, 0.5),
                end: Point::new(0.2, 0.5),
            }),
            style: ShapeStyle {
                stroke: None,
                stroke_width: 256.0,
                ..Default::default()
            },
            ..ShapeData::new(ShapeKind::Line)
        };
        let node =
            create_shape_node("line", "Line", Point::ZERO, Size::new(100.0, 100.0), shape).unwrap();
        let (mut graph, selection) = selected(vec![node]);
        let export = prepare_vector_export(&graph, &selection).unwrap();
        assert_eq!(
            export.bounds(),
            Rect::new(Point::new(19.0, 49.0), Size::new(62.0, 2.0))
        );
        assert!(svg(&export).contains("stroke=\"none\""));
        graph.nodes.get_mut(&NodeId::from("line")).unwrap().data["shape"]["style"]["stroke"] =
            serde_json::json!(0);
        graph.nodes.get_mut(&NodeId::from("line")).unwrap().data["shape"]["style"]["stroke_width"] =
            serde_json::json!(0);
        let zero = prepare_vector_export(&graph, &selection).unwrap();
        assert_eq!(zero.bounds(), export.bounds());
        assert!(svg(&zero).contains("stroke=\"none\" stroke-width=\"0\""));
    }

    fn pencil(id: &str, points: Vec<Point>) -> Node {
        create_pencil_node(
            id,
            id,
            Point::new(-10.0, -20.0),
            Size::new(20.0, 40.0),
            PencilData {
                points,
                style: PencilStyle {
                    color: 0x012abc,
                    stroke_width: 6.0,
                    opacity: 0.25,
                },
                ..PencilData::new(vec![])
            },
        )
        .unwrap()
    }

    #[test]
    fn single_and_repeated_pencil_points_are_circles_with_stroke_diameter() {
        for points in [vec![Point::new(0.5, 0.5)], vec![Point::new(0.5, 0.5); 4]] {
            let (graph, selection) = selected(vec![pencil("dot", points)]);
            let export = prepare_vector_export(&graph, &selection).unwrap();
            assert_eq!(
                export.bounds(),
                Rect::new(Point::new(-4.0, -4.0), Size::new(8.0, 8.0))
            );
            let text = svg(&export);
            assert!(text.contains(
                "<circle cx=\"10\" cy=\"20\" r=\"3\" fill=\"#012abc\" stroke=\"none\"/>"
            ));
            assert!(text.contains("opacity=\"0.25\""));
            assert!(!text.contains("<path"));
        }
    }

    #[test]
    fn pencil_preserves_order_repeated_vertices_and_round_joins() {
        let (graph, selection) = selected(vec![pencil(
            "path",
            vec![
                Point::new(1.0, 0.0),
                Point::new(0.25, 1.0),
                Point::new(0.25, 1.0),
                Point::ZERO,
            ],
        )]);
        let export = prepare_vector_export(&graph, &selection).unwrap();
        assert_eq!(
            export.bounds(),
            Rect::new(Point::new(-14.0, -24.0), Size::new(28.0, 48.0))
        );
        let text = svg(&export);
        assert!(text.contains("d=\"M 20 0 L 5 40 L 5 40 L 0 0\""));
        assert!(text.contains("fill=\"none\" stroke=\"#012abc\" stroke-width=\"6\" stroke-linecap=\"round\" stroke-linejoin=\"round\""));
        assert_eq!(text.matches("opacity=").count(), 1);
    }

    #[test]
    fn snapshot_uses_graph_order_and_tracks_only_exported_content() {
        let (mut graph, mut selection) = selected(vec![rectangle("a"), rectangle("z")]);
        write_node_order(&mut graph, [NodeId::from("z"), NodeId::from("a")]).unwrap();
        let export = prepare_vector_export(&graph, &selection).unwrap();
        let original = svg(&export);
        assert!(
            original.find("<title>z</title>").unwrap() < original.find("<title>a</title>").unwrap()
        );
        assert_eq!(export.suggested_name(), "Vector-selection.svg");
        assert!(export.matches(&graph, &selection));
        graph.nodes.get_mut(&NodeId::from("a")).unwrap().locked = true;
        graph.nodes.get_mut(&NodeId::from("a")).unwrap().data["host"] =
            serde_json::json!({"ignored": "url(https://example.invalid)"});
        assert!(export.matches(&graph, &selection));
        write_node_order(&mut graph, [NodeId::from("a"), NodeId::from("z")]).unwrap();
        assert!(!export.matches(&graph, &selection));
        write_node_order(&mut graph, [NodeId::from("z"), NodeId::from("a")]).unwrap();
        graph.nodes.get_mut(&NodeId::from("a")).unwrap().position.x += 1.0;
        assert!(!export.matches(&graph, &selection));
        graph.nodes.get_mut(&NodeId::from("a")).unwrap().position.x -= 1.0;
        graph.nodes.get_mut(&NodeId::from("a")).unwrap().data["shape"]["style"]["opacity"] =
            serde_json::json!(0.25);
        assert!(!export.matches(&graph, &selection));
        selection.nodes.remove(&NodeId::from("z"));
        assert!(!export.matches(&graph, &selection));
        assert_eq!(svg(&export), original);
    }

    #[test]
    fn unsupported_hidden_missing_and_non_node_selections_fail_as_a_whole() {
        let (mut graph, mut selection) = selected(vec![rectangle("r")]);
        set_node_visible(&mut graph, &NodeId::from("r"), false).unwrap();
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("隐藏图层")
        );
        set_node_visible(&mut graph, &NodeId::from("r"), true).unwrap();
        selection.nodes.insert(NodeId::from("missing"));
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("no longer exists")
        );
        selection.nodes.remove(&NodeId::from("missing"));
        graph
            .add_node(Node::new("image", "Image", Point::ZERO).with_kind("image"))
            .unwrap();
        selection.nodes.insert(NodeId::from("image"));
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("仅支持形状和笔迹")
        );
        selection.nodes.remove(&NodeId::from("image"));
        selection.select_group("frame", true);
        assert!(prepare_vector_export(&graph, &selection).is_err());
        selection.groups.clear();
        selection.select_edge("edge", true);
        assert!(prepare_vector_export(&graph, &selection).is_err());
        selection.clear();
        assert!(prepare_vector_export(&graph, &selection).is_err());
    }

    #[test]
    fn titles_are_xml_text_and_unknown_metadata_is_never_markup() {
        let mut node = rectangle("\"><script>alert(1)</script>");
        node.title = "</title><script a=\"x\">&'\u{0}\u{fffe}猫".into();
        node.data["onclick"] = serde_json::json!("alert(1)");
        node.data["shape"]["style"]["css"] = serde_json::json!("url(https://example.invalid)");
        let (graph, selection) = selected(vec![node]);
        let export = prepare_vector_export(&graph, &selection).unwrap();
        let text = svg(&export);
        assert!(text.contains("&lt;/title&gt;&lt;script a=&quot;x&quot;&gt;&amp;&apos;��猫"));
        for unsafe_fragment in [
            "<script",
            "onclick",
            "url(",
            "https://example.invalid",
            "\u{0}",
            "\u{fffe}",
        ] {
            assert!(!text.contains(unsafe_fragment));
        }
        assert!(
            !export
                .suggested_name()
                .contains(['/', '\\', '<', '>', '"', ':'])
        );
        assert!(export.suggested_name().ends_with(".svg"));
        assert_eq!(suggested_name(".."), "Vector.svg");
        assert_eq!(suggested_name("drawing.SVG"), "drawing.svg");
        assert!(suggested_name(&"猫".repeat(100)).len() <= 100);
        let mut oversized = rectangle("oversized");
        oversized.title = "x".repeat(513);
        let (graph, selection) = selected(vec![oversized]);
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("512")
        );
    }

    #[test]
    fn invalid_and_unrepresentable_bounds_and_canvas_span_are_rejected() {
        let (mut graph, selection) = selected(vec![rectangle("r")]);
        for position in [
            Point::new(f32::NAN, 0.0),
            Point::new(f32::INFINITY, 0.0),
            Point::new(f32::MAX, 0.0),
        ] {
            graph.nodes.get_mut(&NodeId::from("r")).unwrap().position = position;
            assert!(prepare_vector_export(&graph, &selection).is_err());
        }
        graph.nodes.get_mut(&NodeId::from("r")).unwrap().position = Point::ZERO;
        graph.nodes.get_mut(&NodeId::from("r")).unwrap().size.width = 0.0;
        assert!(prepare_vector_export(&graph, &selection).is_err());
        let mut far = rectangle("far");
        far.position.x = MAX_VECTOR_EXPORT_SPAN;
        let (graph, selection) = selected(vec![rectangle("near"), far]);
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("32768")
        );

        let mut left = rectangle("left");
        left.position = Point::ZERO;
        left.size.width = 16384.0;
        let mut right = left.clone();
        right.id = NodeId::from("right");
        right.position.x = 16380.0;
        let (mut graph, selection) = selected(vec![left, right]);
        assert_eq!(
            prepare_vector_export(&graph, &selection)
                .unwrap()
                .bounds()
                .size
                .width,
            MAX_VECTOR_EXPORT_SPAN
        );
        graph
            .nodes
            .get_mut(&NodeId::from("right"))
            .unwrap()
            .position
            .x += 0.25;
        assert!(prepare_vector_export(&graph, &selection).is_err());
    }

    #[test]
    fn selection_point_and_output_limits_are_enforced() {
        let (graph, mut selection) = selected(
            (0..=MAX_VECTOR_EXPORT_OBJECTS)
                .map(|index| rectangle(&index.to_string()))
                .collect(),
        );
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("128")
        );
        selection.nodes.remove(&NodeId::from("128"));
        assert_eq!(
            prepare_vector_export(&graph, &selection)
                .unwrap()
                .object_count(),
            MAX_VECTOR_EXPORT_OBJECTS
        );
        let nodes = (0..8)
            .map(|index| pencil(&index.to_string(), vec![Point::ZERO; 8192]))
            .collect();
        let (mut graph, mut selection) = selected(nodes);
        assert!(prepare_vector_export(&graph, &selection).is_ok());
        let extra = pencil("extra", vec![Point::ZERO]);
        selection.nodes.insert(extra.id.clone());
        graph.add_node(extra).unwrap();
        assert!(
            prepare_vector_export(&graph, &selection)
                .unwrap_err()
                .contains("65536")
        );
        let mut writer = SvgWriter::default();
        writer
            .write_str(&"x".repeat(MAX_VECTOR_EXPORT_BYTES - 1))
            .unwrap();
        assert!(writer.write_char('猫').is_err());
        assert_eq!(writer.text.len(), MAX_VECTOR_EXPORT_BYTES - 1);
        writer.write_char('x').unwrap();
        assert!(writer.write_char('x').is_err());
        assert_eq!(writer.text.len(), MAX_VECTOR_EXPORT_BYTES);
    }
}
