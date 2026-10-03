# Scale-fixed stroke width

This bounded APP-02 increment adds a default-disabled policy to each native
`Stroke`. The unchanged fixed APP-02 contract is now bounded verified after
[automatic and native evidence](scale-fixed-native.md); the
[original-scope audit](scale-fixed-acceptance-audit.md) and limitations below
remain explicit. Older open-stroke evidence and its retained outlier are preserved.

```rust
use gpui_flow_scene::{Color, Stroke, StrokeScaleFixed};
let always = Stroke::new(Color::BLACK, 8.).with_scale_fixed(StrokeScaleFixed::Fixed);
let zoom_in = Stroke::new(Color::BLACK, 8.).with_scale_fixed(StrokeScaleFixed::Threshold(1.));
```

Native JSON uses `scale_fixed: true`, a finite positive numeric threshold, or
`"zoom-in"` (normalized to numeric 1). Missing/false/zero means disabled; disabled
serialization omits the field. Negative and nonfinite thresholds are rejected.
Every stroke layer owns its policy; ordinary/fixed/threshold layers may coexist.
The deprecated Leafer `strokeWidthFixed` alias is not a native field.

## Exact width semantics

For authored width `w`, accumulated logical scales `(sx, sy)` and threshold `m`:

- Disabled: `w`
- Fixed: `w / sx`
- Threshold: `w / sx` when `sx > m || sy > m`, otherwise `w / m`

The test is strictly greater on either axis; the divisor in its high branch is
always X. Thus with width 10 and scales `(0.5, 2)`, threshold 1 resolves to width
20. At `(0.5, 1)`, the same policy resolves to width 10. Fixed and threshold 1
are different below unit scale. Reflections use positive magnitudes.

`StrokeScaleData` computes local column lengths using f64, rounds each local
magnitude once to f32, accumulates ancestry products in f64, then applies logical
camera zoom. It never decomposes the final
world matrix. A parent scale `(2,1)` followed by a 90-degree child rotation still
has provenance `(2,1)`, so fixed width 10 resolves to local width 5. Rotated,
reflected and anisotropic local chains are supported, including sheared world
matrices produced by those chains. Each affected local transform must have
invertible, orthogonal columns: `abs(dot) <= 8 * f32::EPSILON * sx * sy`.
Material local shear is rejected atomically on owners or ancestors. Unaffected
ordinary-stroke hierarchies keep their existing affine support.

This is pinned scalar-X behavior, not constant thickness in every orientation.
At diagonal scales `(2,4)`, fixed width 10 gives vertical thickness 10 and
horizontal thickness 20. Dash lengths and signed phase stay in authored local
units. Caps and joins use resolved width; fill masks and alignment rules remain
unchanged. The same existing Kurbo/Lyon/Boolean mesh supplies all alignments,
paint, painted picking and bounds. Fixed solid round strokes also use that mesh.

## Finite threshold precision

Native affine coefficients are f32 data. They do not retain the original TRS
expression or declared scales that produced them. The canonical local magnitude
is f32-rounded column length; ancestry products and camera multiplication are
not rounded back to f32. Thus a real product such as `3 * (1f32 / 3)` can still
exceed 1 by less than one f32 ULP and must take the strict high branch.

Rounding local magnitudes removes the tiny quarter-turn cosine residue, but
cannot reconstruct every rotated declared scale. For example, the actual Rust
`rotation(4) * scale(0.5, 1.25)` has a Y length that rounds to `1.2500001`. Treating
that as an unambiguous crossing of threshold 1.25 would abruptly switch to the
X divisor. This matrix-only boundary is explicitly rejected rather than guessed.

For a local Y column whose rounded length differs from its dominant coefficient,
the resolver carries an admission band of `8 * f32::EPSILON * local_sy` through
ancestry. If the parent magnitude/band is `(y, e)` and the local values are
`(ly, le)`, the accumulated band is `e*ly + y*le + e*le`; logical camera zoom
scales both magnitude and band. Axis columns, including quarter turns whose
minor contribution disappears at f32 magnitude precision, have a zero band.
When numeric policy has `sx < m` and the Y band includes `m`, resolution returns
an explicit ambiguous-threshold error. The band never selects a width branch,
clamps a scale, or changes the strict `>` comparison.

Consequently, some otherwise valid rotated anisotropic numeric-threshold
contexts are unsupported near their Y threshold, including exact-looking unit
and nonunit boundaries. `Fixed` has no such Y-branch restriction. Exact
axis/quarter-turn above/equal/below values retain their one-ULP distinction.
`StrokeScaleData::exact(x, y)` accepts separately known, checked scale provenance
for low-level callers; ordinary scene transforms use the matrix reconstruction
contract above. This is a conservative admission rule, not a claim to recover
arbitrary lost authoring history. A future declared-scale scene field would be
needed for lossless TRS provenance in ambiguous cases.

## Context, admission and retained presentation

Canonical scene/engine bounds and hit queries use accumulated owner scale with
camera 1. `resolved_for_view`, `validate_view`, and the explicit `*_for_view`
queries use a supplied positive logical zoom. Low-level shape stroke geometry
uses identity scale when an authored fixed policy is passed directly; scene and
renderer consumers resolve ancestry first. `Stroke::resolved` is the only width
formula. DPR, pan and canvas origin never enter it.

A resolved scene is an immutable, camera-bound derived snapshot. It may be
queried at that camera, but cannot be edited, serialized as document data,
resolved for a different camera, or used as authored export input. Re-resolve the
original scene instead. An internal nonserializable stroke marker preserves
shared-mesh rendering after width normalization; native JSON cannot import it.
A marker inserted into an authored scene without its view context is rejected.
`SceneView::presented_geometry` returns this read-only presentation geometry.

SceneView retains matching native objects, CPU inputs, hit geometry and selection
bounds/handles even when no state styles are configured. Invalid direct host
views and hard-invalidated/pending CPU presentations withhold the affected
presentation and its hits/handles. A valid replacement recovers normally.
Standalone native users can use `RenderObject::try_from_paint_item`; raw
`RenderObject` literals with unresolved fixed policies receive an explicit
`UnresolvedStrokeScale` diagnostic. The compatibility infallible constructor
requires a pre-admitted view and panics with a clear message otherwise.

Navigation preflights the candidate zoom before assignment. Editor commits
validate their final normalized/derived candidate at the active logical zoom,
before history or observer publication. Hosts operating SceneEngine directly
set its presentation admission context with `set_stroke_view_scale`; no-view
queries retain camera-1 semantics. Ancestor-scale/reparent edits invalidate fixed
descendant local bounds and ancestor subtree bounds. Clone/detach/reattach uses
a transient, nonserializable subtree forest retaining source scale provenance;
final insertion checks the actual target ancestry before committing. Transient
forests must be inserted into a target scene before rendering or export.

Fit evaluates actual resolved bounds at each candidate zoom, using a bounded
search with at most 64 resolutions; numeric threshold jumps are evaluated
directly. Invalid probes are unavailable candidates; only a successfully
resolved, actually fitting candidate can be published. This does not promise a
global optimum across precision-rejected intervals. A failed fixed-stroke fit
retains the prior viewport. Drag containment, edge snapping, marquee, selection
and resize/rotation/line handles use view-resolved bounds or the accepted
presentation. Navigation does not change authored widths, revision or history.

Existing geometry tolerance, grid and work/allocation limits are unchanged.
Nonzero aligned widths `[1/64, 65536]` apply to effective width. Overflow,
underflow, precision loss and budget exhaustion fail explicitly; widths are
never clamped and unsupported contexts never fall back to ordinary strokes.

## CPU, export and bounded exclusions

CPU domain inputs retain ancestry scale separately from world matrices. Stroke
resolution occurs before mesh, source bounds, shadows and surface admission.
A domain captured from a resolved scene rejects reuse with a different logical
camera. Ordinary raster cameras for fixed strokes require positive uniform scale and
translation; their separate `device_scale` changes physical resolution only.
A rotated raw camera matrix does not carry an independent exact logical zoom,
so it is rejected for fixed strokes. Rotated/reflected element ancestry is
still supported within the local precision contract.

Owned PNG/JPEG/WebP scene export resolves `SceneExportRequest.scale` before
folding `pixel_ratio` into the physical transform, retaining explicit logical
scale in its immutable domain. Thus export scale 2 / DPR 2 does not compensate
four times. Existing export crop, background and codec rules are unchanged.

The following combinations are explicitly rejected in this increment:

- Strict Leafer interchange for fixed styles, including derived styles; no
  lossy skew/scale provenance mapping is added to DAT-03
- SVG export of fixed styles
- Imperative Canvas subtree capture containing fixed styles; Canvas command
  strokes remain their existing centered local-width contract
- `ItemBox::Stroke` participation with fixed or derived fixed styles; ordinary
  box layout remains supported and independent of camera
- Text, Image and Group style strokes in the advanced geometry domain; Groups
  remain supported as ancestors, and existing Custom CPU limitations remain
- Genuinely sheared local ancestry, singular/nonfinite scale contexts, and the
  rotated numeric Y-threshold ambiguity band described above

A scene Canvas element's vector border uses its ordinary scene stroke contract,
separately from Canvas imperative commands. Existing clips, masks, erasers and
shadow alpha consume resolved geometry without new effect semantics.

## Source pins and verification scope

Width behavior follows [UIData.__getRealStrokeWidth](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/data/src/UIData.ts#L145)
and [MathHelper.getScaleFixedData](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/math/src/MathHelper.ts#L61).
Scale provenance follows [MatrixHelper.multiplyParent](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/math/src/MatrixHelper.ts#L149),
and DPR separation follows [LeaferCanvasBase.setWorld](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/canvas/canvas/src/LeaferCanvasBase.ts#L129).
The existing [stroke-alignment contract](stroke-alignment.md) retains its geometry
and numerical limits.

Focused regressions cover scalar/threshold goldens, JSON, transform provenance,
local shear rejection, all-alignment shared mesh/bounds/hits, precision limits,
ancestor cache invalidation, history, current-view admission, fit, drag/snap,
subtree transfer, immutable context guards, native mesh reuse, retained snapshot
geometry, DPR and export. Execution results belong to the accompanying task
validation record; this document alone does not claim native acceptance,
backend pixel identity, broad performance or cross-platform verification.
