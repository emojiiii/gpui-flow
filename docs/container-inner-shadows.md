# Bounded container inner shadows

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

Group, Box and Frame now accept the same `Style::inner_shadow_layers` as the
[leaf inner-shadow contract](inner-shadows.md). This is a bounded extension of the
existing CPU compositor, locked image Gaussian, sibling masks and paint phases.
There is no new renderer, dependency, shape property or budget increase.

## Source and dependency graph

Let B be the container's original composed subtree on transparent pixels:

- Group contributes no own fill or stroke
- Box/Frame contribute their actual fill/stroke paint alpha, in their declared
  paint order, together with children
- Children contribute their complete rendered result, including their own inner
  and outer effects, local opacity, internal blends and sibling-mask results
- The container's child clip is applied to children; its own strokes are not cut
- Both kinds of the container's own effects are omitted from B

Every own outer layer independently samples B. Every own inner layer also
independently samples B and computes A × (1 − G(A)(x − offset)), where A is B's
alpha and G is the existing bounded Gaussian. Own layers never feed each other.
In particular, own inner alpha does not feed own outer alpha. A parent's B does
include its child's final inner/outer result. This is the same dependency rule
for leaves and containers, with no recursive self-feedback.

An empty Group or unpainted empty Box/Frame has no source. Sparse child gaps,
image holes and glyph holes stay empty. No bounding-box or opaque geometric fill
is synthesized. Translucent zero-offset sources retain the documented A(1−A)
behavior; this can raise final alpha. For Group child A=128, opaque red inner
coverage is 64, giving premultiplied [64,0,96,160] over blue [0,0,128,128].
Container opacity is applied afterward, once.

## Explicit paint phases are visually different

All own outer layers precede the following phase stream:

1. Group: children → inner layers
2. Box/Frame BeforeChildren: fill → inner layers → stroke → children
3. Box/Frame StrokeAfterChildren: fill → children → inner layers → stroke

The inner source always includes the complete original subtree, even when its
insertion point precedes children. Therefore BeforeChildren can paint tint under
an opaque child which subsequently hides it. For translucent blue child A=128,
zero-offset opaque red inner produces [32,0,128,160] in BeforeChildren versus
[64,0,96,160] in StrokeAfterChildren. These phase modes are deliberately not
visually equivalent. A transparent inner declaration never moves own strokes.

Whole-container opacity/blend is applied once after all phases. An inherited
clip trims the final isolated result once and does not create a new source edge.
In contrast, the container's own child clip changes B and can create an inner
edge from clipped child content. Own outside strokes remain outside the child
clip. A container used as an alpha mask exposes its complete final alpha,
including any inner-induced alpha increase.

## Guards, bounds, invalidation and budgets

The [outer-shadow PassThrough guard](group-shadows.md#passthrough-safety) now also
covers visible inner layers. If an opacity-1 PassThrough container would newly
isolate descendants requiring an external backdrop, it rejects with
`UnsupportedShadowBackdrop`, including inside Normal or OwnedCanvas domains.
Normal descendants, existing opacity isolation and translucent mask-run isolation
retain their supported behavior. An opaque mask alone does not make it safe.
Transparent effect declarations select a CPU domain but add no effect isolation
or filtering. Custom producers still reject, including transparent declarations;
invisible/zero-opacity producers keep the existing dormant-subtree convention.

Inner layers add no external extent. Geometric layout, picking and selection do
not change. Descendant PAINT changes now damage the full affected inner-bearing
ancestor extent even when its fixed bounds are unchanged; this does not force
geometry recomputation. Recursive cache identity includes effect order, phases,
clip, masks and descendants. An incompatible accepted frame is retired, and a
late old success cannot replace the current request.

The admission replay includes every layer, source recapture, child effect,
sibling mask, clip/intersection, isolation surface and compositing pass before
domain pixels are allocated. It uses the same recursive source bounds and depth
as rendering. The phase insertion point changes color order, not the accounting:
child clip/mask temporaries are released before own inner scratch, and all source
replays still execute the complete original phase stream. Exact work/peak caps
and cap-minus-one failures are checked across an eight-case container/mask/phase
matrix. Deep repeated captures still grow rapidly and reject explicitly.

Unchanged limits: 4096 per dimension, 4,194,304 pixels per surface, 128 MiB known
working set, 32 million work units and 16 isolation levels. Positive similarity
transforms and bounded device sigma remain required. Full text sources are
requested for ancestor inner effects, subject to the existing text budgets.
There is no hidden crop, downscale, tile, budget increase or partial fallback.

## Leafer boundary

Pinned [InnerShadow.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/effect/src/InnerShadow.ts)
and [UIRender.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/render/src/UIRender.ts)
remain structural references. Flow consistently uses actual composed paint alpha,
where upstream shape sources and Group/Box content paths differ. Explicit Flow
paint phases, independent own-layer sampling and conservative backdrop rejection
are the tested contract; this does not promise Leafer/browser pixel equivalence.
Spread, box/knockout, per-layer blends, scale-fixed effects, Custom sources and
general filters remain unsupported.

Run `cargo run --locked --offline -p flow-studio --bin container_inner_shadows`
for the native lab. Existing `inner_shadows` now labels its Frame toggle as a
supported effect; its older rejection check is historical.

## Validation

The final Linux aggregate passed 883 headless tests/doctests (8 pre-existing
ignored), 97 UI library tests, the new three-domain shared-budget lab test,
workspace/all-targets and documentation strict Clippy, both format checks,
documentation test targets, three native builds (`container_inner_shadows`,
`inner_shadows`, `inner_shadow_text_probe`), and the existing real-font probe's
six self-inner/ancestor-outer DPR cases plus oversized full-source refusal.
Counts describe one successful final aggregate, not repeated attempts added
together. The `container-inner-final-2` receipt records exit 0, locked/offline
inputs, one job, no debug/incremental output and the unchanged 3 GiB disk guard.
Available space after the aggregate was 6,422,245,376 bytes.

Fourteen new compositor regressions cover sparse subtree alpha, independent own
outer/inner sampling, both phase colors/occlusion, outside strokes, opacity once,
child versus inherited clips, rounded clipping at fractional DPR, masked sources
and container masks, unsafe/safe PassThrough cases, owned backdrops, complete
Image/Text inputs, empty sources, transform/filter refusal, deep replay rejection
and the eight-case exact admission matrix. Added engine/editor/UI regressions
cover inner-only ancestor damage at unchanged extent, identity/selection/Undo,
and accepted-frame retirement plus a successfully rendered obsolete completion.
The three-domain lab fixture fits one shared default budget, retaining outputs.

The first focused run caught two fixture mistakes: alpha masks require a direct
Group parent, and the existing half-opacity source-over rounding yields 128 on
the white backdrop. Those expectations were corrected without relaxing a guard.
The first aggregate passed headless/UI but stopped at a lab test macro-name
collision from a wildcard GPUI import. Explicit imports fixed it; no recursion
limit or renderer budget was increased. The complete final aggregate then passed.

Target-window-only dot-cloud Linux smoke verification visibly covered sparse
Group gaps and child-shadow alpha; Box versus Frame overflow; distinct Box inner/
stroke insertion before or after children; inner removal/restoration; independent
outer removal; whole-group and child opacity; blur and layer reversal; an empty
nonoverlapping sibling-mask run; unsafe Multiply rejection, explicit Normal
recovery and Undo; repeated toggles; geometry-only selection and shadow-only
misses; and wheel pan/Ctrl-wheel zoom. Transient `Compositing...` and `StaleFrame`
badges cleared and complete frames returned. The target window closed normally,
disappeared from inventory and exited 0 with an empty native log.

An initial shell launch without a display variable produced no observable target
window and was interrupted; the verified run used the display-connected cloud
terminal. These are smoke observations, not screenshot goldens, browser parity,
latency/performance certification or a process-wide allocation guarantee. Native
reopening/close-during-worker races, live DPR/multi-monitor changes, GPU failures,
all previous examples, macOS/Windows, CI and the eight ignored tests were not
exercised. The real-font ancestor-inner path has synthetic compositor coverage;
the existing native-font probe still covers self-inner and ancestor-outer cases.
No Actions run or deployment was triggered.
