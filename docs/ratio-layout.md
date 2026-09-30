# Main-axis ratio-locked resize

`FlowItem { lock_ratio: true, ..Default::default() }` makes an allocated main
size preserve the child's current intrinsic width/height ratio. This is a
bounded implementation of Leafer's uniform `lockRatio` resize contract, not a
numeric CSS aspect-ratio, image-natural-size policy, or full Flow API parity.

```rust
use gpui_flow_layout::{FlowItem, FlowLayout, FlowStyle};
use gpui_flow_scene::{display, DisplayObject, Scene, Size};
let mut scene = Scene::new();
scene.insert(DisplayObject::new("row", display::Frame::new(100., 100.)), None, None)?;
scene.insert(DisplayObject::new("card", display::Rect::new(20., 10.)), Some("row".into()), None)?;
let mut layout = FlowLayout::default();
layout.containers.insert("row".into(), FlowStyle::default());
layout.items.insert("card".into(), FlowItem {
    grow: 1., lock_ratio: true, ..Default::default()
});
layout.apply_to_scene(&mut scene)?;
assert_eq!(scene.get(&"card".into()).unwrap().object.shape.local_bounds().unwrap().size,
           Size::new(100., 50.));
# Ok::<(), Box<dyn std::error::Error>>(())
```

The executable example adds reactive resize, one-step Undo/Redo and JSON checks:

```bash
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin lock_ratio
```

## Supported allocation

- Rect, Ellipse, Image and fixed-size Box/Frame children in a fixed-size,
  non-wrapping parent can use main-axis `grow`, `basis`, minimum and maximum
- Row/RowReverse allocate width; Column/ColumnReverse allocate height. The other
  intrinsic dimension changes by the same scale. Positive nonuniform axis-aligned
  transforms retain their linear matrix and intrinsic ratio; an existing scale
  of (2,4) is not replaced with a uniform transform
- Weighted grow, cap redistribution and overflow retain Flow's existing rules.
  No negative-free-space shrink is added. A basis or maximum can explicitly
  reduce both dimensions. Missing grow basis still starts at zero for allocation
- Cross-axis Start/Center/End align the **final resized box** using Flow's existing
  safe alignment policy. Oversized children overflow from the normal start edge
- Numeric gaps, Auto/Fit gaps, padding and reverse traversal keep their existing
  rules. Column grow with Fit remains rejected. Children and paint order never
  change, and unlocked siblings retain their old resize behavior
- Image uses its provided geometry size, not `natural_size`; the image fit policy,
  corner radii, stroke widths, child transforms and other properties do not scale
- Position-only locked items may participate in wrapping/wrap-reverse, content
  measurement and arbitrary affine placement. This is **not wrapped ratio resize**
- Hidden children keep their slots. Excluded children are untouched and do not
  acquire a ratio or the supported-kind requirements

The ratio is read from the current input geometry, with no hidden persistent
ratio cache. An explicit host shape edit changes the ratio used by the next
layout. It is not a global restriction on editing or an editor resize-handle
policy. Finite positive intrinsic and transformed sizes are required; zero,
non-finite, overflowing or underflowing allocations fail rather than inventing a
ratio. Floating-point sizes remain f32; the coupling uses f64 intermediates and
compares the resulting intrinsic main size before changing the cross size. This
avoids repeated one-ULP transformed-slot round trips becoming ratio drift.

## Explicitly rejected combinations

A participating locked item rejects these configurations atomically:

- Cross-axis min/max constraints or Stretch, including inherited Stretch unless
  `align_self` overrides it with a non-stretch alignment. Two sizing owners must
  not silently distort the ratio or transfer constraints to another axis
- Any main-axis allocation (`grow > 0`, a basis, or a main min/max) in a parent
  with `wrap: true`, even if the current values would happen to leave size alone
- Ratio allocation when either parent dimension is content-sized, or when the
  locked Box/Frame itself has either content-sized dimension
- Text, measured text, Group, Path, Line, Polygon, Star and Custom ratio policies.
  Text font-size resizing and custom/group resize protocols remain separate work
- Rotation, reflection, skew or singular transforms during allocated resizing.
  Position-only affine placement remains available for supported shape kinds
- Any actual resize through a visible stroke item box. The previous explicit
  stroke-box restriction is preserved; box mode may resize a stroked intrinsic
  shape while leaving its stroke width unchanged

These errors occur in the private layout candidate. Scene, revision, listeners,
notifications, history, Redo and active playback survive rejection unchanged.
The existing editor transaction and opt-in reactive provider handle successful
changes as one commit/Undo. Exact Undo/Redo restores snapshots without reflow;
a destroyed/recreated ID does not inherit the old live ratio binding. No engine,
scene schema, renderer, dependency, scheduler or animation ownership changes were
needed for this slice.

## Upstream semantics and crate reuse

Pinned official Leafer plugin revision:
`3eb445bf34debb638766615282224b77108bf610`.

- [`resizeWidth` and main-axis grow](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/x/grow.ts)
  clamp the requested width, calculate a scale, and pass the same scale on both
  axes when `lockRatio` is enabled. [Height resize](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/y/grow.ts)
  uses the symmetric contract
- [`scaleResize`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/resize/src/resize.ts)
  dispatches type-specific behavior; the ordinary
  [size scaler](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/resize/src/scaler.ts)
  scales width/height, not every appearance field. This slice implements that
  ordinary size subset independently, without copying upstream source
- [flowX](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowX.ts)
  and [flowY](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowY.ts)
  capture line cross extents before main-axis grow. Merely measuring the new
  cross extent afterward would change upstream wrap spacing/alignment; preserving
  stale extents can instead change a later pass or produce overlapping rows.
  **This adapter rejects wrapped ratio allocation rather than approximating that
  order.** It does not claim upstream wrap/grow or full alignment-pass equivalence

Taffy 0.13.0 is already the pinned numeric allocator and remains unchanged.
Its [`Style::aspect_ratio`](https://docs.rs/taffy/0.13.0/taffy/style/struct.Style.html)
is a CSS preferred-size relationship. The inspected `flexbox.rs` functions
`generate_anonymous_flex_items`, `determine_hypothetical_cross_size` and
`determine_used_cross_size` resolve definite sizes, transfer min/max and allow
Stretch without necessarily adjusting the main size. Adding that field to the
existing two-definite-dimension leaves does not implement uniform resize: a
20×10 leaf with ratio 2 and grow still becomes 100×10. A regression executes this
case against the pinned crate.

Accordingly, Taffy still allocates main sizes/weights/caps; a small separate
`ratio.rs` module validates ownership and couples the final intrinsic dimensions.
It is not a second flex solver. Existing huge/subnormal/weight-ratio f64 fallback,
line formation, transforms, transactions and reactive derivation are reused.
No Cargo.toml or Cargo.lock changes are needed.

## Compatibility and verification

The new bool defaults to false and is omitted when false in serialized JSON.
Old documents, numeric gap fields, grow and default sizing behavior are unchanged.
JSON uses `lock_ratio`, not Leafer `lockRatio`; this does not add Leafer import or
export. Exhaustive Rust `FlowItem` literals must add `lock_ratio: false` or use
`..Default::default()`.

Focused checks:

```bash
cargo test --locked -p gpui-flow-layout --test lock_ratio --test ratio_reactive
```

The fixtures cover both axes/reversals, caps/basis, mixed locked/unlocked siblings,
positive nonuniform transforms, fixed nested containers, spacing, JSON, supported
shape kinds, exclusions and explicit rejection boundaries. Engine/editor tests
exercise cached bounds, picking, listener identity, one notification, Undo/Redo,
playback success/failure, stale bindings, late rollback and 64 fractional reflows.
No upstream JavaScript execution, renderer change, new native window validation,
performance guarantee or full Leafer parity is implied by these tests.

Local Linux validation on 2026-09-30: 34 new focused regressions, 681 headless
tests/doctests (8 pre-existing ignored), 47 UI unit tests, workspace/all-target
strict Clippy, standalone documentation strict Clippy, formatting, and the new
executable example passed. Builds reused one target with one job, incremental
compilation off and a 3 GiB free-disk guard. No CI, native-window smoke test,
media rerun, or cross-platform validation was performed for this slice.
