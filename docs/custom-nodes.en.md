# Custom nodes

Define a node's ports and data, then register a `NodeRenderer` for its kind.
`FlowCanvas` supplies the default frame, ports, selection, and drag behavior.

## Define a node and its renderer

Use a stable kind string. The graph validates port connections, while the
application is responsible for validating its node payload.

```rust
use gpui::{AnyElement, App, Window, div, px, prelude::*};
use gpui_flow::{NodeRenderContext, NodeRenderer};
use gpui_flow::core::{DataType, Node, Point, Port};
use serde_json::json;

fn review_node() -> Node {
    Node::new("review-1", "Review", Point::new(400.0, 120.0))
        .with_kind("review")
        .with_port(Port::input("draft", "Draft", DataType::String))
        .with_port(Port::output("approved", "Approved", DataType::String))
        .with_data(json!({"status": "Pending review"}))
}

struct ReviewRenderer;

impl NodeRenderer for ReviewRenderer {
    fn render(
        &self,
        node: &Node,
        context: NodeRenderContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> AnyElement {
        let status = node.data["status"].as_str().unwrap_or("Unknown").to_owned();
        div()
            .p(px(12.0 * context.zoom))
            .text_size(px(13.0 * context.zoom))
            .child(status)
            .into_any_element()
    }
}
```

Use `context.zoom` for your body's padding, text size, and other pixel
dimensions. `NodeRenderContext` also exposes `selected` and `theme`.

## Register the renderer

In a GPUI callback, create the view and register the renderer using the same
kind as `review_node()`. Registering the same kind again replaces the renderer.

```rust
use gpui::prelude::*;
use gpui_flow::{FlowCanvas, core::Graph};

let mut graph = Graph::new();
graph.add_node(review_node())?;
let canvas = cx.new(|cx| {
    let mut canvas = FlowCanvas::new(graph, cx).expect("valid graph");
    canvas.registry.register("review", ReviewRenderer);
    canvas
});
```

For later payload edits, use `canvas.editor.transaction` in a GPUI entity
update, emit `FlowEvent::DocumentChanged` on success, and call `cx.notify()`.
Register an execution handler separately if the node needs to run work.
[Graph architecture](architecture.en.md#transactions-and-undo) and
[workflows and runners](workflows.en.md) cover both APIs.

## Customize the canvas

Inside a canvas update, set the theme, options, or edge renderer.

```rust
use gpui_flow::{FlowOptions, FlowTheme, StraightEdge};
use std::sync::Arc;

canvas.theme = FlowTheme {
    accent: 0x8b9dff,
    ..FlowTheme::default()
};
canvas.options = FlowOptions::canvas_only();
canvas.edge_renderer = Arc::new(StraightEdge);
```

`FlowOptions::canvas_only()` keeps viewport controls and the minimap. When
implementing a custom edge shape, `EdgeRenderer::paint` receives endpoints in
window coordinates. Implement `hit_test` using the same geometry to make the
edge pickable.

See [components](components.en.md) for building a host toolbar, inspector,
or standalone node.
