# Vector stroke alignment

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

The adapter uses Kurbo dash splitting and Lyon expansion of a
centered stroke of width `2 * width`. It canonicalizes the shape's filled region
under `Style::fill_rule`, independently of the presence, visibility or color of
fill paints. Inside intersects the doubled stroke with that region; Outside
subtracts it. The stroke's overlapping triangles are all oriented alike and
combined with **NonZero**, never EvenOdd. Caps, joins and dash phase keep their
existing semantics; each source contour restarts the dash pattern. Open source
contours close **only for the fill mask**: the end-to-start edge never acquires
stroke ink or an extra join. A straight or collinear open contour has no own fill
region, so Inside is empty and Outside is the full doubled centered stroke. In
a compound path that line is still clipped against the other contours’ combined
fill region. Both rules apply across all open and closed contours together;
winding reversal does not exchange Inside and Outside.

Polygon intersection/difference uses pinned **i_overlay 9.2.0** (MIT OR
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

Inside/Outside support Rect, Ellipse (including open arcs), Line, open or closed
Polygon/Path/Pen, Star, Box, Frame and Custom fallback contours. Mixed open/closed
compound geometry shares the same mask and fill rule. Text, Image and Group
remain explicitly rejected at scene validation, even when an unsupported aligned
paint is transparent or zero-width. Explicitly closed contours still require non-collinear points; too-short or
collinear closed inputs retain their rejection boundary. Open non-collinear contours are checked by
non-collinearity, not signed area: a self-intersecting bow tie remains meaningful.
Custom fallback native geometry is supported; Custom CPU producers remain
unsupported by the existing compositing contract. [Bounded scale-fixed strokes](scale-fixed-strokes.md) now resolve width before
this same mesh; their transform, precision and consumer gates are separate.

Empty and move-only paths yield empty meshes. A zero-length open segment follows
the shared Lyon cap contract: Butt has no ink; Round/Square Outside has its doubled
cap geometry, while Inside has no ink when the combined fill mask is empty.
A dashed path lying wholly in a gap also has no ink. The adapter records expected
ink after dash splitting and before f32/Lyon conversion, so a real stroke erased
by extrusion or grid collapse still returns a precision error. An empty painted
mesh does not erase the object's intrinsic bounds.

Existing source/dash/Lyon mesh budgets still apply. Visible aligned geometry
additionally obeys these per-stroke limits:

- At most 4,096 input edges before Boolean processing, including implicit
  closing edges and open centerlines whose own fill region is empty
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

Distinct source edges (including implicit closing edges) that collapse on the
grid, non-collinear contours that become collinear, positive point-to-edge clearances below 1/1024, and nonzero
stroke triangles that collapse on the grid are rejected. Boolean result edges
or complete contours that collapse during final f32 conversion are rejected
as well. Exact coincident points/edges and genuine self-intersections remain legal and use the fill rule.
Features below the curve-flattening tolerance have no analytic topology
promise. Small-feature rejection is preferable to silently filling tiny holes
or erasing narrow gaps. Unsupported closed-degenerate/oversized/extreme-coordinate
inputs return an error with no partial scene mutation or successful partial stroke output.

These limits bound intermediary work as well as final output. They intentionally
reject some otherwise meaningful complex paths; this is a bounded first slice,
not a general-purpose unbounded CAD Boolean engine. No production-scale
performance or cross-platform claim is made.

## Sources and comparison demo

- [Leafer stroke reference](https://www.leaferjs.com/ui/reference/UI/stroke.html)
- [Pinned Leafer doubled-stroke paint stages](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/paint/src/Stroke.ts)
- [Pinned Leafer Canvas clip adapter](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/canvas/canvas/src/LeaferCanvasBase.ts)
- [Canvas implicit subpath closure for clipping](https://html.spec.whatwg.org/multipage/canvas.html#dom-context-2d-clip)
- [i_overlay package and integer coordinate limits](https://docs.rs/crate/i_overlay/latest#integer-coordinate-limits)

Run `cargo run --locked -p flow-studio --bin stroke_alignment`. The top native
and bottom CPU rows contain equivalent Center/Inside/Outside geometry,
transparent gradients and layered strokes without fill paint. Scenario controls
cycle open L, quadratic/cubic curve, Line, mixed contours, open arcs and the
closed baseline. Caps, joins, odd dashes with negative phase, fill rule,
reflection/nonuniform affine transform and rounded clipping are reversible
scene edits. Undo/Redo restores one transaction. The diagonal/closing chord must
remain blank in the open scenarios. See the validation record below for checks
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

## Open-contour increment — 2026-10-02

The open-contour extension keeps Canvas command strokes centered-only and adds
no public fields, arrowheads, per-side widths or scale-fixed behavior. Existing
closed tests are retained. Scene regressions cover open L and curves/arcs,
implicit-mask-only closure, all caps/joins, empty/collinear/zero-length inputs,
compound rules/winding, fill-paint independence, odd negative-phase dashes,
source/implicit-edge precision and work budgets, and atomic rejection. Consumer
regressions cover shared bounds/hits, damage, stroke-box layout and Undo/Redo,
CPU union alpha and transformed clipping, shadows, SVG vectors, and native mesh
reuse at DPR 1/1.5/2.

Final-source Linux validation passed with one Cargo job and the existing shared
native target, retaining the 3 GiB disk reserve:

- Default-member tests and doctests: 1,566 passed, 0 failed, 10 existing ignored
- UI library: 252 passed; seven-scenario fixture control tour: 1 passed
- Strict workspace/all-targets Clippy and strict docs-example Clippy
- Workspace and docs-example formatting, whitespace check, and native demo link

All 475 build-input file hashes matched before and after the final test/link
sequence. Exact commands, log hashes, scope and binary provenance are recorded
in the validation summary (historical record in Git history).
The fixture's bounded even/odd dash patterns are `36/24` and `36/24/12` at phase
`-13`; the latter is exercised with all cap types without raising production
budgets. The later [open-contour native acceptance](open-stroke-native.md) records
seven scenarios, history and navigation,23 trace assertions and41 focused pixel
assertions. Its auxiliary native/CPU box comparison is20/21 within one pixel;
the isolated outlier is preserved, with no backend pixel-equivalence claim.
At that 2026-10-02 checkpoint APP-02 remained partial for scale-fixed.
The later [fixed-stroke acceptance](scale-fixed-native.md) closes that bounded
requirement; the historical 2026-10-01 and open-stroke results above retain their
original source, measurements and limits.
