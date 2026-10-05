//! Stable type colors, shared by sockets, committed edges and drag previews.
//! The optional document map is deliberately separate from port type contracts.
use gpui_flow_core::{DataType, Graph};

pub fn port_type_key(data_type: &DataType) -> String {
    match data_type {
        DataType::Any => "any".into(),
        DataType::Bool => "bool".into(),
        DataType::Number => "number".into(),
        DataType::String => "string".into(),
        DataType::Object => "object".into(),
        DataType::Named(name) => format!("named:{name}"),
    }
}
pub fn default_port_color(data_type: &DataType) -> u32 {
    match data_type {
        DataType::String => 0x4785e7,
        DataType::Number => 0xe5a534,
        DataType::Bool => 0xe2789d,
        DataType::Object => 0x77869b,
        DataType::Any => 0x9b9fa8,
        DataType::Named(name) => match name.as_str() {
            "image" => 0xa274e7,
            "video" => 0xea9062,
            "audio" => 0x47ad96,
            "model" => 0x6881db,
            "palette" => 0xd172a7,
            _ => {
                const COLORS: [u32; 6] =
                    [0x4785e7, 0xa274e7, 0xea9062, 0x47ad96, 0xd172a7, 0xe5a534];
                let hash = name.bytes().fold(0u32, |hash, byte| {
                    hash.wrapping_mul(31).wrapping_add(byte as u32)
                });
                COLORS[hash as usize % COLORS.len()]
            }
        },
    }
}
pub fn graph_port_color(graph: &Graph, data_type: &DataType) -> u32 {
    graph
        .data
        .get("port_colors")
        .and_then(|v| v.get(port_type_key(data_type)))
        .and_then(|v| v.as_u64())
        .filter(|v| *v <= 0xffffff)
        .map(|v| v as u32)
        .unwrap_or_else(|| default_port_color(data_type))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn type_color_is_global_and_customizable_without_port_mutation() {
        let mut graph = Graph::new();
        let image = DataType::Named("image".into());
        assert_ne!(
            graph_port_color(&graph, &image),
            graph_port_color(&graph, &DataType::String)
        );
        graph.data = serde_json::json!({"port_colors":{"named:image":0x123456}});
        assert_eq!(graph_port_color(&graph, &image), 0x123456);
        assert_eq!(
            graph_port_color(&graph, &DataType::String),
            default_port_color(&DataType::String)
        );
        graph.data["port_colors"]["named:image"] = serde_json::json!(0x1000000u32);
        assert_eq!(graph_port_color(&graph, &image), default_port_color(&image));
    }
}
