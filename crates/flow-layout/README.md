# gpui-flow-layout

Pure Rust, opt-in row/column flow layout for fixed and content-sized scene Box/Frame containers.
Padding, fixed/automatic/fit item and line gaps, wrapping (including reverse line placement),
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
percentages, baseline alignment, stroke-box resizing, or layout of Groups.
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

Optional `texts` sizing uses `TextMeasurer` through `with_text_measurer`.
`Natural` owns both axes (wrap=false); `AutoHeight` owns height and accepts fixed
or parent-allocated width. The native `GpuiTextMeasurer` uses real platform shaping;
core tests use explicit contract fixtures. No backend is silently substituted.
Measured edits compose with reactive layout and one Undo; explicit
`refresh_text_metrics` invalidates cached metrics and actually reflows.
See [text measurement](../../docs/text-measurement.md).

`GapMode::Auto` distributes positive space between items (or wrapped lines);
`GapMode::Fit` also permits negative spacing/overlap without resizing geometry.
Set `gap_mode` and `line_gap_mode` independently; both default to `Fixed`, retaining
existing numeric `gap`/`line_gap` behavior and JSON. Automatic spacing requires a
fixed-size axis. Row grow takes precedence; column grow supports Auto but its
combination with Fit is explicitly unsupported. Singleton slots keep their normal
alignment. See [spacing rules](../../docs/flow-layout.md#automatic-and-fit-spacing).


`FlowStyle::item_box = ItemBox::Stroke` opts into conservative, own-object stroke
slots using scene bounds. The default `ItemBox::Box` and old JSON are unchanged.
Position-only vector strokes compose with affine transforms, wrapping/reverse,
gaps, nested content sizing and atomic reactive stroke edits. Stroked resizing
and visible native Text/Image strokes fail explicitly; unstroked grow and text
measurement keep their existing content-size contract. Descendant overflow and
ancestor clipping never alter a slot. See [stroke item boxes](../../docs/flow-layout.md#stroke-item-boxes)
for exact limits and pinned upstream sources.

`FlowItem::lock_ratio` opts into main-axis uniform size allocation for
Rect/Ellipse/Image and fixed Box/Frame in a fixed, non-wrapping parent. Existing
Taffy allocation is reused; the cross dimension follows the current intrinsic
ratio. Cross constraints/Stretch, allocated wrapping/content sizing, text/custom
resize and visible stroke-slot resizing fail atomically. Position-only wrapping
remains supported. Missing JSON defaults to false; exhaustive `FlowItem` literals
need the bool or `..Default::default()`. See [ratio resize](../../docs/ratio-layout.md)
for the precise upstream and CSS differences, reactive behavior and runnable example.
