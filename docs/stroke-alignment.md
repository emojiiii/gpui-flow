# Closed-contour stroke alignment

`StrokeAlignment::{Center, Inside, Outside}` is local-space geometry on each
stroke layer. `Stroke::new` and old JSON without `alignment` keep `Center`.
JSON uses `center`, `inside`, and `outside`; the builder is `with_alignment`.
The project deliberately preserves its old default rather than adopting Leafer's
per-element defaults (closed shapes inside, Path/Line center, Text outside).

```rust
use gpui_flow_scene::{Color, FillRule, Stroke, StrokeAlignment, StrokeJoin, Style};
let style = Style {
    fill: None,
    fill_rule: FillRule::EvenOdd,
    stroke: Some(Stroke::new(Color::BLACK, 4.0)
        .with_join(StrokeJoin::Miter)
        .with_alignment(StrokeAlignment::Outside)),
    ..Style::default()
};
```

## Shared geometry

The adapter reuses Kurbo 0.11.3 dash splitting and Lyon 1.0.22 expansion of a
centered stroke of width `2 * width`. It canonicalizes the shape's filled region
under `Style::fill_rule`, independently of the presence, visibility or color of
fill paints. Inside intersects the doubled stroke with that region; Outside
subtracts it. The stroke's overlapping triangles are all oriented alike and
combined with **NonZero**, never EvenOdd. Caps, joins and dash phase keep their
existing semantics; each closed source contour restarts the dash pattern.

Polygon intersection/difference uses pinned **i_overlay 9.0.0** (MIT OR
Apache-2.0, Rust 1.88 minimum), with its checked i_float coordinate adapter.
Lyon triangulates the canonical result. The same final `StrokeMesh` feeds native
vector triangles, CPU compositing, painted hit tests and paint bounds. CPU
coverage unions triangles before applying a layer's alpha once. Distinct paint
layers keep their ordinary back-to-front ordering and independent alignment.
Transforms, including reflection and nonuniform scale, apply after local stroke
construction. Existing ancestor clip chains still apply afterward.

All scene consumers pass the actual fill rule to
`Shape::stroke_mesh_with_rule(&stroke, fill_rule)`. The old
`Shape::stroke_mesh(&stroke)` remains a compatibility wrapper using NonZero;
callers with an EvenOdd style must use the explicit method.

For a miter-joined 20×20 rectangle with width 4:

| Alignment | Mesh bounds | Mesh area |
|---|---|---:|
| Inside | [0,0]–[20,20] | 256 |
| Center | [−2,−2]–[22,22] | 320 |
| Outside | [−4,−4]–[24,24] | 384 |

`DisplayObject::paint_bounds` preserves intrinsic geometry even when unpainted,
keeps conservative legacy centered bounds, and unions the final aligned mesh.
Inside never adds an outward margin; Outside does not use an oversized
miter-limit-only padding. `Style::stroke_extent` remains a conservative helper
(inside 0, outside up to twice the old centered reach), not the final mesh box.

`fill_rule` now marks geometry/paint/hit dirty, triggers opt-in stroke-box layout
providers and invalidates incompatible retained CPU output. Alignment, width,
rule and resulting layout remain within one atomic, undoable editor transaction.

## Bounded admission and numerical contract

Inside/Outside require one or more explicitly closed, non-collinear vector
contours. Rect, Ellipse, closed Polygon, Star, closed Path/Pen, Box, Frame and
closed Custom fallback geometry can use them. Mixed open/closed paths, Line,
Text, Image and Group are explicitly rejected at scene validation, even when
an unsupported aligned paint is transparent or zero-width. Custom fallback
native geometry is supported; Custom CPU producers remain unsupported by the
existing compositing contract. Scale-fixed strokes remain separate and absent.

Existing source/dash/Lyon mesh budgets still apply. Visible aligned geometry
additionally obeys these per-stroke limits:

- At most 4,096 input edges before Boolean processing
- A combined 2,097,152 work allowance: source point/edge precision tests plus
  `n*(n-1)/2 + n` worst-case Boolean edge-pair/work estimates, charged before
  each of the fill-canonicalization and stroke/region operations
- At most 16,384 canonical Boolean output points after either operation
- At most 262,144 final vertices and triangles, independently
- Source and expanded-stroke local coordinates in [−65,536,65,536]; nonzero
  aligned widths in [1/64,65,536]

The Boolean grid has a fixed zero origin and 1/4096-local-unit spacing, so
independent stroke widths cannot shift its filled boundary. Conversion is fallible and
respects the conservative integer range; the adapter never silently coarsens
the grid to admit a larger object. Grid rounding plus f32 output conversion is
below 0.004 local units within the supported range. Flattened curves retain the
existing 0.1-local-unit approximation, independent of viewport/DPR. This is an
absolute-error contract, not exact analytic-curve topology at arbitrary scale.

Distinct source edges that collapse on the grid, non-collinear contours that
become collinear, positive point-to-edge clearances below 1/1024, and nonzero
stroke triangles that collapse on the grid are rejected. Boolean result edges
or complete contours that collapse during final f32 conversion are rejected
as well. Exact coincident
points/edges and genuine self-intersections remain legal and use the fill rule.
Features below the curve-flattening tolerance have no analytic topology
promise. Small-feature rejection is preferable to silently filling tiny holes
or erasing narrow gaps. Degenerate/oversized/extreme-coordinate inputs return
an error with no partial scene mutation or successful partial stroke output.

These limits bound intermediary work as well as final output. They intentionally
reject some otherwise meaningful complex paths; this is a bounded first slice,
not a general-purpose unbounded CAD Boolean engine. No production-scale
performance or cross-platform claim is made.

## Sources and comparison demo

- [Leafer stroke reference](https://www.leaferjs.com/ui/reference/UI/stroke.html)
- [Pinned Leafer doubled-stroke paint stages](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/paint/src/Stroke.ts)
- [i_overlay 9.0.0](https://docs.rs/i_overlay/9.0.0/i_overlay/)
- [i_overlay integer coordinate contract](https://docs.rs/i_overlay/9.0.0/i_overlay/core/integer/index.html)

Run `cargo run --locked -p flow-studio --bin stroke_alignment`. The top native
and bottom CPU rows contain equivalent Center/Inside/Outside compound paths,
transparent gradients and layered strokes without fill paint. Controls change
fill rule, dash, reflection/nonuniform affine transform and rounded clipping;
Undo restores one transaction. See the validation record below for checks
actually run; demo availability by itself is not a native execution claim.

## Local validation — 2026-10-01

All checks below ran locally on the Linux cloud computer with one Cargo job,
no incremental compilation, debug info disabled and a 3 GiB free-space guard.
GitHub Actions were not requested or run for this slice.

- `cargo test --locked --offline`: 764 passed, 8 existing ignored
- `cargo test -p gpui-flow --lib --locked --offline`: 85 passed
- Strict `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`
- Strict docs-example workspace Clippy and both workspace format checks
- `cargo build --locked --offline -p flow-studio --bin stroke_alignment`

The 11 new scene tests cover the rectangle golden, default/serde compatibility,
fill absence, both fill rules, reversed winding, concavity, rounded and SVG-arc
curves, self-intersections, compound dashes, supported closed primitives/Custom,
atomic unsupported/budget rejection, tiny holes/gaps, large-coordinate admission,
and legacy inactive Center behavior. Additional engine/layout/compositor/UI
regressions cover geometry invalidation, stroke-box provider reruns, one-step
Undo/Redo, stale-output rejection, gradient layer order, single alpha application,
reflection/rotation/nonuniform scale, nested clips and exact native mesh reuse.

The native comparison window was inspected on this Linux cloud desktop. Both
rows agreed for fill-rule changes without fill paint, translucent layered
gradients, dash gaps, reflection/nonuniform scale/rotation, rounded clipping,
Undo and pan/zoom. Closing and reopening restored the default scene, and the
second close exited successfully (exit code 0, empty error log).
Geometry-invalidating edits briefly showed “Compositing…”
with the affected CPU rows withheld, then displayed the updated output. This
is verified recovery, not a smooth-frame-rate or pixel-identical backend claim.
Cross-platform execution, broad performance and unrelated old native examples
were not rerun for this slice.
