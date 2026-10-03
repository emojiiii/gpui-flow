# Bounded multiple outer shadows

This extends the existing [leaf](compositing.md#bounded-leaf-outer-shadows) and
[subtree](group-shadows.md) shadow contracts. `Style::shadow: Option<Shadow>`
retains its meaning. `Style::shadow_layers: Vec<Shadow>` adds ordered layers;
`Style::shadows()` visits the base shadow first, then the array in its stored order.
The combined count must not exceed `MAX_SHADOW_LAYERS` (8), including transparent
entries. No input is silently truncated. Old JSON defaults to an empty array;
serialization writes `shadow_layers: []`, consistently with fill/stroke layers,
so adding/removing the first layer participates in engine change detection.
The legacy `shadow` field still accepts an object or null, never an array.

Every entry uses the existing `Shadow::new(x, y, blur, color)` API and its finite,
bounded local-space offset/blur validation. Invalid/over-limit edits fail
atomically through Scene/engine/editor transactions and `Scene::from_json`.
Unknown fields inside a Shadow still reject. `PropertyKey::ShadowLayers` exposes
an array with default `[]` and PAINT-only invalidation. Adding, removing, editing
or reordering layers supports the same serialization and Undo/Redo as the base.
The ordered array now carries the bounded [spread, box and scale-fixed modes](shadow-modes.md). Per-layer blend and general filter graphs remain separate work. Inner layers use the separate [bounded inner-shadow API](inner-shadows.md); inner box is explicitly inert. The fixed effect set also includes [foreground blur](foreground-blur.md), [owned background blur](background-blur.md) and [grayscale](grayscale.md).

## Exact composition contract

1. Form the complete source without **any of this node's own shadows**
2. For each shadow in base-then-array order, render that same source alpha,
   independently blur/offset/tint it, then source-over it onto earlier layers
3. Draw the complete source above all of its own shadows
4. Apply this node's opacity/blend and the inherited clip at the existing single
   effect boundary

Thus the last layer is visually above earlier layers, behind the source. For
opaque source ink, red alpha 128 followed by green alpha 128 at an otherwise
empty shared offset yields premultiplied RGBA `[64, 128, 0, 192]`; reversal yields
`[128, 64, 0, 192]`. A +20 layer and a +40 layer project a 10-wide source no farther
than +50; the +40 layer never projects the +20 result to +60/+70. Source alpha
holes in images, glyph ink and vectors remain holes in every zero-blur layer.

For Group/Box/Frame, source alpha includes descendant shadows, mask results,
local opacity and supported internal blends, plus the existing explicit container
paint phases. Child clipping occurs while the source is composed; ancestor
clipping occurs after all of this node's effects. Parent layers can project child
layers, but own layers never feed one another. Sibling mask coverage includes
all of the mask node's visible layers. Offscreen source ink can cast each layer
into the viewport. No bounding-box alpha approximation is used.

A declaration containing only transparent layers still requests a CPU domain,
just like the legacy transparent base shadow. It adds no filter work, projection
extent or new isolation. If any layer is visible, all existing positive-similarity
transform restrictions and [PassThrough safety rules](group-shadows.md#passthrough-safety)
apply. In particular, a visible additional layer with an absent/transparent base
cannot bypass unsafe-backdrop rejection. Custom producers remain unsupported.

## Bounds, cache and budget

Visual bounds union independent projections of the unchanged source bounds.
Container source bounds already include descendant effects and child clipping.
Invalid projected extents propagate as rejection rather than being swallowed by
floating-point union operations. Geometric layout, ItemBox, hit testing and
selection do not expand. Paint-only descendant changes still damage a layer-only
ancestor even when a fixed Frame's conservative extent is unchanged.

Layer values and order are in the recursive domain identity and placement check.
An accepted incompatible frame is hidden/retired; old successful completions
cannot replace newer layer changes. Image/Text generations and viewport/DPR
identity continue to apply.

This implementation intentionally re-renders and extracts source alpha for each
visible layer using the existing SourcePlan and locked image Gaussian/A8 path.
It does **not** introduce a shared alpha/filter cache or claim optimal performance.
Each layer reserves its padded RGBA/A8/filter scratch, recursively renders all
child effects, charges filtering and destination compositing, then releases its
scratch before the next layer. The no-pixel-allocation admission pass replays the
same complete schedule before the domain's first pixel allocation. Source content
is rendered again for the final visible object. This can multiply nested work;
deep multi-layer chains are rejected by the existing cumulative budget instead
of partially rendered, downscaled or tiled.

All previous limits remain, including 4096 per dimension, 4,194,304 surface pixels,
128 MiB known working set, 32 million work units and 16 levels. Eight layers is
an input ceiling, not a promise that any eight-layer scene fits those budgets.
A later invalid/over-budget layer rejects the entire output before a valid early
layer allocates or paints. The existing bounded non-interruptible Gaussian and
backend-private/OS-allocation caveats still apply.

## Leafer reference and scope

The official [shadow API](https://www.leaferjs.com/ui/en/reference/UI/shadow.html)
accepts one effect or an array. The inspected
[Shadow.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/effect/src/Shadow.ts)
iterates effects in array order, samples the same cached shape for each, composites
each result and clears the temporary canvas between layers. Its extent calculation
unions independently projected sides. These observations motivate Flow's order
and no-own-feedback contract. Flow keeps its separate compatible `shadow_layers`
API, tested composed-subtree alpha, explicit paint phase policy, Gaussian choice,
resource ceilings and conservative rejection. This is not a Leafer JSON importer,
complete effect implementation or browser-pixel-equivalence promise.

Run the native lab with
`cargo run --locked --offline -p flow-studio --bin multiple_shadows`.
Controls change array presence/order, offset, blur, opacity, child clip and paint
phase; existing unsafe blend/isolation and Undo controls remain available.

## Validation

The final Linux aggregate passed 847 headless tests/doctests (8 pre-existing
ignored), 93 UI library tests, strict workspace/all-targets and documentation
Clippy, both format checks, documentation test targets and the native lab build.
The `multi-shadow-final` receipt records exit 0, locked/offline dependencies,
one job, no debug/incremental output and a 3 GiB disk guard; available space after
the run was 7,416,266,752 bytes. These are final aggregate counts, not repeated
attempts added together. Unit fixtures explicitly cover layer order/alpha, no own feedback,
distinct blur/offset, source holes, child effects, mask coverage/results, container
phase/clip, once-only ancestor AA clipping, offscreen sources, transparent layers,
PassThrough rejection, exact recursive admission limits, deep-chain and later-layer
rejection, atomic document/history updates and stale UI completion handling.


Target-window-only cloud Linux smoke checks visibly covered:

- Swapping base/additional order changed overlap from yellow-dominant to pink-dominant
- Moving one Box layer left left the other layer's source projection in place
- Frame additional-layer blur changed a hard outline to a soft outline
- Removing/restoring Group layers and Undo preserved the base and child effects
- Whole-Group alpha, Frame child clip and both Box paint-phase choices
- Full-opacity PassThrough with an artistic child rejected; explicit Normal and
  returning the child to source-over restored the Group
- Repeated layer toggles recovered without a persistent loading/error state
- Selection stayed around actual source geometry; shadow-only pixels cleared it
- Wheel pan and Ctrl+wheel zoom kept sources, clips, effects and labels aligned
- Normal window close, disappearance from the inventory and process exit 0 with
  an empty native log

Quick edits and zoom briefly exposed the existing Compositing/StaleFrame status
before the new complete frame settled; this is not a low-latency/performance claim.
No source implementation fix was needed after the final aggregate. Earlier
validation corrected a test helper that selected a native Frame instead of its
child composite domain, and Clippy-only constant-chunk iteration in new tests.
The read review also caught and fixed empty-array omission missing PAINT diffs
and invalid projected bounds being swallowed by float union, before validation.

This is a smoke run, not screenshot-golden or cross-platform certification.
Reopen, close-during-worker races, live DPR/multi-monitor changes, live GPU failure,
all old examples, CI, macOS/Windows and the ignored tests were not exercised.


当前新增[有界叶节点多层内阴影](inner-shadows.md)。支持范围、真实paint alpha与Leafer几何shape的差别、fill/inner/stroke顺序、完整有界文字源和拒绝规则见该契约；此前阶段的内阴影缺项记录保留为历史验证范围。

Current [bounded container inner shadows](container-inner-shadows.md) extend the
leaf slice to Group/Box/Frame, preserving explicit paint phases, actual subtree
alpha, independent own effects and existing budgets. See that contract for the
current validation scope; older slice receipts above remain historical.

The subsequent [shadow-mode contract](shadow-modes.md) documents the exact A8 one-tap correction and added conservative positive-sigma work charge. Earlier validation counts and native observations above remain historical; new-mode source and native gates are separately recorded.
