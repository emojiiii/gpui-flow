# Bounded shared-selection resize

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

The later [Image transform slice](image-transform-tools.md) removes Image-only
uniform restrictions and supports affine Image roots; Text remains restricted.
Original dated validation receipts below retain their historical scope.

Status: bounded implementation revalidated on Linux on 2026-10-01 after a cloud
workspace interruption and source reconstruction. Fresh automated checks and
native-window coverage are recorded below with their limits. No full Leafer or
cross-platform parity claim is made; no GitHub Actions or deployment was run.

## Shared geometry

Two or more selected IDs use one world-axis-aligned box. Participants are
`SceneSelection::roots`, so selected descendants of selected ancestors are not
transformed twice. This includes a parent/child selection with one independent
root. A sole selected ID retains the existing local-axis
[single-selection handles](resize-handles.md); legacy bottom-right APIs remain
unchanged.

The shared box unions each root's cached `SceneEngine::world_bounds`: geometric
subtree bounds including strokes and internal child clipping. It excludes
shadow/effect bounds. Existing conservative selection bounds can extend beyond
ink hidden by an ancestor clip.

Pointer-down freezes the union, actual world pointer position, each root's
local matrix `M` and parent world matrix `P`. The existing eight-handle helper
calculates one world delta `D`; every root receives
`M' = inverse(P) * D * P * M`. The opposite shared anchor, spacing and world axes
are preserved. Existing rotated, skewed and reflected vectors/Images are supported;
new flips are not introduced. Identity delta returns the exact initial matrices,
avoiding inverse-rounding drift and preserving Redo for no-op gestures.

- Ordinary sides change one shared world axis; corners change both
- Shift scales the complete selection uniformly around the opposite anchor
- Any Text descendant in any selected subtree forces the entire selection
  to scale uniformly, including vectors and spacing
- Uniform corners use the existing signed-diagonal projection; uniform sides
  use their active axis and keep the opposite edge midpoint fixed
- Each minimum factor is `max(0.05, 1 / max(initial_world_union_length, 1))`;
  maximum factor is 100. These are per-gesture shared-box limits, not descendant
  or screen-pixel limits. Sub-unit unions may remain unchanged or grow, but
  cannot shrink further
- Custom subtrees, locked roots/descendants, singular or non-finite matrices,
  zero-area geometric roots, or overflowing unions/candidates reject the whole
  shared operation
- Native Text world transforms must remain finite, axis-aligned, positive
  and uniform. Existing platform/style constraints are unchanged

Paint and picking share the existing 8-screen-pixel squares, 10-pixel
nearest-center picking and directional cursors. The shared outline appears
alongside the selected objects' existing outlines.

## Atomicity, capture and tools

`HitTarget::SelectionResizeHandle { handle }` addresses the complete selection;
no arbitrary primary object owns its input or tree events. Each move emits one
`SceneCommand::SetResizeTransforms` batch. Middleware may normalize matrices,
but cannot drop, add, duplicate, reorder, retarget or replace its ordered handle
membership. Every member is checked before one editor/engine transaction. A
final validator runs after normalization and reactive layout, before any
identity/cache/observer/history commit. Failed members or layout expose no
partial change and cancel the still-owned gesture. Release records one Undo
step; no-op gestures preserve Redo.

Capture records each independent participant's engine-scoped generation handle
and attachment epoch, engine/document identity and the accepted engine revision.
Only a successful owned batch advances that revision marker. This covers
ancestor transforms, descendant capabilities and topology without a full-tree
scan on every pointer event. Selection membership is also checked. Any external
document edit conservatively invalidates shared capture; cancellation never
replays the obsolete snapshot over it. Existing direct-engine history boundaries
are preserved.

Text inner ownership, outer-tool availability, Space-pan, read-only and plugin
policy remain authoritative. Escape, focus loss, pointer cancel, button loss,
plugin disable/uninstall and outer-controller changes cancel an owned resize.
Foreign pointers cannot operate it; pointer leave alone retains capture.

Document freshness is checked in constant time; selection comparisons and
member validation still scale with selected roots/subtrees. Existing geometry,
engine transaction and layout costs remain. No large-document performance claim
is made.

## Source alignment and limits

Compared read-only with official Leafer plugin commit
`3eb445bf34debb638766615282224b77108bf610`:

- [simulate: page-coordinate selection union](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/editor/simulate.ts)
- [TransformTool: shared world-change matrix](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/TransformTool.ts)
- [SimulateElement: safe shared transform changes](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/display/SimulateElement.ts)

This independent implementation adds no dependency, second selection framework
or persisted simulated object. The later [rotation slice](rotation-handles.md) adds
a shared center-pivot handle. Skew/pivot controls, intrinsic resizing,
Alt-center, path/line control points, multi-selection Text editing and arbitrary
custom tool views remain outside this slice. New public enum variants require
updates to downstream exhaustive Rust matches; document formats do not change.

## Fresh validation after reconstruction (2026-10-01)

- 18 new focused multi-selection fixtures passed
- 583 affected engine/editor/interaction/layout/animation tests passed, including
  those 18 and the existing single-resize, Text ownership, history and router
  regressions. These counts are not additive
- 98 UI library tests passed, including native double-click activation filtering
- Strict workspace/all-targets Clippy, workspace formatting and the Linux
  `selection_resize` native build/link passed with Rust 1.98.1 and locked
  dependencies. Cache recovery used the network; subsequent checks were offline
- The restored Rust/Cargo sources remained unchanged through passing checks and
  native acceptance. The one-job/debug0 profile and 3 GiB disk guard were used
- A real cloud-desktop window showed the shared box and accepted a mixed
  Rect/Text/Image right-edge drag: all three matrices scaled uniformly to about
  0.9, with shared spacing. One Undo restored all three and Redo restored the
  complete resized selection
- Space-pan initiated on a shared handle preserved both selection and document
  matrices. At 1.5x zoom after pan, a two-vector shared right-edge resize moved
  the box by the requested screen distance and used the same world delta across
  different transformed parents
- Selecting a parent, its Text child and another root showed one shared box. A
  bottom-edge drag scaled both independent roots uniformly to about 1.1 while
  the selected child's local matrix stayed exactly unchanged; one Undo restored
  the whole gesture
- Read-only mode hid the shared box/handles while preserving object outlines
- The native window was closed through its titlebar, disappearance was verified,
  and the launcher recorded process exit code 0

Immediate screenshots sometimes contained an earlier queued frame. Acceptance
used a fresh final screenshot and the accepted-matrix trace, rather than an
immediate frame alone. A stale computer-tool session timed out before launching
the final window and recovered after a local tool-session reset. A workspace
Clippy process was interrupted without a result; the explicit retry completed
successfully. Neither incomplete attempt is counted as a pass.

Fresh native coverage is the three resize paths described above, plus shared
box/picking, pan, zoom, history and read-only checks. All eight handles and Shift
math are covered by the fresh headless fixtures. Before the workspace
interruption, the earlier native window also exercised all eight vector handles
and Shift shrinking; those are historical observations, not additional fresh
acceptance claims.

Not freshly hand-tested: in-flight Escape/focus-loss/pointer-cancel and release
outside the view, Text inner keyboard input, every corner/Shift combination,
DPI changes, Windows/macOS, real IME candidate flows and large-document
performance. Relevant pure cancellation/Text exclusivity tests passed; they do
not replace native coverage. Unchanged whole-workspace headless/media/document
suites were not rerun.

Fixtures: `crates/flow-interaction/tests/selection_resize.rs`.
Run `cargo test --locked -p gpui-flow-interaction --test selection_resize`, then
affected aggregate/UI tests and strict lint/format checks. The native example is
`cargo run --locked -p flow-studio --bin selection_resize -- --trace`.
Its presets cover vector, mixed-native and parent/child selections; trace output
contains selected matrices only.
