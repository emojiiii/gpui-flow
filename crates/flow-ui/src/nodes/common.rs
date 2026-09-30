use crate::{FlowTheme, components::valid_zoom};
use gpui::{prelude::*, *};
use gpui_flow_core::{Node, NodeId, Point, Size};
pub(super) fn model(kind: &str, title: &str, id: NodeId, position: Point) -> Node {
    let mut node = Node::new(id, title, position).with_kind(kind);
    node.size = Size::new(252., 190.);
    node
}
pub(super) fn string(node: &Node, key: &str, fallback: &str) -> String {
    node.data
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or(fallback)
        .to_owned()
}
pub(super) fn number(node: &Node, key: &str) -> Option<f64> {
    node.data
        .get(key)
        .and_then(|v| v.as_f64())
        .filter(|v| v.is_finite() && *v >= 0.)
}
pub(super) fn body(theme: FlowTheme, zoom: f32) -> Div {
    let z = valid_zoom(zoom);
    div()
        .flex()
        .flex_col()
        .gap(px(8. * z))
        .p(px(14. * z))
        .text_size(px(12. * z))
        .text_color(rgb(theme.muted))
}
pub(super) fn placeholder(
    label: impl Into<SharedString>,
    height: f32,
    color: u32,
    zoom: f32,
) -> Div {
    let z = valid_zoom(zoom);
    div()
        .h(px(height * z))
        .rounded_md()
        .bg(rgb(color).alpha(0.12))
        .flex()
        .items_center()
        .justify_center()
        .text_color(rgb(color))
        .child(label.into())
}
