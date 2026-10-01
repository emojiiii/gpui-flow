# Bounded subtree outer shadows

This extends the [leaf shadow contract](compositing.md#bounded-leaf-outer-shadows)
to Group, fixed-size Box and fixed-size Frame. The original single-shadow slice
is now extended by [bounded multiple layers](multiple-shadows.md), using the same
`Shadow` value and locked image Gaussian. No new renderer, paint type, dependency or implicit container
paint-order choice is introduced.

## Alpha source and order

The source is the actual composed subtree on transparent pixels. It is never a
bounding rectangle. It includes all visible supported descendants, their own
shadows, local opacity, internal blend results, images/current animation frames,
host-produced text ink and the output of sibling alpha-mask runs. Group has no
own fills/strokes. Box/Frame include their own fills and final stroke meshes.

Each node follows this order:

1. Compose its existing paints and descendants, omitting its own inner and outer effects
2. Apply its child clip while composing children; own fill/stroke are not cut by
   that clip. Mask nodes only contribute through the existing masked-run result
3. Extract that composed alpha, filter/offset/tint it, and draw the shadow behind
   the complete phase stream. Own inner effects sample independently from that original
   source; child effects do cast the parent shadow
4. Apply the parent's opacity and blend once to shadow plus source, then its
   inherited antialiased clip once at the enclosing composition boundary

BeforeChildren retains fill → stroke → children. Explicit StrokeAfterChildren
retains fill → children → stroke. Both execute inside one effect/opacity boundary.
A Frame's own shadow can extend beyond its child clip; an ancestor clip can trim
it. Source sampling is not cropped to the viewport or ancestor clip, so outside
ink can still cast a visible shadow. A group's use as a sibling alpha mask includes
its rendered shadow in mask coverage. Unsupported/missing Image/Text/Custom
producers still reject the entire required domain.

## PassThrough safety

An effect requires a temporary source surface. At opacity 1, adding it to a
PassThrough container would change descendant artistic blends that previously
reached the container's external backdrop. Such a container rejects with
`UnsupportedShadowBackdrop(id)`, including inside a Normal ancestor or OwnedCanvas.
Having the backdrop somewhere in the CPU domain does not make it available inside
this new surface. No automatic conversion to Normal occurs.

Safe cases include source-over-only branches, blends already enclosed by an
explicit Normal descendant, and a parent opacity below 1 (which already isolates
under Flow's existing contract). Existing translucent mask-run isolation is
respected; an opaque mask alone is not sufficient. A zero-alpha shadow does not
add isolation/filter work, but its declaration still selects a CPU domain. Normal
or artistic shadow-bearing containers retain their explicit isolation semantics;
artistic roots still require an actual owned CPU backdrop. Native siblings, host
GPUI widgets and unknown window pixels are never captured.

This is intentionally a bounded contract. It can reject scenes another renderer
can execute; users must explicitly choose Normal or move the blend into a suitable
isolated subtree. Removing/changing the shadow never rewrites blend settings.

## Allocation, cancellation and recursive cost

Before allocating the first output RGBA surface of a domain containing a visible
shadow, a no-pixel-allocation admission pass replays the recursive paint schedule
against a private ledger. Prepared geometry and caller-provided input snapshots
already exist. The pass covers output and retained input/output charges, all live
isolation layers, child/ancestor clip masks and temporary intersections, sibling
mask/content surfaces, alpha extraction, every filter's padded source and known
scratch, filter work, destination compositing, every per-layer source replay and the final source rendering.

It deliberately revisits child effects when sampled into a parent shadow and
when painted normally. Deep chains can have exponential work; the ledger rejects
them before any domain pixels or Gaussian calls. Nested source dimensions use the
same checked SourcePlan as rendering. Structural/isolation depth remains bounded,
and a shadow-source replay does not reset isolation depth. Budget failure leaves
retained-byte state unchanged and publishes no partial output. The actual renderer
keeps its budget checks as a backstop. Admission is conservative, especially where
a grid could contain no visible dots; it never silently downscales or tiles.

Existing limits remain: 4096 per dimension, 4,194,304 pixels per surface, 128 MiB
known working set, 32 million cumulative work units, 16 levels, source offset/blur
and positive-similarity-transform restrictions from the leaf contract. Work units
bound known raster visits/convolution taps, not elapsed time. Cancellation is
checked during admission, between recursive operations, per extraction/tint row,
and before/after the non-interruptible bounded Gaussian. Backend-private geometry
allocation and OS allocation failure remain outside the guarantee.

## Bounds, invalidation and history

Prepared visual bounds union descendant effects, apply child clipping, then expand
for this node's own shadow. Engine effect bounds use that same conservative order.
Paint-only descendant changes propagate visual damage to shadow-bearing ancestors
even when a fixed Frame's extent stays identical; this does not trigger geometric
layout recomputation. Layout, world geometry, picking and selection are unchanged.

Existing complete recursive DomainSource identity and placement checks include
all shadow fields, child transforms and phase choices. An incompatible accepted
image is hidden/retired, pending work is cancelled and late completion cannot be
installed. Image/Text generation and viewport/DPR identity remain in the existing
keys. Ordinary transactions and Shadow-property writes use the same Undo/Redo,
serialization and live-object identities as before.

## Leafer reference boundary

The official [shadow](https://www.leaferjs.com/ui/en/reference/UI/shadow.html) API
includes outer shadows and additional spread, box, multiple and scale-fixed
options. Flow now supports its [bounded multi-layer subset](multiple-shadows.md). The official
[blendMode](https://www.leaferjs.com/ui/en/reference/UI/blendMode.html) contract
allows PassThrough descendants to blend with underlying content and describes
Normal as isolation; that is why new unsafe isolation is rejected here.
[Group](https://www.leaferjs.com/ui/en/reference/display/Group.html) has no own
fill/stroke. [Box](https://www.leaferjs.com/ui/en/reference/display/Box.html) adds
rectangle styling and visible overflow; [Frame](https://www.leaferjs.com/ui/reference/display/Frame.html)
is clipped by default. Flow's existing style and order defaults are preserved.

Pinned sources in the [source map](leafer-source-map.md) distinguish
[BranchRender isolation](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/BranchRender.ts),
[UI effect/paint order](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/render/src/UIRender.ts),
and [shape sampling](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/paint/src/Shape.ts).
Those upstream branches are not copied or claimed as complete parity. Flow's
composed-subtree alpha, explicit paint phases, conservative rejection and image
Gaussian are its tested contract, not a browser/Leafer byte-equivalence promise.

Custom, spread/box-only/per-shadow-blend/scale-fixed shadows and
general filters remain unsupported. Run the native lab with
`cargo run --locked --offline -p flow-studio --bin group_shadows`.

## Validation status

The final local Linux aggregate passed 825 headless tests/doctests (8 pre-existing
ignored) and 91 UI library tests, workspace/all-targets and documentation strict
Clippy, workspace/documentation format checks, documentation test targets, and
the `group_shadows` native build. Counts describe each final aggregate once, not
sums of repeated attempts. The `group-shadow-final` run receipt records exit 0,
locked/offline dependencies, one job, no debug/incremental output and a 3 GiB
free-disk guard; available disk after the run was 7,620,268,032 bytes.

New regressions cover sparse alpha rather than bounds, nested child shadows,
Image/Text ink holes, both container phase orders, rounded child clipping and
outside strokes, mask result sampling, parent opacity once, offscreen sources,
unsafe/safe nested PassThrough cases, transparent shadows, owned/external
backdrops, exact recursive resource accounting and exponential-chain rejection.
The eight-case admission/runtime matrix includes shadow-bearing masks, opaque
and translucent mask runs, child effects and both container paint orders. Exact
runtime peak/work caps succeed; reducing either cap by one rejects before any
domain output/scratch allocation or paint work. Model/editor tests cover clipped
recursive effect bounds, unchanged-extent descendant damage through intermediate
groups, unchanged geometry/selection, stable identity, invalid-edit atomicity and
Undo/Redo. UI controller tests hide/retire accepted root/nested container frames
on effect changes and reject successfully rendered obsolete completions.

Target-window-only cloud Linux smoke checks visibly covered:

- Group alpha gaps and child shadows included in the parent's pink effect
- Box overflow versus Frame child clipping, including the changed shadow source
- Sharp/blurred effect toggles and both Box stroke positions
- Unsafe child Multiply rejection and recovery after explicitly selecting Normal
- Whole-group opacity, parent-shadow removal preserving child shadows, Undo and
  repeated toggles without a persistent loading/error state
- Geometry-only selection and no selection from shadow-only pixels
- Pan/zoom retaining alignment of labels, source pixels, clips and effects
- Normal window close, confirmed disappearance and process exit code 0 with an
  empty native log

These are visible smoke observations, not screenshot goldens or performance
measurements. Reopening, close-during-worker races, exhaustive old-example
interaction, live DPR/multi-monitor changes, live GPU failure, macOS/Windows,
CI and the ignored tests were not exercised. The first aggregate caught the
unchanged-extent damage bug now fixed; a later matrix fixture correctly hit the
PassThrough guard and was corrected to request Normal explicitly. No guard was
relaxed to make that test pass.


当前新增[有界叶节点多层内阴影](inner-shadows.md)。支持范围、真实paint alpha与Leafer几何shape的差别、fill/inner/stroke顺序、完整有界文字源和拒绝规则见该契约；此前阶段的内阴影缺项记录保留为历史验证范围。

Current [bounded container inner shadows](container-inner-shadows.md) extend the
leaf slice to Group/Box/Frame, preserving explicit paint phases, actual subtree
alpha, independent own effects and existing budgets. See that contract for the
current validation scope; older slice receipts above remain historical.
