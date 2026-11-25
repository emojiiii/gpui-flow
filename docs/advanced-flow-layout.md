# Advanced Flow layout and bounded scheduling (LAY-03)

This is the advanced layout implementation for the frozen LAY-03 acceptance:
fixed-parent percentages, padded `shrinkContent` semantics, measured text row
baselines, dependency-selected reflow, frame coalescing, and atomic rejection of
cycles/ownership conflicts. The opt-in `SceneView` bridge now wires the model
queue to `Window::on_next_frame`; see [the bridge contract](flow-frame-layout.md).
The native fixture below exercises that bridge. A model test or callback receipt
alone is not a claim about GPU presentation or final text pixels.

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
explicit `TextSizing::Natural`, `AutoHeight` or `AutoWidth`, in a Row/RowReverse, and have a
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

The mode-aware `first_baseline_writing_mode` hook preserves that horizontal
contract and explicitly opts into Y/Y-reverse source baselines. It uses the first
logical paragraph and its own inline advance, not the longest column. Empty
first paragraphs use native empty-line metrics without inserting a glyph. The
vertical baseline must lie inside its native font-size cell and the content box;
unsupported backends or out-of-box results reject the transaction. Horizontal
AutoWidth additionally includes its fixed-height Top/Middle/Bottom alignment
slack. See [text measurement](text-measurement.md) for the complete bounded API.

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
the private Taffy numeric line adapter and f64 fallback. Flow retains its
default shrink behavior.

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


## Native LAY-03 fixture and physical script

Run `cargo run -p flow-studio --bin advanced_flow_layout`. This fixture uses the
actual `GpuiTextMeasurer` and `SceneView::enqueue_flow_edit`; it never calls the
model queue's `flush` itself. Each physical button executes a complete bounded
scenario. Assertions fail loudly; JSON lines on stderr carry the action,
callback count, revision, pending count, actual measured inputs and selected
container counts. Keep that log together with the binary/source hashes and
screenshots when recording acceptance.

The initial canvas has four independent containers:

- Fixed 620 × 130 parent with padding 20: 50% and 25% children are 290 and 145
  wide, with 45-high rectangles. The fractions use 580 × 90 content dimensions
- The 220 × 120 shrink panel has left/right padding 180/100 and top/bottom 90/60:
  normalized content is `(120, 60, 60, 30)` and its 50% child is 30 × 15. The
  outer panel is still 220 × 120
- The baseline row uses real DejaVu Sans at 18, 34 and 52 px with top padding
  12, 3 and 7. The yellow line marks the shared first alphabetic baseline
- The right-hand text branch is independent; local edits must not measure or
  mutate it

Use a real mouse in the native window, waiting for the footer after each click:

1. Capture the initial native window. Check all three differently sized text
   samples visually rest on the yellow baseline, and the two percentage
   rectangles and small shrink rectangle agree with the dimensions above
2. Click **Batch 20**. The staged log must say `batch20-staged-uncommitted`, with
   20 pending edits, original width 620 and unchanged revision. The later
   `batch20` receipt must have 20 edits, one callback and revision +1. Parent
   width becomes 500 and children become 230/115 wide. The first queue solve
   covers all four containers
3. Click **Local text**. Only the small text changes. `local-text` must report
   one selected container, no `Untouched Ag` measurement, and one revision.
   The stable parent and text snapshots must remain exactly equal. Capture
   the footer and the unchanged native baseline
4. Click **Undo**. `undo-exact` proves one step restores the entire pre-local
   scene. Then click **Batch 20**, followed by **Undo**, to verify a twenty-edit
   frame is also one exact Undo boundary
5. Click **Owner conflict**. A separate installed derived provider competes
   for the first rectangle's width. Expect an error containing `conflicts`,
   no committed geometry or revision change, and one retained pending edit
6. Click **Stale reset**. The fixture stages an edit, resets the engine to an
   identical-looking document, and verifies the previous element handle is
   invalid. The posted callback must report `stale` without writing the new
   document or advancing its revision
7. Click **Backend fail**, then **Retry**. Failure must retain one pending edit
   and the exact scene/revision. After recovery, Retry posts a genuine new
   frame callback and commits the same edit with exactly one revision
8. Click **Cancel staged**. The fixture posts an edit then cancels it before
   the frame. A later real-frame verifier requires zero pending edits, no
   extra bridge receipt, and unchanged scene/revision
9. Click **Replace staged**. This repeats the check while replacing the queue;
   the old callback must never flush the replacement. Capture the footer
10. Click **Batch 20** again to verify normal interaction recovers after the
    cancellation/replacement scenarios, then close the window normally

`Recover queue` discards failed staged input and restores the normal backend
and queue. Local text is available only after a successful frame has established
a settled dependency baseline; Undo and queue replacement invalidate it. This
fixture does not silently fall back to an immediate editor transaction.

Two direct unit tests in `flow-text/src/gpui_backend.rs` invoke the actual
`GpuiTextMeasurer::first_baseline` override. A deterministic platform shim checks
shaped ascent/descent, centered leading, empty/multiline strings, invalidation,
invalid requests, missing fonts and caught shaping panic. This proves the
adapter path without requiring a native window; the native fixture supplies
real-platform font measurement and physical pixel inspection separately.

The physical script was executed on the Linux host recorded below. This is not
a claim that it passed on every host. Preserve the executable, receipts and
screenshots when repeating it. No Actions or deployment is required.


Fixture preparation checkpoint (2026-10-02): both new direct baseline tests
passed, strict Clippy passed for all GPUI-feature text targets and this Studio
binary, and the native fixture linked successfully. Running the frozen binary
with `--probe` used real platform fonts and passed all four-container geometry,
first-baseline equality and fixed-point assertions (two initial layout passes;
percentage widths 290/145). `--probe` opens no window and does not exercise the
native callback or inspect painted pixels. The physical run below separately
completes the frame/pixel acceptance gate for this frozen contract.


## Completed native acceptance: 2026-10-02

The fixed **LAY-03 contract can be closed** for the declared subset on this Linux
host. The frozen fixture ran from **18:19:04Z to 18:21:16Z**, was driven with the
physical button sequence above, and exited normally with status **0**. An
independent audit read all 19 raw JSONL records, verified the executable/hash
receipts and inspected all 12 original PNG screenshots. Full evidence, exact
payloads and individual screenshot hashes are retained in
`2026-10-02-advanced-layout-summary.json` (historical record in Git history).

The receipt audit recomputed the expected sequence rather than trusting the
footer's `PASS` strings:

- Three twenty-edit batches retained the committed scene/revision while
  `pending=20`, then each produced exactly one callback and one revision. The
  first changed parent width 620→500, the second 500→620, and the final recovery
  520→620. Child widths followed content width minus 40 padding
- The local-text batch selected exactly **one container and three items**.
  Its measurement inputs contain only the three baseline participants, with no
  `Untouched Ag` measurement. All three baseline participants can remeasure;
  this is dependency-selected work, not a one-shaping-call claim
- Two Undo actions restored the precise pre-edit scene snapshots asserted by
  the frozen fixture. Neither added a frame callback. Screenshots independently
  show restoration of text placement and percentage geometry
- Owner conflict, reset-stale work and backend outage each retained one pending
  edit without committing geometry or advancing the callback-time revision.
  The deliberate document reset itself advanced revision 5→6 before the stale
  callback; it also invalidated the previous element handle
- Retry consumed the retained edit, advancing revision 6→7 and width 500→520.
  Cancel and replacement both left revision/callback counts at 7, with zero
  pending edits. Final recovery reached callback **8**, revision **8**, pending
  **0**. There were five successful and three failed frame receipts in total

Original screenshot pixels independently corroborate those receipts. Horizontal
color spans give exact percentage widths **290/145**, **230/115** and **240/120**
as appropriate, plus the unchanged **30 × 15-pixel** shrink child. Vertical
samples also confirm both percentage rectangles remain 45 pixels high. The three real-font
sizes visibly share the yellow alphabetic baseline. The independent text-panel
RGB bytes are identical across all 12 images. Scene-region RGB bytes match
between the original narrowed state, both Undo results and all three rejection
screenshots; retry/cancel/replacement also match one another. The final recovered
scene region is byte-identical to the initial image. The static caption continues
to say initial width 620; actual geometry, receipts and footer show later widths.

### Executable provenance and remaining boundaries

The tested executable SHA-256 is
`7f8da361691d6fd14a1093ddd3b6a0b720de6c4e198ba9696a7818e8d52059ae`.
It was linked from reconstructed local checkpoint
`81a405783b127ab08fa4793b694506971d0309de` (tree
`e7073665c7db2aee5dbd8ad51d678178dc0b3e77`, corresponding to the reported remote
bridge checkpoint `ca44fa43`), **plus uncommitted fixture, crop and Leafer JSON
work**. The advanced model core was previously published at
`d9a3921525a1989cb332963900ff84d648ac51ba`. The whole executable must not be
attributed to the exact `ca44fa43` tree.

The link-time hashes of the fixture, baseline backend, bridge implementation,
shared `SceneView` module and lockfile still matched during audit. The original
three-file fixture archive and checkout-status receipt are preserved. An
additional manifest hashes all **210 workspace inputs** named by the linker
build's Cargo dependency file as observed during audit. Four unrelated crop
source inputs had changed after linking. This later inventory is explicitly not
a retroactive cryptographic attestation of uncaptured link-time WIP bytes; no
complete reproducible build-tree claim is made.

The callback is an atomic document/layout commit receipt, not a GPU fence or
latency measurement. Screenshots establish the visible result on this host;
there is no macOS/Windows or arbitrary font-fallback parity claim. Opt-in queue
scheduling does not change immediate editor APIs. Full CSS baseline groups,
implicit flex shrink, general observer-graph performance and unsupported cycles
remain outside the contract. Wrap/reverse, budgets, malformed metrics and
additional provider/window lifecycle guards retain their model/bridge-test
coverage rather than new physical scenarios in this run. The audit made no
production changes and invoked no Cargo, UI, commit, push, Actions or deployment.
