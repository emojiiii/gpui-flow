# Ordered vector paint stacks

This milestone implements a bounded, source-informed appearance contract. It is
not Leafer API compatibility or its full compositing/effect pipeline.

## Input and painter order

`Style` retains the existing optional `fill` and `stroke` and adds two vectors:

- `fill_layers: Vec<Paint>`: extra fills, in back-to-front order
- `stroke_layers: Vec<Stroke>`: extra centered strokes, in back-to-front order

The precise order for one vector object is **base fill → fill layers → base
stroke → stroke layers**. Each fill uses the object's `fill_rule`. Each stroke
has its own paint and width, with round caps and joins. Every layer supports the
existing solid, multi-stop linear and radial paints. Coordinates remain local
to the same geometry, then follow the object's affine transform and ancestor
clips. Later source-over paints may cover earlier paints.

The singular fields remain base paints, not aliases or overrides for the new
vectors. In particular, `Style::default()` still has a black base fill. Set
`fill: None` if a stack must start on a transparent surface. The `fills()` and
`strokes()` iterators expose the canonical order used by rendering and picking.

```rust
use gpui_flow_scene::{Color, Stroke, Style};

let appearance = Style {
    fill: Some(Color::hex(0x3159a8).into()),
    fill_layers: vec![Color::rgba(255, 255, 255, 48).into()],
    stroke: Some(Stroke::new(Color::hex(0x172848), 8.0)),
    stroke_layers: vec![Stroke::new(Color::WHITE, 2.0)],
    ..Style::default()
};
```

Rect, Ellipse, Line, Polygon, Star, Path, Pen, Box, Frame and Custom fallback
geometry support stacks. Text, Image and geometry-free Group reject nonempty
extra layers at validated scene/engine boundaries. A directly constructed
`RenderObject` also reports `UnsupportedFeature::LayeredPaint` for these shapes;
`DisplayElement` refuses that unsupported body rather than showing a partial
stack. Custom renderer overrides remain responsible for their own support.

There are at most `MAX_PAINT_LAYERS = 32` combined base and additional paints
per object, including transparent/zero-width inputs. Excess input is rejected,
never truncated. All paint geometry, gradient stops and stroke widths receive
the same validation as the singular fields. Invalid mutations are atomic.

## Bounds, picking, and engine changes

`Style::stroke_extent()` is half the largest positive width among visible base
and extra stroke paints. Transparent solid paints, gradients whose stops are
all transparent, and zero-width strokes do not extend bounds or create stroke
hits. `DisplayObject::paint_bounds()` expands intrinsic geometry by this extent;
these remain conservative geometric bounds, including intrinsic content even
when all paints are transparent. Parent aggregate bounds, transformed bounds,
clips and damage planning consume the same result.

Painted hit testing considers every fill and the union of active centered
strokes. It shares the existing winding rule, flattened stroke geometry and
round-cap distance test. It is geometric paint-presence picking: a gradient with
any visible stop counts as active across its geometry; this is not per-pixel
alpha picking. Text/image legacy hit policies are unchanged. Nonzero hit
tolerance is still a picking convenience rather than a rendered extension.

`PropertyKey::FillLayers` exposes an array descriptor with an empty default and
marks paint/hit dirty. `PropertyKey::StrokeLayers` additionally marks geometry
dirty, recomputing ancestor bounds and covering old plus new extents in damage.
Both typed setters and closure-based transactions classify these properties.
Reordering paints is a real visual change; assigning equal arrays is a no-op.
Rejected mutations preserve the scene, revision and cached layout.

## Native backend execution

`display/vector.rs` prepares ordered tessellated paint operations and submits
each one to GPUI. Solid layers paint their paths directly. Gradient layers use
the existing bounded device-resolution raster/texture path. All paints use the
same affine and rounded-clip preparation; stroke widths are tessellated locally
before nonuniform scaling. Fully transparent and zero-width layers are skipped
without renumbering paint slots.

Gradient cache identity now contains a `Fill(index)` or `Stroke(index)` slot,
in addition to object/input/transform/clip/opacity/device state. Two gradients
on the same object cannot alias, including two differently sized strokes using
the same gradient. Existing cache allocation, upload and retirement limits and
explicit diagnostics still apply; adding a stack does not enlarge those budgets.

## Compatibility and boundaries

Old JSON documents omit the new fields and deserialize to empty vectors. Scene
roundtrips preserve new layer order, paint data and widths. The new serializer
writes the arrays even when empty so engine diffs can observe empty-to-populated
changes. This is backward-reading compatibility, not a promise that an older
binary can display new stacks. Rust exhaustive struct literals require the new
fields or `..Style::default()`.

Element and inherited group opacity are still multiplied into **each paint**.
There is no isolated temporary surface: overlapping paints/children at opacity
0.5 can accumulate alpha instead of compositing the whole element/group once.
Box/Frame child drawing and clipping do not imply group isolation or a new
hierarchical compositor. Advanced blend modes, masks, shadows, filters, stroke
inside/outside placement and state-style cascades remain absent. Basic property
animation is now available separately; see [animation](animation.md).
CPU damage preparation is not partial GPU rendering.

## Verification and source alignment

Focused tests exercise legacy serde defaults, stack roundtrips/order, invalid
and over-budget atomic updates, unsupported shapes, transparent stroke bounds,
layer-aware picking, affine/clip behavior, typed setters, closure edits, no-op
updates, parent cache invalidation and old/new damage. Native unit tests exercise
the actual renderer paint-preparation path and raster samples, even-odd holes,
width-dependent strokes, transparent skipping, and non-aliasing texture keys.
Native window evidence is a separate integration check, not inferred from these
pure tests.

The contract is independently implemented from these official pinned sources:

- [UIData normalization and maximum stroke width](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/data/src/UIData.ts): array paint input, complex paint tracking and maximum stroke width
- [Fill array execution](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/paint/src/Fill.ts): ordered fill execution
- [Stroke rendering](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/paint/src/Stroke.ts): centered and other upstream stroke-placement paths; only centered widths are implemented here
- [UIRender appearance stages](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/render/src/UIRender.ts): fills and strokes are real ordered rendering stages, with additional effects/compositing stages beyond this milestone

See [the source architecture map](leafer-source-map.md#8-appearance-is-a-compositing-contract)
for provenance and the remaining compositor gap. No upstream source was copied.
