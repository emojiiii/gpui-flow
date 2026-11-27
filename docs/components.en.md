# GPUI components

Use standalone components from `gpui-flow` to compose GPUI interfaces.
Your host owns the graph, selection state, and actions passed to the controls.

## Render a standalone node

```rust
use gpui::IntoElement;
use gpui_flow::{FlowTheme, NodeFrame, PortHandle, TextNode};
use gpui_flow::core::{DataType, Point, PortDirection, Size};

fn prompt_card(selected: bool) -> impl IntoElement {
    let theme = FlowTheme::default();
    NodeFrame::new("prompt-card", "Prompt")
        .size(Size::new(280., 180.))
        .selected(selected)
        .theme(theme)
        .body(TextNode::new("A quiet landscape").theme(theme))
        .child(
            PortHandle::new("text-out", PortDirection::Output, DataType::String)
                .label("Text")
                .anchor(Point::new(280., 80.))
                .theme(theme),
        )
}
```

`NodeFrame::size` uses logical dimensions and the frame scales with
`zoom`. If specified, `position` is the frame origin in the parent's screen
coordinates. `PortHandle::anchor` is the port center in the same coordinate
space as its overlay parent. Scale pixel sizes in a custom body with the
same zoom value.

`body()` holds the content. Use `child()` for overlays such as ports or
badges. Attach the host's pointer handling with `on_header_mouse_down`,
`on_resize_mouse_down`, or `PortHandle::on_connect`.

## Built-in node bodies

| Component | Common options |
| --- | --- |
| `TextNode`, `NoteNode` | Text and theme |
| `ImageNode` | Local path, caption, and preview height |
| `VideoNode`, `AudioNode` | Host-provided `MediaView` player or media details |
| `GenerationNode`, `AgentNode` | Provider label, model, progress, and run callback |
| `OutputNode` | Host-provided result content |

`FlowCanvas` uses these as default bodies when a node kind has no registered
custom renderer. A host can also construct the bodies with `from_node`.
See [custom nodes](custom-nodes.en.md#register-the-renderer).

## Compose editor controls

| Component | Host integration |
| --- | --- |
| `ControlButton` | Handle `on_click` and set the disabled state |
| `Toolbar` | Insert leading content and action children |
| `Palette` | Supply `PaletteEntry` values and handle the selected node kind |
| `Inspector` | Add read-only fields or your own input elements |
| `ViewportControls` | Handle Zoom Out, Reset, Zoom In, and Fit |
| `MiniMap` | Supply world bounds and handle the clicked world point |

Give sibling controls distinct IDs. In a GPUI view, attach action handlers
with `cx.listener`. The host applies graph changes through
[editor transactions](architecture.en.md#transactions-and-undo).
