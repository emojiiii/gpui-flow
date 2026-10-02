# Advanced Flow layout and bounded scheduling (LAY-03)

This is the advanced layout implementation for the frozen LAY-03 acceptance:
fixed-parent percentages, padded `shrinkContent` semantics, measured text row
baselines, dependency-selected reflow, frame coalescing, and atomic rejection of
cycles/ownership conflicts. **The model queue alone is a partial delivery.** A
host must wire it to an actual next-frame callback and verify that integration
before the single-frame criterion can be called complete.

## Percentage dimensions

`FlowItem.width_percent` and `height_percent` are optional finite positive
fractions: `Some(0.5)` means 50 percent. They use the parent's **content box**, in
parent-local slot units, independently of row/column direction. Positive
axis-aligned child scaling is inverted when assigning intrinsic geometry. Values
greater than one intentionally overflow; min/max constraints clamp the resolved
slot. Wrap and reverse use those resolved sizes. Excluded children retain their
geometry and consume no slot.

The corresponding parent `AxisSizing` must be `Fixed`. It may itself receive a
definite allocation from an ancestor, so nested percentage chains settle through
the existing bounded bottom-up/top-down layout. A percentage cannot share its
axis with grow/basis, cross-axis stretch, content sizing, or measured text size
ownership. `lock_ratio` and percentages cannot be combined. A measured
`TextSizing::AutoHeight` may receive percentage width; `Natural` owns its width.
Text's measured height cannot also be percentage-owned. Zero-area allocations
fail unless a positive minimum resolves them to valid geometry.

Unsupported transformed/custom resize policies, indefinite parent percentages,
nonfinite percentages, overflow, competing size owners, and nonconvergence reject
the complete private candidate before scene/history/observers are changed. Same-
axis content/percentage dependency cycles are rejected during validation; other
unstable measure/arrange feedback is bounded by `MAX_FLOW_LAYOUT_PASSES` (32).
No CSS implicit `flex-shrink`, percentage min/max, or percentage padding is added.

```rust
use gpui_flow_layout::{FlowItem, FlowStyle, Insets};
let parent = FlowStyle { padding: Insets::all(10.), ..Default::default() };
let child = FlowItem {
    width_percent: Some(0.5),
    height_percent: Some(0.25),
    max_width: Some(240.),
    ..Default::default()
};
```

## What upstream shrinkContent actually means

At pinned Leafer commit `1d703303ff0759ce4973d54c87eeb8192dddc570`,
[`LeafLayout`](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/layout/src/LeafLayout.ts)
uses `shrinkContent` to separate a content-bounds value from box bounds;
`shrinkContentCancel` removes that separate value so content aliases the box
again. The pinned
[Flow integration](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/index.ts)
applies padding through `copyAndSpread(..., true)` and then `unsign`. This is a
bounds operation, not a way of shrinking children to fit.

`FlowStyle.content_bounds_policy = ContentBoundsPolicy::LeaferShrink` selects that
padded-bounds normalization. A shared `FlowLayout::content_bounds` query exposes
the same intrinsic-local box used for percentage resolution, wrap and placement.
The underlying box and child geometry are not shrunk. Clearing padding returns
the exact intrinsic box without leaving an old cached inset behind.

The unusual over-padding behavior is intentional: pinned
[`BoundsHelper::unsign`](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/math/src/BoundsHelper.ts)
reflects a negative extent and shifts its origin. For a 220-wide box with left
180/right 100, the content box starts at 120 and has width 60. Legacy Flow
instead clamps that extent to zero at x=180. `ContentBoundsPolicy::Clamp` remains
the default so existing JSON and geometry retain their behavior. This additive
policy is a Flow API, not a claim that upstream has a boolean public
`shrinkContent` style property. Numeric layout stays in the existing f64 domain;
the public `Bounds` query, like Scene geometry, is represented in f32.

The pinned
[`flowX`](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowX.ts)
resolves unit-based dimensions from `box.__layout.contentBounds` before line
formation. Flow's explicit fractions follow that bounded definite-parent rule.

## Text first-baseline rows

`AlignItems::Baseline`, including `FlowItem.align_self`, aligns each participating
text's first alphabetic baseline within its own row. The item must be Text with
explicit `TextSizing::Natural` or `AutoHeight`, in a Row/RowReverse, and have a
positive axis-aligned transform. Mixed nontext rows can opt those other items
into Start/Center/End separately. Column baseline, synthesized baselines for
nontext boxes, rotated text, and rich inline baseline groups are not implemented.

The source-compatible `TextMeasurer::first_baseline` hook returns a distance from
the first logical line-box top. Its default returns an explicit unsupported
error, so existing `TextMetrics` literals and ordinary backends remain unchanged.
Opted-in baseline rows require a backend that implements the hook. GPUI uses the
same native shaping and `(line_height - ascent - descent) / 2 + ascent` formula
as `gpui_raster`, with panic handling and a font-cache epoch check. Baselines
outside the first line box are rejected, including extremely tight line heights
that would require supporting out-of-box baselines. This bounded rule does not
claim full CSS baseline alignment.

Layout adds text top padding, maps through the positive y scale, and measures a
line as max ascent plus max descent. Mixed font sizes/padding therefore do not
incorrectly use only max item height. Wrapped lines get independent baselines;
reverse changes visit order without changing tree/paint order. Bad metrics,
missing fonts, panics and invalid baselines discard the whole candidate. Native
font shaping and final painted alignment still require the host-native fixture;
fixture backends prove the model contract, not font pixels.

## Dirty dependency selection

`arranged_dirty(previous, candidate)` is available on both `FlowLayout` and
`MeasuredFlowLayout`. `previous` must already be a fixed point of the same
configuration and font generation; use ordinary `arranged` after configuration
or global font changes. The reactive provider certifies a full initial fixed
point before installing, then uses the same dependency selector on engine edits.

Changes to configured container dimensions/topology and participating child
geometry seed dirty containers. Content-size dependencies travel upward;
parent-owned size allocations travel downward. Text measurement follows selected
parents and changed text inputs. Fixed child-container translation alone does
not remeasure its contents. A worklist visits each selected container once when
computing the dependency closure, including valid cross-axis feedback edges.
Independent branches are not solved or measured. Metadata/fill/opacity/visibility
changes are not geometry inputs; stroke-dependent layouts additionally inspect
their existing declared stroke dependencies.

All candidate validation remains global. Cloning/scanning/validation costs are
not eliminated, and this is not a general observer graph, Taffy tree cache, or
large-document performance guarantee. `FlowReport.containers`, `items` and
`passes` report actual selected arrangement work. Both dirty and full layout use
the existing private Taffy 0.13.0 numeric line adapter and f64 fallback; no solver
version or default shrink behavior is changed.

## Explicit frame staging and host contract

`FlowFrameQueue::new(layout)` or `FlowFrameQueue::measured(measured_layout)` owns
immutable configuration plus at most one pending scene candidate. Call
`enqueue(&editor, edit)` to stage an edit. It returns true only on the
empty-to-pending transition. The host must request one genuine next-frame
callback on that transition, then call `flush(&mut editor)` from that callback.
Merely enqueueing does not commit or schedule anything. Repeated input before
that callback coalesces into one pending candidate and one atomic editor
transaction/Undo boundary.

- Maximum 256 accepted edits per pending batch (`MAX_FLOW_FRAME_EDITS`)
- Edit closure errors/panics leave earlier staged input and the document intact
- Scene topology is validated while staging; layout ownership is validated at flush
- Engine identity, document epoch, revision and provider token must still match
- An intervening commit, reset, replacement or provider change rejects stale work
- Initial flush solves everything; subsequent settled stamps enable dirty reflow
- Final geometry must agree with other installed derived owners or the commit rejects
- A failed flush retains input for explicit retry after backend recovery or cancel
- `cancel` discards pending input; `invalidate` forces a full next arrangement
- Empty flush returns `None` and performs no history operation

The host must generation-guard callbacks across cancel/replacement/unmount, avoid
busy retries on persistent failures, and expose a failure instead of silently
claiming a frame committed. Font invalidation must invalidate both the backend
and this queue before the next flush. Undo/Redo use the editor's existing exact
snapshots; queue state and scheduling tokens are runtime-only and not serialized.

## Validation scope

`crates/flow-layout/tests/advanced.rs` covers model percentage axes/scaling,
constraints/exclusion/reverse/wrap, definite-parent and ownership rejection,
shrink/over-padding/cancel, old JSON, first baselines/padding/scale/wrapped lines,
missing or bad metrics, independent dirty text branches, seeded nested percentage
reflow, repeated fractional idempotence, frame edit budgets/staleness/panics,
one-Undo coalescing, recovery and derived-owner rejection. Existing layout,
ratio, Taffy compatibility, text and reactive suites remain required.

The completed model checkpoint passed 238 layout/text tests and doctests,
including 28 new advanced cases. The GPUI-feature flow-text suite also passed
13 unit tests plus one doctest; its native baseline override compiled, and strict
Clippy passed for the GPUI-feature text crate. These tests do not launch a native
window. Native frame/pixel verification remains a separate gate; the real-host
single-frame criterion is not satisfied by the model checkpoint alone. No Actions
or deployment was used.
