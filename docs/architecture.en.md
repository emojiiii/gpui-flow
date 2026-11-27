# Graph architecture

`gpui-flow-core` owns the graph, editing operations, and document format.
`gpui-flow` renders the graph as a native GPUI `FlowCanvas`. Both the core
model and the execution library can run without a desktop session. The
[library guide](library.md#build-a-typed-graph) shows a complete graph example.

| Layer | Responsibility |
| --- | --- |
| `gpui-flow-core` | Graph, coordinates, transactions, undo, and persistence |
| `gpui-flow-runtime` | Background tasks and typed data flow between runners |
| `gpui-flow-interaction` | Input handling and editor commands |
| `gpui-flow` | Canvas, renderer registry, focus, and view controls |

## Graph model and connections

| Type | What it represents |
| --- | --- |
| `Node` | Kind, title, position and size in world coordinates, ports, and application JSON |
| `Port` | Direction, data type, and connection capacity |
| `PortRef` | A node ID paired with a port ID |
| `Edge` | Connection from an output to an input, with an optional label and application data |
| `Group` | Named rectangle and a set of member nodes |

Port IDs are unique within a node. Connections need existing endpoints and
matching types. `Any` matches either endpoint. A `Named` type compares its name
and leaves payload validation to the host. An input accepts one connection by
default. Outputs have unlimited capacity by default. Set `with_capacity(None)`
for an unlimited input or `with_capacity(Some(0))` to disable connections.

`Graph::connect` rejects duplicate connections. Graphs allow cycles by default
and disallow self-connections. Set `graph.options.allow_cycles = false` when
building an executable DAG. If an importer edits public fields directly, run
`Graph::validate()` before using the result. `validation_issues()` returns paths
for individual failures.

## Transactions and undo

Use a transaction for each discrete edit. The editor validates the changed
graph before saving it to history.

```rust
use gpui_flow_core::{Editor, GraphError, NodeId};

fn rename_node(editor: &mut Editor, id: &str, title: &str) -> Result<(), GraphError> {
    editor.transaction("Rename node", |graph| {
        let id = NodeId::from(id);
        let node = graph.nodes.get_mut(&id)
            .ok_or_else(|| GraphError::MissingNode(id.clone()))?;
        node.title = title.into();
        Ok(())
    })
}
```

For a drag or resize, use `begin_transaction` and finish with
`commit_transaction` or `cancel_transaction`. Undo restores graph and selection.
The default history limit is 100 snapshots. Paste assigns new node IDs,
remaps references within the copied fragment, and adds one undo step.

Groups contain one level of nodes. Moving a member leaves its group's
rectangle as stored. Call `refresh_group_bounds` when the group should fit
its members.

## World and screen coordinates

```text
screen = world * zoom + pan
world  = (screen - pan) / zoom
```

Here `screen` is local to the canvas and `pan` is in screen pixels.
`FlowCanvas` subtracts the canvas origin from window pointer coordinates before
applying the viewport transform. `zoom_at` keeps the world point beneath the
pointer in place.

## Graph and document persistence

| API | Data saved |
| --- | --- |
| `Graph::to_json` / `Graph::from_json` | Nodes, edges, groups, and graph options |
| `Editor::to_json` / `Editor::from_json` | `FlowDocument` with the graph and viewport |
| `save_document` / `load_document` | Validated `FlowDocument` on disk, up to 32 MiB |

Use these APIs to validate the schema and geometry on import. Documents keep
resource paths as strings. The host manages the resource files, autosave,
and any schema migrations. Selection, undo history, and runner state live in
memory.

See [custom nodes](custom-nodes.en.md) for rendering a graph and
[workflows and runners](workflows.en.md) for executing it. Use a
[display scene](display-scenes.en.md) for editable graphics in a nested tree.
