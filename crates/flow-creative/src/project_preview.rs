//! Read-only artwork framing for recent-project covers, through the same Scene
//! compositor as Frame PNG. No document mutation, URI reads or preview files.
use crate::{ProjectAsset, frame_png::FramePngExport, tr};
use gpui_flow_core::{Graph, Group, Point, Rect, Selection, Size, hidden_node_ids};

pub const PROJECT_PREVIEW_SIZE: [u32; 2] = [267, 156];

pub struct ProjectPreview {
    pub export: FramePngExport,
    pub label: &'static str,
}

/// Existing Frames: use the first Frame with visible members in stable ID order.
/// Its unsupported members reject the preview instead of being silently omitted.
/// Ungrouped artwork uses visible object bounds plus 12 world units of padding;
/// editor chrome, connections and selection decorations are not artwork.
pub fn prepare_project_preview(
    graph: &Graph,
    assets: &[ProjectAsset],
) -> Result<ProjectPreview, String> {
    let hidden = hidden_node_ids(graph);
    let mut selection = Selection::default();
    if !graph.groups.is_empty() {
        let frame = graph
            .groups
            .values()
            .find(|frame| frame.node_ids.iter().any(|id| !hidden.contains(id)))
            .ok_or("No Frame has visible artwork")?;
        selection.groups.insert(frame.id.clone());
        return Ok(ProjectPreview {
            export: FramePngExport::prepare_preview(
                graph,
                &selection,
                assets,
                PROJECT_PREVIEW_SIZE,
            )?,
            label: tr("画框预览", "Frame preview"),
        });
    }
    let visible = graph
        .nodes
        .values()
        .filter(|node| !hidden.contains(&node.id))
        .collect::<Vec<_>>();
    if visible.is_empty() {
        return Err("No visible artwork to preview".into());
    }
    if visible
        .iter()
        .any(|node| !node.bounds().is_valid() || !node.size.is_positive())
    {
        return Err("Invalid artwork bounds".into());
    }
    let bounds = visible
        .iter()
        .map(|node| node.bounds())
        .reduce(Rect::union)
        .expect("nonempty");
    let bounds = Rect::new(
        bounds.origin - Point::new(12., 12.),
        Size::new(bounds.size.width + 24., bounds.size.height + 24.),
    );
    let mut frame = Group::new("project-preview", "Canvas overview", bounds)
        .with_nodes(visible.iter().map(|node| node.id.clone()));
    frame.data = serde_json::json!({"flow_creative_frame":1});
    selection.groups.insert(frame.id.clone());
    let mut candidate = graph.clone();
    candidate
        .add_group(frame)
        .map_err(|error| error.to_string())?;
    Ok(ProjectPreview {
        export: FramePngExport::prepare_preview(
            &candidate,
            &selection,
            assets,
            PROJECT_PREVIEW_SIZE,
        )?,
        label: tr("画布概览", "Canvas overview"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        shapes::{ShapeData, ShapeKind, create_shape_node},
        standard_registry,
    };
    fn shape(id: &str, at: Point) -> gpui_flow_core::Node {
        create_shape_node(
            id,
            "Box",
            at,
            Size::new(240., 160.),
            ShapeData::new(ShapeKind::Rectangle),
        )
        .unwrap()
    }
    #[test]
    fn preview_preserves_authored_geometry_and_skips_hidden_objects() {
        let mut graph = Graph::default();
        graph
            .add_node(shape("visible", Point::new(100., 200.)))
            .unwrap();
        graph
            .add_node(shape("hidden", Point::new(50_000., 20.)))
            .unwrap();
        gpui_flow_core::set_node_visible(&mut graph, &"hidden".into(), false).unwrap();
        let original = graph.clone();
        let preview = prepare_project_preview(&graph, &[]).unwrap();
        assert_eq!(preview.label, "画布概览");
        assert_eq!(preview.export.object_count(), 1);
        assert_eq!(
            preview.export.bounds(),
            Rect::new(Point::new(88., 188.), Size::new(264., 184.))
        );
        assert_eq!(graph, original);
        let size = preview.export.pixel_size();
        assert!(size[0] <= 267 && size[1] <= 156);
    }
    #[test]
    fn frame_preview_uses_exact_crop_and_does_not_include_outside_nodes() {
        let mut graph = Graph::default();
        graph
            .add_node(shape("member", Point::new(100., 200.)))
            .unwrap();
        graph
            .add_node(
                standard_registry()
                    .make_node("generate_video", "outside", Point::ZERO, None)
                    .unwrap(),
            )
            .unwrap();
        let bounds = Rect::new(Point::new(90., 190.), Size::new(8000., 6000.));
        graph
            .add_group(Group::new("frame", "Frame", bounds).with_nodes(["member".into()]))
            .unwrap();
        let original = graph.clone();
        let preview = prepare_project_preview(&graph, &[]).unwrap();
        assert_eq!(preview.label, "画框预览");
        assert_eq!(preview.export.bounds(), bounds);
        assert_eq!(preview.export.object_count(), 1);
        assert_eq!(graph, original);
        let mut selection = Selection::default();
        selection.groups.insert("frame".into());
        assert!(FramePngExport::prepare(&graph, &selection, &[]).is_err());
        assert!(FramePngExport::prepare_preview(&graph, &selection, &[], [0, 156]).is_err());
        assert!(FramePngExport::prepare_preview(&graph, &selection, &[], [513, 156]).is_err());
    }
    #[test]
    fn unsupported_visible_content_is_not_silently_filtered() {
        let mut graph = Graph::default();
        graph.add_node(shape("shape", Point::ZERO)).unwrap();
        graph
            .add_node(
                standard_registry()
                    .make_node("generate_video", "video", Point::ZERO, None)
                    .unwrap(),
            )
            .unwrap();
        assert!(prepare_project_preview(&graph, &[]).is_err());
        graph
            .add_group(
                Group::new(
                    "a",
                    "Unsupported",
                    Rect::new(Point::ZERO, Size::new(400., 300.)),
                )
                .with_nodes(["video".into()]),
            )
            .unwrap();
        graph
            .add_group(
                Group::new(
                    "b",
                    "Supported",
                    Rect::new(Point::ZERO, Size::new(400., 300.)),
                )
                .with_nodes(["shape".into()]),
            )
            .unwrap();
        assert!(prepare_project_preview(&graph, &[]).is_err());
    }
    #[test]
    fn empty_canvas_and_frames_have_no_fake_artwork() {
        let mut graph = Graph::default();
        assert!(prepare_project_preview(&graph, &[]).is_err());
        graph
            .add_group(Group::new(
                "empty",
                "Empty",
                Rect::new(Point::ZERO, Size::new(400., 300.)),
            ))
            .unwrap();
        assert!(prepare_project_preview(&graph, &[]).is_err());
    }
}
