//! Built-in payload conventions; node kinds remain open strings for application extensions.
use gpui_flow_core::*;
use serde_json::json;

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
/// Build a node with typed default ports. Media sources are metadata only until your renderer loads them.
pub fn builtin_node(kind: &str, id: impl Into<NodeId>, position: Point) -> Node {
    let mut node = Node::new(id, kind, position).with_kind(kind);
    node.title = match kind {
        "text" => "Prompt / 文本",
        "image" => "Image / 图片",
        "video" => "Video / 视频",
        "audio" => "Audio / 音频",
        "generation" => "Generate / 生成",
        "note" => "Note / 便签",
        "agent" => "Agent / 智能体",
        _ => "Output / 输出",
    }
    .into();
    node.size = Size::new(252., 190.);
    node.data = match kind {
        "text" => json!({"text":"A quiet landscape, soft light, cinematic composition"}),
        "image" => {
            json!({"source":"","alt":"Image preview","caption":"Connect your own image asset"})
        }
        "video" => {
            json!({"source":"","duration":12,"caption":"Video metadata · decoder not bundled"})
        }
        "audio" => {
            json!({"source":"","duration":24,"caption":"Audio metadata · playback adapter needed"})
        }
        "generation" | "agent" => {
            json!({"prompt":"Transform the connected inputs","provider":"mock","model":"offline-demo"})
        }
        "note" => {
            json!({"text":"Drag nodes · connect typed ports\nAll generation in this demo is MOCK"})
        }
        _ => json!({"text":"Collect generated results here"}),
    };
    node.ports = match kind {
        "text" => vec![Port::output("out", "Text", DataType::String)],
        "image" | "video" | "audio" => vec![
            Port::input("in", "Asset", DataType::Named(kind.into())),
            Port::output("out", "Asset", DataType::Named(kind.into())),
        ],
        "generation" => vec![
            Port::input("prompt", "Prompt", DataType::String),
            Port::input("reference", "Reference", DataType::Named("image".into())),
            Port::output("out", "Image", DataType::Named("image".into())),
        ],
        "agent" => vec![
            Port::input("in", "Context", DataType::Any),
            Port::output("out", "Result", DataType::Any),
        ],
        "note" => vec![],
        _ => vec![Port::input("in", "Result", DataType::Any).with_capacity(None)],
    };
    node
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
