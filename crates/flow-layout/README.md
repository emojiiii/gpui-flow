# gpui-flow-layout

Pure Rust, opt-in row/column flow layout for fixed and content-sized scene Box/Frame containers.
Padding, item/line gaps, wrapping (including reverse line placement),
row/column reverse traversal, alignment, weighted grow, and min/max sizes
produce normal scene geometry and transform changes. No GPUI dependency or clock.

```rust
use gpui_flow_layout::{FlowLayout, FlowStyle, Insets};
use gpui_flow_scene::{display, DisplayObject, Scene};
let mut scene = Scene::new();
scene.insert(DisplayObject::new("row", display::Box::new(300.0, 100.0)), None, None)?;
scene.insert(DisplayObject::new("a", display::Rect::new(40.0, 30.0)), Some("row".into()), None)?;
let mut layout = FlowLayout::default();
layout.containers.insert("row".into(), FlowStyle {
    padding: Insets::all(12.0), gap: 8.0, ..Default::default()
});
layout.apply_to_scene(&mut scene)?;
assert_eq!(scene.get(&"a".into()).unwrap().object.transform.tx, 12.0);
# Ok::<(), Box<dyn std::error::Error>>(())
```

Use `apply_to_editor` for one undoable operation, or `apply_to_engine` for one
`ChangeOrigin::Layout` batch. Both preserve live element handles/listeners.
`arranged` computes a private scene copy for inspection or a host transaction.
`FlowDocument` serializes the scene and layout configuration together.

This is an explicit layout pass, not reactive scheduling or Leafer API parity.
Position-only layout accepts transformed intrinsic geometry. Resizing supports
Rect/Ellipse/Text/Image/Box/Frame with positive axis-aligned transforms; other
resize requests fail atomically. Hidden children retain their slot; set
`FlowItem.in_flow=false` to exclude a child. Content-sized parents measure with bounded bottom-up/top-down passes. No implicit shrink,
percentages, baseline/font measurement, stroke-box layout, or layout of Groups.
See `docs/flow-layout.md` for precise semantics and upstream source references.


`FlowStyle::width_sizing` / `height_sizing` accept `AxisSizing::Content` with
finite positive min/max limits. A fixed main axis may wrap while its cross axis
sizes to all lines. An auto main axis does not wrap, matching upstream Flow's
measurement rule. Contradictory grow/stretch ownership is rejected, not guessed.
`transaction_editor` combines a host input edit and dependent measurement in one
undoable transaction, normalizing registered custom geometry before measuring.
It is an explicit host entry point, not a global reactive watcher.

Optional `install_reactive` binds the already arranged configuration to live engine instances. Relevant ordinary edits derive layout before commit; Undo/Redo restores exact snapshots and runtime configuration stays host-owned. See [reactive layout](../../docs/reactive-layout.md) for lifecycle and animation ownership.

Reverse directions visit children last-to-first while retaining normal coordinate
axes and structural/paint order. `wrap_reverse` independently reverses completed
line placement. Old JSON defaults to forward order. See docs/flow-layout.md.
