# gpui-flow-core

Renderer-independent graph editing. Its only dependencies are `serde` and
`serde_json`; it does not link GPUI, a window system, or an execution engine.

## Typed graph

```rust
use gpui_flow_core::*;

let mut graph = Graph::new();
graph.add_node(
    Node::new("source", "Source", Point::new(40.0, 60.0))
        .with_kind("number")
        .with_port(Port::output("value", "Value", DataType::Number)),
)?;
graph.add_node(
    Node::new("sink", "Sink", Point::new(360.0, 60.0))
        .with_port(Port::input("value", "Value", DataType::Number)),
)?;
graph.connect(
    "source-to-sink",
    PortRef::new("source", "value"),
    PortRef::new("sink", "value"),
)?;
# Ok::<(), GraphError>(())
```

- IDs are string newtypes and serialize as strings. Nodes, edges, and groups use
  deterministic `BTreeMap` ordering
- Edges run from output to input. Exact data types must match unless either is
  `Any`. `Named(String)` provides application-specific types
- Inputs accept one edge by default; outputs have unlimited fan-out. Set
  `max_connections` to `None` for unlimited connections or `Some(0)` to disable
  a port. Duplicate connections are always rejected
- Self-connections are disabled by default. Cycles are permitted unless
  `graph.options.allow_cycles` is false
- Node and port IDs must be nonempty. Port IDs are unique within their node.
  Geometry must be finite, and node/group sizes must be positive
- A node can belong to at most one group. Group bounds are explicit; call
  `refresh_group_bounds` when the group should wrap its current members
- Application payloads live in `data: serde_json::Value`; `Node::kind` is a
  renderer/executor registry key. No payload execution is performed here

## Editing and history

Construct `Editor::new(graph)?`, then use `transaction` for custom edits or the
built-in move, resize, delete, paste, duplicate, and group operations. Each
transaction works on a private candidate, validates it, and commits atomically.
An error or panic in a custom callback cannot partially mutate the graph.

For a pointer gesture:

1. Call `begin_transaction("Move nodes")` on pointer-down
2. Call `move_selection(delta)` or `transaction(...)` during pointer movement
3. Call `commit_transaction()` on pointer-up, or `cancel_transaction()` on Escape

A gesture is one undo step. Failed intermediate edits are rolled back without
cancelling the gesture. Undo and redo are disabled until it ends. No-op edits do
not clear redo. History stores graph/selection snapshots (100 by default); use
`set_history_limit` to trade memory for undo depth. Camera pan/zoom is not part of
undo history. Snapshot storage is intentionally simple and is not a sparse diff
system for graphs with hundreds of thousands of nodes.

Selection supports nodes, edges, and groups. Moving a group moves its members
once, even if those members are selected too. Deleting a selected group ungroups
its members; explicitly selected nodes are deleted with their incident edges.
The built-in move, resize, and delete operations honor node/group locks. A custom
`transaction` is authoritative and may intentionally change locked items.

Copy/paste preserves node payloads, internal edges, and complete groups. It drops
external connections and gives every pasted graph item a fresh ID, remapping
references. Paste offsets are relative world-space translations. Clipboard
imports and destination connection policies are validated before committing.

## Geometry and persistence

`screen = world * viewport.zoom + viewport.pan`. Screen coordinates are relative
to the canvas; a toolkit should subtract its window-space canvas origin first.
`zoom_at` keeps the world point under the pointer fixed. `fit`, visible bounds,
selection marquee, and minimap geometry are toolkit-independent.

Use `Graph::to_json/from_json` for graph-only persistence, or
`Editor::to_json/from_json` / `FlowDocument` for graph plus camera state. These
helpers validate IDs, endpoints, port compatibility/capacity, cycles, groups,
geometry, and schema version. Bare serde deserialization alone does **not** run
semantic validation. `Graph::validation_issues()` exposes stable paths and
messages for import diagnostics. Unknown schema versions are rejected; there is
no implicit migration. Undo history and selection are not persisted.

## Verification

```sh
cargo test -p gpui-flow-core
cargo fmt -p gpui-flow-core -- --check
```
