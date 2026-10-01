# Explicit visual flow layout

`gpui-flow-layout` is an optional, headless Rust layout module. It arranges the
direct children of fixed or content-sized `Box` and `Frame` scene objects. It changes actual
shape geometry and local transforms, so existing GPUI drawing, picking, engine
bounds, damage, JSON, and editor undo/redo see the same result.

This is a bounded implementation milestone, not Leafer API compatibility or a
asynchronous auto-layout scheduler. Deserialization alone installs no behavior.
Use explicit passes, or install the optional engine precommit provider described
in [reactive-layout.md](reactive-layout.md) after initial arrangement.

## API and transaction boundary

```rust
use gpui_flow_layout::{FlowLayout, FlowStyle, FlowItem, Insets, AlignItems};

let mut layout = FlowLayout::default();
layout.containers.insert("panel".into(), FlowStyle {
    padding: Insets::all(12.0),
    gap: 8.0,
    align_items: AlignItems::Center,
    ..Default::default()
});
layout.items.insert("flexible".into(), FlowItem {
    grow: 1.0,
    min_width: Some(24.0),
    max_width: Some(240.0),
    ..Default::default()
});
// A single history entry, never one per child:
layout.apply_to_editor(&mut editor)?;
```

- `arranged(&Scene)` returns a private candidate scene and `FlowReport`
- `apply_to_scene` atomically updates an inert document
- `apply_to_engine` uses one normal engine transaction with `ChangeOrigin::Layout`
- `apply_to_editor` uses one ordinary editor transaction (`Edit` origin), retaining
  selection and making the complete pass one undoable operation; an active gesture
  absorbs it into that gesture's history entry
- Equal repeated results do not produce a new engine revision or undo entry
- Live handles, hierarchy, ordering, listeners, object IDs, styles, custom data,
  and resource ownership are retained
- Invalid configuration, unsupported resizing, arithmetic overflow, or validation
  failure leaves the original scene, engine revision, listeners, and history intact
- These are trusted host operations like `SceneEditor::transaction`; they do not
  consult editor capability policies or skip locked children

Layout configuration is external input. Geometry undo does not undo changes made
to the `FlowLayout` value itself. A host that edits configuration needs to manage
its configuration history and when to reflow; do not blindly trigger reflow while
restoring an undo snapshot.

## Measurement and placement

The default direction is `Row`; `Column` swaps the main/cross axes. Padding has
four non-negative local-unit sides; content width/height saturate at zero when
padding exceeds the container. `gap` is main-axis spacing; `line_gap` separates
wrapped lines. Children are considered in structural child order (or reversed traversal for
RowReverse/ColumnReverse),
not sorted by ID or paint `z_index`.

Each participating child is measured by its intrinsic geometric bounds after its
local affine linear transform. Existing translation is ignored for measurement
and replaced with the layout position. Negative geometry origins and rotated,
skewed, or reflected shapes work for position-only arrangement. The container's
own transform is untouched, including when its ancestors are transformed.
`FlowStyle::item_box` defaults to `ItemBox::Box`, preserving stroke-agnostic slots.
Opt-in `ItemBox::Stroke` includes the object's own conservative stroke bounds;
see [stroke item boxes](#stroke-item-boxes). Effects, descendant overflow, and
font glyph ink bounds do not influence slots. Group has no intrinsic box and
must be excluded using `in_flow=false`.

`visible=false` remains a paint visibility setting: a hidden child consumes a
layout slot. `FlowItem.in_flow=false` excludes a child and retains its geometry and
translation. A separately configured nested container is still processed even
when excluded from its parent's flow; exclusion is not subtree disabling.

`justify` supports Start, Center, End, and SpaceBetween. SpaceBetween keeps the
configured gap as a minimum and shares positive leftover space between siblings;
a singleton stays at Start. `align_items` supports Start, Center, End, and Stretch,
with a per-item `align_self` override. A non-wrapped line uses the full content
cross-axis extent. Wrapped lines use their tallest/widest measured item, and the
complete line block is placed with Start/Center/End `align_content`. Overflow is
start-aligned with fixed/Auto spacing. Opt-in Fit spacing can overlap items;
there is no implicit geometry shrink-to-fit behavior.

Wrapping uses each item's constrained main-axis basis plus the complete next gap
before deciding to start a new line. An oversized item occupies its own line and
overflows. Wrapped grow allocation happens independently per line. Nested layout
is parent-first, allowing a resized nested Box/Frame to arrange its children using
its final intrinsic size. Content-sized dimensions use the bounded measurement rules below; cyclic size ownership is rejected.

## Grow, constraints, and resizing

`grow` is a finite non-negative weight. Positive remaining space after bases and
gaps is distributed proportionally; max-constrained items freeze and their unused
share is redistributed until the active items or space are exhausted. Explicit
`basis` is a stable input in main-axis parent-local units. Without a basis, growing
items start at zero (clamped to their minimum), while non-growing items use current
measured geometry. Keeping the basis separate from output geometry makes repeated
grow/wrap passes stable and permits shrinking a previously grown container.

`min_width`, `max_width`, `min_height`, and `max_height` constrain parent-local
selected item-box dimensions. They must be finite and strictly positive; min cannot
exceed max. Minimums can force overflow and are never silently reduced. Stretch is
also subject to cross-axis min/max.

When a requested size differs from current measured bounds, resizing is supported
for Rect, Ellipse, Text, Image, Box, and Frame with a positive axis-aligned transform
(`a,d > 0; b,c == 0`). Intrinsic size is updated through normal shape mutation,
preserving the affine scale and all other shape fields. Rotated/skewed/reflected
resizing and arbitrary Path/Polygon/Star/Line/Custom resizing are explicitly
rejected. Fixed placement of these shapes is supported. Text changes its layout
rectangle only; no font shaping or font-size scaling is implied.

Scene primitives require positive intrinsic sizes. A grow/stretch allocation of
zero therefore returns an error rather than creating an invalid shape. Give the
item a positive minimum when exhausted-space overflow is desired. All calculations
use widened intermediates and checked finite output; arbitrary huge finite gaps or
positions may still be rejected by ordinary scene bounds validation.

## Persistence

`FlowDocument { scene, layout }` provides checked `to_json`/`from_json` helpers.
It serializes explicit layout configuration beside the scene, without reserving
application-owned `Scene.data` or object data. Plain `Scene::to_json` saves the
arranged geometry only. Runtime caches/handles/listeners are never saved.

Container IDs must exist and refer to Box/Frame nodes. Item overrides
must refer to direct children of configured containers. Remove or update stale
configuration after deleting/reparenting nodes; it is rejected rather than silently
ignored. Numeric validation also applies to excluded item inputs. Unknown style
fields are rejected to avoid silently accepting misspelled feature switches.

## Source grounding and deliberate limits

The official Leafer flow plugin was inspected at commit
`3eb445bf34debb638766615282224b77108bf610`:

- [Flow registration and fixed/auto container integration](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/index.ts)
- [Horizontal pass](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowX.ts)
  and [vertical pass](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowY.ts)
- [Bounds selection](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/common/bounds.ts),
  [wrap aggregation](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/common/wrap.ts),
  and [line alignment](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/x/layout.ts)
- [Weighted grow and size-range redistribution](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/x/grow.ts)

The Rust code is an independent implementation; no upstream source was copied.
The borrowed architectural contracts are explicit measurement then arrangement,
ordered children, separate hidden/excluded participation, per-line allocation,
and range-constrained grow. This module deliberately uses typed optional input
and atomic scene changes instead of JavaScript prototype mutation.

Not implemented: global reactive scheduling, grow on the cross axis, complete ratio-lock combinations,
shrink, stroke-box resizing, reactive dependency scheduling, input/computed
property separation within Scene itself, or Leafer import/export compatibility.
Percentages and baseline alignment are also unsupported here; the official Flow
reference labels these future support, so they are not claimed existing Leafer gaps.
Current scene mutators clone/validate candidates; this layout module is not a
linear-time or large-scene performance guarantee.


## Content sizing and edit-driven reflow

`FlowStyle.width_sizing` and `height_sizing` default to `AxisSizing::Fixed`, so old
configurations keep their current behavior. `AxisSizing::content()` measures the
intrinsic geometric boxes of participating children. `Content { min, max }`
provides limits; the minimum defaults to one positive local unit and must be
finite and > 0. The optional maximum must be finite and >= minimum.

- Row natural width sums constrained main-axis bases plus gaps; natural height
  takes the largest child cross extent. Column swaps the axes
- With a fixed main axis, wrapping measures all line cross extents, line gaps
  and padding. A fixed-width row can therefore calculate its own content height
- With a content-sized main axis, wrapping is disabled, as in upstream
  `getParseData` (`needWrap` requires a non-auto main axis). A maximum caps the
  container box and permits overflow; it does not silently shrink children
- Minimum size creates alignment space. Center/end/space-between use the
  resolved container size; wrapped line blocks use align_content as before
- Hidden children retain their layout slot. `in_flow=false` children do not
  participate in this measurement and keep their own transform
- Empty content containers use padding and their positive minimum. Own affine
  transforms do not change intrinsic sizing units; child transformed boxes do
- Stroke/effect extents and text font shaping are not intrinsic measurement here;
  Text uses its explicitly provided geometry box, and Group has no supported
  intrinsic resize box

Nested containers are measured bottom-up, then arranged parent-first so parent
width allocations can update a child's wrapping before the resulting height is
propagated upward. The private candidate is repeated to a fixed point with a
hard limit of 32 full passes. Failure returns `FlowError::NonConvergent` and
commits no partial scene. `FlowReport.passes` reports actual work; container/item
counts count unique configured work, not the number of repeated visits.

One dimension must have one sizing owner. An auto main axis cannot be defined
by a child's positive grow; an auto cross axis cannot depend on child stretch.
A nested content-sized dimension cannot also receive parent grow/basis/stretch
or item min/max allocation on that dimension. Choose non-stretch alignment or
put a content dimension's limits in its own AxisSizing. These cases return an
explicit error rather than retaining a stale size or entering a feedback loop.
This is a bounded contract, not all CSS or Leafer auto-sizing combinations.

Use `layout.transaction_editor(&mut editor, edit)` to combine an input change
and dependent layout into one atomic Undo step. It first normalizes registered
custom-element geometry with the editor's engine, then measures and validates
before committing. Invalid input/geometry/layout changes nothing; earlier edits
and Undo/Redo stay available. Plain `apply_to_editor` still explicitly reflows
an already edited scene. Direct engine or scene writes are not automatically
intercepted, and loading a FlowDocument does not install a background watcher.

Registered element definitions must be pure and deterministic: equal resolved
properties must produce equal geometry. Preparation and final commit may call a
definition multiple times. Do not use call count, clock time, randomness, external
I/O or mutable state as geometry input. The final committed custom geometry is
tested as the same layout fixed point, rather than trusting the preview alone.

A successful layout transaction during editor playback finishes the current
animation frame as its own Undo step, then records the input edit plus layout as
another step. Stale animation ticks/cancels cannot overwrite that edit. Rejected
input or layout is computed privately and leaves playback, history and Redo
intact; it does not end an active session.

Examples:

```bash
cargo run --locked -p flow-studio --bin auto_size
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin auto_size
```

The native example changes a fixed width and adds/removes cards; all dependent
positions and frame height are updated by the transaction entry point, with
Undo/Redo. Native compile/link evidence is separate from actual GUI verification.

Additional official-source grounding at the same pinned leafer-in revision:

- [Flow box/content bounds and second layout](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/index.ts)
- [Automatic-side wrapping rules](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/common/data.ts)

The Rust implementation keeps explicit configuration and atomic snapshots,
instead of copying prototype/decorator or global watcher behavior.

## Optional engine precommit integration

The standalone calls above remain explicit. After initial arrangement, `install_reactive` opts into automatic derivation on ordinary engine/editor geometry and tree edits. This synchronous mode preserves one commit/history step and uses live instance bindings. It does not add global watchers or serialize runtime ownership. See [reactive-layout.md](reactive-layout.md) for exact installation, lifetime, Undo and animation contracts.


## Reverse traversal and wrapped line placement

`FlowDirection::RowReverse` corresponds to Leafer `x-reverse`, and
`ColumnReverse` to `y-reverse`. The layout visits participating children
last-to-first before measuring and packing lines. Placement still advances from
the normal left/top content edge; justify/align determine the block's offset.
This follows Leafer's traversal contract, not CSS's reversed main-axis start edge.
The scene's children, paint order, event ancestry and stable instance handles are
never reordered. Hit testing uses the resulting positions through the normal
engine geometry/cache path.

`wrap_reverse: true` reverses the placement order of completed rows/columns on the
cross axis. It does not reverse the items inside each line. Combine it with a
reverse direction to reverse both independently. All line gaps, different line
heights/widths, content alignment, grow/min/max and item alignment still apply.
This flag only matters when wrapping is enabled; a content-sized main axis still
disables wrapping. Missing wrap_reverse in older JSON defaults to false, and old
row/column direction values keep their existing meaning.

Auto-size measures the actual reverse packing groups before placement. Reversing
heterogeneous children can change line membership and therefore intrinsic cross
size; reversing only the line placement order leaves that measured size unchanged.
Both reverse directions use the same axis-ownership validation as their forward
counterparts, including nested parent allocations and content-sized children.

This composes with explicit layout transactions and the opt-in reactive provider:
ordinary tree/shape edits derive reverse positions within the same commit/Undo.
No new observers or history rules are introduced. The `layout_appearance` native
example now includes reversed wrapped rows and a reversed column; its real scene
uses the same renderer and picking pipeline.

Official pinned sources: [flow dispatch](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/index.ts),
[flowX](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowX.ts),
[row placement](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/x/layout.ts)
and [column placement](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/y/layout.ts).
Upstream shrinkContent/shrinkContentCancel separates padded content bounds from
box bounds; it is not a CSS flex-shrink implementation. This milestone adds no
implicit negative-free-space shrinking, percentages, text intrinsic measurement,
frame scheduler or global input/computed property system.


## Automatic and fit spacing

```rust
use gpui_flow_layout::{FlowStyle, GapMode};
let style = FlowStyle {
    wrap: true,
    gap_mode: GapMode::Auto,
    line_gap_mode: GapMode::Fit,
    ..Default::default()
};
```

`gap_mode` selects main-axis item spacing; `line_gap_mode` selects spacing between
completed wrapped lines on the cross axis. `Fixed` is the default and retains the
existing finite, non-negative numeric `gap` / `line_gap`. `Auto` and `Fit` replace
that numeric value with a computed gap. Numeric values are still validated even
when replaced. These are typed Rust/JSON mode fields, not Leafer API imports.
Old JSON without mode fields still behaves identically; default `Fixed` fields
are omitted when serializing. Existing `gap: 8.0` source expressions keep their
numeric type. Exhaustive `FlowStyle` literals must supply the two new fields or
use `..Default::default()`.

The no-grow spacing policy follows pinned Leafer
[`common/gap.ts`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/common/gap.ts)
and its callers in
[`flowX.ts`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowX.ts)
/
[`flowY.ts`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowY.ts):

- For two or more slots, Auto divides positive leftover extent by `count - 1`.
  It uses zero gap when the slots fill or overflow the available extent
- Fit divides all leftover extent by `count - 1`, including negative values.
  Child geometry remains unchanged. The last slot can start before the first
  when its extent exceeds the available axis; no artificial non-overlap clamp
- No outer-edge gap is inserted. Padding is removed before calculating extent
- Empty and singleton sets have no automatic gap and retain normal alignment
- Wrapping measures items with zero automatic gap. Each completed line distributes
  its own remaining main-axis space; Fit does not force all children onto one line
- Row/RowReverse with positive `grow` uses zero automatic item gap, matching
  `flowX`'s grow-or-gap precedence, even when every growing item reaches its maximum.
  Existing explicit `justify` policy still applies to any remainder; automatic-gap
  distribution does not run afterward
- Column/ColumnReverse with participating positive `grow` supports Auto, whose
  gap remains zero: positive grow consumes the space and exhausted grow leaves
  nothing for Auto. Its combination with Fit is rejected atomically. Pinned
  `flowY` runs `autoGap` after `growY` independently, unlike `flowX`; Fit can then
  compute negative gaps after exhausted grow. That order is not emulated on top
  of Flow's existing constrained-basis allocation. Fixed-gap column grow remains
  unchanged; automatic cross-line gaps can still accompany column grow
- Line gaps distribute the available cross extent after all line sizes are known.
  `wrap_reverse` reverses whole lines after measurement; traversal reverse and
  structural/paint order keep their existing independent behavior
- Hidden participating children count; `in_flow=false` children do not

This bounded implementation requires each automatic spacing axis to be fixed-size
and rejects content sizing on that same axis rather than inventing a feedback rule.
A fixed main axis with automatic item gaps may still have a content-sized cross
axis, provided line gaps stay Fixed. Automatic line gaps have no effect without
actual wrapping. Stroke slots are covered below; per-axis item weights remain
unsupported. The bounded ratio-lock allocation subset is covered separately below.

### Reuse and validation boundary

Taffy 0.13.0 continues to compute item sizes, min/max redistribution, and cross-axis
alignment/stretch. No new solver dependency or replacement allocator was added.
The adapter passes zero numeric gap while computing an automatic line, then applies
a small f64 signed-spacing policy to its main positions. CSS gap semantics are
non-negative; Leafer's negative Fit gaps require this policy layer. The previous
f64 huge/subnormal/weight-ratio fallback is unchanged. Cross-line assembly remains
in the existing Flow adapter. Scene mutation, overflow checks, reactive derivation,
engine handles and one-Undo transactions remain on the existing shared path.

The focused `gap_modes` fixtures cover positive/negative spacing, overflowing and
singleton slots, empty containers, both axes, wrap/reverse, grow/caps, transformed
boxes, hidden/excluded participation, fractional/huge values, invalid inputs,
old JSON defaults, repeat stability, and reactive resize/undo. Run
`cargo test -p gpui-flow-layout --test gap_modes` plus the crate's existing suites.
These fixtures are independent expected-output contracts from the pinned upstream
policy; they are not a claim of full Leafer import/export or browser parity.


## Stroke item boxes

Set `FlowStyle { item_box: ItemBox::Stroke, ..Default::default() }` to measure
visible vector strokes as part of each direct child's slot. JSON uses
`"item_box": "stroke"`; omitted or `"box"` retains the previous behavior, and the
default is omitted on serialization. This is a Rust configuration field, not
support for importing Leafer's `itemBox` property or complete Leafer JSON.
Exhaustive Rust `FlowStyle` literals must add `item_box` or use
`..Default::default()`; serde defaults preserve existing documents.

The mode reuses `DisplayObject::paint_bounds()` and the existing Taffy adapter;
there is no new stroke geometry or flex solver. It transforms that local bound
through the child's affine linear matrix, then compensates its origin during
placement. Existing translations never enlarge the slot. Centered solid round
Rect strokes have the expected width/height plus stroke width. Layered strokes
use the largest visible extent. Cap/join meshes are included conservatively;
dash gaps remain in the bound. This is **not pixel-tight stroke measurement** and
does not claim exact upstream bounds for arbitrary paths, caps or miters.

- Row/Column, both reverse directions, wrap/wrap-reverse, all non-resizing
  alignment, fixed/Auto/Fit gaps, and content-sized parents consume the selected
  slot consistently
- Box/Frame use their own shape/stroke, never descendant overflow or ancestor
  clipping. Their intrinsic content dimensions remain separate from their own
  exterior stroke. Nested content measurement adds each stroke at its own level
- Zero-width and fully transparent stroke paints contribute no extent. Object
  opacity and `visible=false` do not collapse slots. `in_flow=false` skips the
  child entirely; Group still needs this exclusion
- A visibly stroked child may be positioned but cannot be resized through a
  stroke slot in this slice. Grow, basis, constraints or Stretch succeed only
  when their allocation leaves that stroke box unchanged. Otherwise the entire
  pass/edit fails atomically. Unstroked siblings can still grow or stretch into
  the remaining space through the existing positive-axis-aligned resize path
- Visible Text/Image strokes are rejected in Stroke mode because the native
  backends do not draw them. Unstroked `Natural` and `AutoHeight` text retain
  their existing content-size ownership and wrap widths. No stroke extent is
  ever written into a font measurement request or intrinsic text dimensions
- A reactive stroke-box layout opts into base/additional stroke invalidation.
  Width, cap, join, dash and stroke-paint edits derive before one commit and one
  Undo entry. Default box-only providers retain their old invalidation behavior

For example, a 20×10 Rect with a centered 4-unit stroke consumes 24×14. With
10-unit left/top padding its intrinsic origin becomes (12,12), placing the outer
stroke at (10,10); a following item with gap 5 starts at outer x=39. These are
source-derived golden fixture expectations, not a claim of running the full
upstream JavaScript engine.

### Pinned upstream boundary

The independently implemented selector/placement contract comes from
[Leafer bounds selection](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/common/bounds.ts)
and the horizontal/vertical passes linked above. Upstream defaults `itemBox` to
`box` and chooses `localStrokeBounds` for the alternate mode.
[Core local stroke transformation](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/LeafBounds.ts)
transforms inner stroke bounds, and
[Box](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Box.ts)
uses Rect-like stroke bounds separately from child render overflow.

Upstream [grow](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/x/grow.ts)
feeds a stroke-box ratio into
[scaleResize](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/resize/src/resize.ts).
That differs from this adapter's intrinsic-size allocation contract, especially
for text and advanced stroke meshes. This slice rejects the unsupported resize
instead of approximating it by subtracting guessed margins or treating stroke
presentation dimensions as content. Closed Inside/Outside strokes now use the
shared final mesh for position-only stroke slots, including reactive edits and
Undo/Redo ([alignment contract](stroke-alignment.md)). Scale-fixed strokes,
Group slots, stroke-text rendering and full upstream resize parity remain open.

## Ratio-locked main-axis allocation

`FlowItem::lock_ratio = true` preserves the current intrinsic width/height ratio
when main-axis grow, basis or ranges resize a supported shape. This opt-in subset
uses Taffy's existing allocation and a separate uniform size coupling module.
It requires a fixed, non-wrapping parent and a free cross axis; allocated wrapping,
content sizing, cross constraints/Stretch, text/custom resize and visible
stroke-slot resizing are explicit errors. Position-only wrapping is allowed.
See [ratio resize](ratio-layout.md) for exact shape/transform restrictions, the
upstream wrap-pass-order difference, compatibility and executable validation.
