# Bounded scene erasers

This implements a bounded APP-03 increment through the existing CPU compositor. It does not complete the fixed45 APP-03 contract. No external GPUI backdrop capture, replacement renderer, SVG erase encoding, or new dependency is introduced.

## Model and producer contract

`DisplayObject.eraser` is `EraserMode::{None, Pixel, Path}` with `None` as the legacy default. `with_eraser` sets it. Canonical Flow JSON uses `"none"`, `"pixel"`, and `"path"`; omitted fields retain `None`, while unknown modes reject deserialization atomically. These are Flow enum values, not a Leafer JSON roundtrip. Leafer's `true` and `"pixel"` mean the implemented Pixel operation; strict Leafer import/export still rejects eraser fields/state instead of silently dropping them. Rust exhaustive `DisplayObject` literals must add `eraser` or use constructors. Missing-field compatibility lets the new reader load old documents; it does not guarantee that an old renderer preserves or presents the new eraser field. Keeping canonical Scene JSON v1 does not imply visual compatibility with old programs.

Engine `PropertyKey::Eraser` has default `"none"` and invalidates PAINT | GEOMETRY | HIT. Transactions, undo/redo and reparenting use the canonical model. Switching modes, including None ↔ Pixel ↔ Path, is an incompatible composite placement change; a previously accepted image cannot be reused across it.

Pixel uses the complete rendered alpha of currently supported producers: geometric vectors, supported image/Canvas snapshots, supported same-backend text ink, and Group/Box/Frame contents and supported effects. Its source is first drawn once on a transparent surface. Destination premultiplied RGB and alpha are multiplied by `1 - source_alpha * object_opacity * inherited_clip_coverage`. Source RGB never colors the destination. Artistic blend settings are overridden for Pixel. Fill/stroke overlaps and child overlaps receive object opacity once after combination.

Path clears antialiased filled geometry intersected with the inherited clip. Vector
leaves retain their fill rule, implicit closure and winding holes; a straight Line
has no filled area. Box/Frame use their own rounded contour without descendants.
Image/Canvas use their rounded rectangle/path geometry without loading pixels or
creating a producer request. Text/Custom reject with `UnsupportedPathEraser`; no
glyph box or pixel alpha is substituted.

Group Path erasure intersects its visible positive-opacity child contours, then
clears within its retained geometric bounds. Nested Groups recurse; Box/Frame
children contribute only their own contour. Hidden child geometry can still set
those retained bounds, matching the existing layout contract. An empty Group has
no clear area; an empty leaf contour in an intersection clears nothing. Active
mask prefixes retain stable z-order when computing the Group clear bounds.

Path ignores fill visibility/alpha, strokes and effects. Positive object opacity
gates the operation without attenuating it; zero opacity and hidden state disable
clearing. Custom Pixel producers retain their existing unsupported-producer error.


## Ownership, isolation and order

Any direct eraser child forces its parent to isolate, including hidden and zero-opacity markers. Pixel erasers themselves also isolate. This follows the pinned source's direct-child marker policy and can change artistic blend results even when the marker clears no pixels. An eraser removes earlier content of that direct parent; later siblings remain visible. Stable equal-z order is unchanged. Existing explicit Box/Frame paint phases remain in force; this does not add Leafer's automatic `__drawAfterFill` policy.

A bare exposed eraser in default Domains mode rejects with `UnsupportedExternalBackdrop`. A nested supported eraser cannot erase preceding native roots or siblings outside its direct parent's owned surface. `capture_subtree` rejects an eraser root for the same reason: Canvas draw publishes a completed image with source-over and cannot use that image to erase existing Canvas pixels. Capturing an ordinary parent containing erasers is supported.

OwnedCanvas's synthetic document root acts as the direct parent of scene roots. A root eraser therefore forces a separate transparent document surface. Background and grid are painted separately, then the finished document is composited over them once. Erasure reveals the owned background/grid and keeps an opaque canvas opaque. Root artistic blends also see this isolated document surface when any root eraser marker exists. The public OwnedCanvas API continues rejecting transparent backgrounds; whole-scene raster export uses its separately explicit export background, including alpha.

## Alpha-mask scheduling

The fixed source's mask flush policy depends on the mask object's opacity, not its paint's alpha. Render and admission use the same effective-operation predicate.

| Direct child after an alpha mask | Mask object opacity = 1 | Mask object opacity < 1 |
| --- | --- | --- |
| Pixel eraser, any artistic style | Previous content flushes to parent; eraser operates on empty run content and its post-flush has zero alpha, so earlier content is unchanged | Erases accumulated run content, then the mask is applied |
| Path, PassThrough style | Clears accumulated run content | Clears accumulated run content |
| Path, explicit blend style | Previous content flushes; path clears empty run content, so earlier content is unchanged | Clears accumulated run content |

The opacity-one Pixel no-op is deliberate source-derived behavior, not an intuitive new mask ordering. An opacity-one mask with translucent fill still follows the opacity-one column. A nested ordinary group containing erasers resolves its own erasure before its completed result is masked. An object marked as both an eraser and an alpha mask renders on a blank mask surface, yielding zero coverage. The next mask starts a new run; hidden masks still start zero-coverage runs. These schedules have local CPU fixtures; they have not been executed in the upstream browser runtime for cross-renderer pixel equivalence.

All five supported direct-Group mask modes share the explicit scheduling and picking contract in [mask modes](mask-modes.md); the table above describes the alpha-mask cases specifically.

## Bounds, picking and resource safety

Scene and Engine ordinary parent bounds omit an eraser child and its complete subtree. The eraser's own bounds remain queryable, and Engine effect bounds follow the same exclusion. Scene-wide visible bounds omit eraser roots/subtrees. Active-mask Groups instead use the first-marker logical prefix described in the mask contract, which can include an eraser before that marker; effect allocation remains conservative. Bounds never attempt to shrink to the remaining visible pixels after erasure.

Geometric picking remains independent of composited pixels. An eraser can be the top geometric hit, an erased object remains a candidate, and an eraser outside its parent's aggregate bounds is still reachable. HitPolicy, hitChildren, visibility, locks and inherited clips retain their existing behavior. This adds neither image pixel picking nor final composited-pixel picking.

Prepared erasers keep operation bounds for culling but do not contribute to a parent's positive raster-allocation extent. A large offscreen eraser cannot enlarge the output surface. Geometry/transform limits and existing producer input validation still apply; erasure does not grant resource-loading permission.

Eraser-containing domains run full schedule admission before output allocation. Forced parent/root layers, Pixel isolation, Path A8 scratch, mask flushes, clip scratch, cumulative pixel writes, retained bytes and depth use the existing checked limits. Scalar destination-out and contour-clear loops check cancellation per row. Any cancellation, budget, producer or geometry failure returns no partial frame and restores the live-byte ledger. Existing request/generation ownership, real successful image-upload receipts, and retirement remain the publication boundary.

Visible erasers reject mixed SVG export before host resource preparation. Strict Leafer export rejects all eraser state, including hidden markers, before output serialization. Canvas capture rejects a bare eraser before drawing; ordinary parent capture retains its internal erasers.

## Fixed source references and validation scope

The behavior follows UI `1e11641d3ea6b0211c83447268a90b8d58feec25` and core `1d703303ff0759ce4973d54c87eeb8192dddc570`:

- [ILeaf eraser types](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/display/ILeaf.ts#L95), [LeafData computed blend/isolation](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/data/src/LeafData.ts#L129)
- [Leaf direct markers and contour clear](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display/src/Leaf.ts#L268), [LeafRender early Path routing](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/LeafRender.ts#L22)
- [MaskModule flush order](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/mask/src/index.ts#L59), [LeafBoundsHelper aggregate exclusion](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/helper/src/LeafBoundsHelper.ts#L10)
- [Branch clip behavior](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/BranchRender.ts#L74), [Text path preparation](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Text.ts#L184), [Picker traversal](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/selector/src/Picker.ts#L148)

Focused source fixtures live in `flow-composite/src/eraser_tests.rs`, Scene/Engine `tests/erasers.rs`, and UI composite presentation tests. They cover alpha/opacity/order, direct-parent ownership, hidden markers, Path geometry, literal mask schedules, actual producer alpha, root backdrop retention, explicit consumer failures, bounds/hit/history parity, admission rollback and publication invalidation. CPU assertions are separate from desktop JPEG observations. No upstream browser execution or cross-platform pixel equivalence is claimed.

Run `cargo test -p gpui-flow-composite eraser_tests` for exact CPU coverage and
budget/retention rollback, and the Scene/Engine `erasers` integration tests for
bounds, picking and history. Group intersections also share geometric clip
admission with Path masks; related mask tests protect that shared schedule.

The Linux `compositing` example’s **Path erasers** control toggles a Group
intersection and Image/Canvas/Box/Frame contours without producer resources.
Its native smoke check observed all five clear regions, Undo restoration and a
repeated activation before normal close. Exact coverage and limits remain CPU
regressions; these visible results are not cross-platform pixel guarantees.
