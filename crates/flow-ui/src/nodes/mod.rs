//! Built-in payload conventions; node kinds remain open strings for application extensions.
use gpui_flow_core::*;

pub const BUILTIN_KINDS: &[&str] = &[
    "text",
    "image",
    "video",
    "audio",
    "generation",
    "note",
    "output",
    "agent",
];
pub fn kind_color(kind: &str) -> u32 {
    match kind {
        "text" => 0xa8b5ff,
        "image" => 0xf5b971,
        "video" => 0xe19cff,
        "audio" => 0x72cbf4,
        "generation" | "agent" => 0x63e2bf,
        "note" => 0xf4d477,
        _ => 0x9cafc9,
    }
}
pub mod agent;
pub mod audio;
mod common;
pub mod generation;
pub mod image;
pub mod note;
pub mod output;
pub mod text;
pub mod video;

pub use agent::*;
pub use audio::*;
pub use generation::*;
pub use image::*;
pub use note::*;
pub use output::*;
pub use text::*;
pub use video::*;

/// Build a graph node. UI components below are also usable without a graph.
pub fn builtin_node(kind: &str, id: impl Into<NodeId>, position: Point) -> Node {
    let id = id.into();
    match kind {
        "text" => text::model(id, position),
        "image" => image::model(id, position),
        "video" => video::model(id, position),
        "audio" => audio::model(id, position),
        "generation" => generation::model(id, position),
        "note" => note::model(id, position),
        "agent" => agent::model(id, position),
        _ => output::model(id, position).with_kind(kind),
    }
}
/// Reusable fallback dispatcher; custom registries can replace any body independently.
pub fn builtin_node_body(node: &Node, context: crate::NodeRenderContext) -> gpui::AnyElement {
    match node.kind.as_str() {
        "text" => text_node_body(node, context),
        "image" => image_node_body(node, context),
        "video" => video_node_body(node, context),
        "audio" => audio_node_body(node, context),
        "generation" => generation_node_body(node, context),
        "note" => note_node_body(node, context),
        "agent" => agent_node_body(node, context),
        _ => output_node_body(node, context),
    }
}
/// A compact, deterministic offline sample graph.
pub fn demo_graph() -> Graph {
    let mut graph = Graph::new();
    for (id, kind, x, y) in [
        ("prompt", "text", 60., 90.),
        ("reference", "image", 60., 330.),
        ("generate", "generation", 420., 170.),
        ("result", "output", 800., 170.),
        ("video", "video", 800., 420.),
        ("audio", "audio", 420., 460.),
        ("note", "note", 60., 580.),
    ] {
        graph
            .add_node(builtin_node(kind, id, Point::new(x, y)))
            .expect("valid demo node");
    }
    for (id, from, port, to, input) in [
        ("e1", "prompt", "out", "generate", "prompt"),
        ("e2", "reference", "out", "generate", "reference"),
        ("e3", "generate", "out", "result", "in"),
    ] {
        graph
            .connect(id, PortRef::new(from, port), PortRef::new(to, input))
            .expect("valid demo connection");
    }
    graph
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_payloads_and_demo_remain_valid() {
        for kind in BUILTIN_KINDS {
            let node = builtin_node(kind, *kind, Point::ZERO);
            assert_eq!(&node.kind, kind);
            let mut graph = Graph::new();
            graph.add_node(node).unwrap();
        }
        let graph = demo_graph();
        assert_eq!(graph.nodes.len(), 7);
        assert_eq!(graph.edges.len(), 3);
        assert!(builtin_node("note", "note", Point::ZERO).ports.is_empty());
        assert_eq!(builtin_node("custom", "custom", Point::ZERO).kind, "custom");
    }

    #[test]
    fn bodies_parse_payloads_and_media_is_explicitly_host_owned() {
        let image = ImageNode::from_node(&builtin_node("image", "image", Point::ZERO));
        assert!(image.source.is_none());
        let mut video_model = builtin_node("video", "video", Point::ZERO);
        assert_eq!(VideoNode::from_node(&video_model).duration, None);
        video_model.data["duration"] = serde_json::json!(12.);
        assert_eq!(VideoNode::from_node(&video_model).duration, Some(12.));
        let audio_model = builtin_node("audio", "audio", Point::ZERO);
        assert_eq!(AudioNode::from_node(&audio_model).duration, None);
        let audio = AudioNode::new("track.wav").waveform([f32::NAN, -0.5, 2.]);
        assert_eq!(audio.waveform, vec![0., 0.5, 1.]);
        assert!(GenerationNode::new("Host configured").provider.is_empty());
        let generation = GenerationNode::from_node(&builtin_node("generation", "run", Point::ZERO));
        assert_eq!(generation.provider.as_ref(), "mock");
    }
}
