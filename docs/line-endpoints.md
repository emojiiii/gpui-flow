# Bounded straight-Line endpoint editing

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

This Leafer-aligned slice edits the intrinsic `start` or `end` coordinate of a
single built-in `Shape::Line`. It reuses the existing outer-selection tool,
plugin registry, target-aware event routing, selection capture, engine transaction
and history. No new geometry dependency, persisted tool object, parallel editor
framework or document-format change is introduced.

## Coordinates and visible controls

- Exactly one selected, attached built-in Line gets two 8-screen-pixel squares,
  at its actual start/end coordinates through its full world transform and the
  viewport. Picking uses the same centers with a constant 10-pixel radius
- The nearest endpoint wins overlapping hit circles; an exact tie selects Start
  independently of input slice order. There is no AABB corner approximation
- The endpoint controls replace that selection's box-resize/rotation controls.
  Existing selection outlines remain; multi-selection retains shared transform
  tools. Disabling the endpoint plugin restores ordinary transform controls
- Pointer-down freezes the actual world pointer, Line coordinates and complete
  invertible world matrix. Each move maps the world pointer delta through the
  inverse linear matrix and adds it to the selected initial local endpoint
- Off-center presses do not jump. The opposite endpoint, object transform and
  full world transform stay fixed. Translation, rotation, skew, reflection and
  nonuniform parent scaling are supported when finite and invertible
- Shift keeps only the dominant component of the cumulative local drag delta;
  equal components keep Y. This constrains movement rather than snapping the
  line angle. Releasing Shift resumes the unconstrained delta
- The frozen minimum length is `min(initial_world_endpoint_distance, 1)` in
  world units. An existing sub-unit nonzero line may stay unchanged, grow and
  return to its initial length; it cannot shrink below that initial length
- Length uses f64 endpoint differences through the full world linear matrix.
  It is independent of translation, zoom, stroke, shadows and axis-aligned
  bounds. A shorter/zero-length proposed sample is ignored, retaining the last
  accepted geometry; moving beyond the opposite endpoint is allowed if the new
  distance meets the floor. There is no clamp, inferred endpoint swap or flip
- Returning to the exact pointer-down position restores the exact original
  coordinate. No-op and out-and-back gestures preserve existing Redo

Only the scene's straight `Line { start, end }` representation is admitted.
All `Path` objects, including a two-command MoveTo/LineTo path, curved/open/closed
paths, polygons, Custom shapes and multi-selection endpoint editing are excluded.
Coincident endpoints, nonfinite/overflowing geometry and singular/nonfinite or
non-invertible-to-f32 world matrices are rejected. Locks on the target or any
ancestor, read-only and plugin policy remain authoritative.

## Atomic mutation and ownership

`HitTarget::LineEndpoint { id, endpoint }` routes tree events to the real live
Line before default actions. `SceneCommand::SetLineEndpoint` carries an
engine-scoped generation handle, endpoint discriminator and proposed local
coordinate. Middleware may normalize the coordinate, but cannot change its
kind, target or endpoint. It cannot replay a cached whole Shape: the transaction
updates only the chosen field on the current Line.

The command checks the complete current selection and attachment, then validates
both the input and final derived scene inside one engine transaction. Layout or
normalization changing the Line type, locks, opposite endpoint, chosen endpoint,
local/world transform or minimum-length contract rejects the entire candidate
before observers, caches or history can see a partial result. Styles, metadata
and unrelated object fields are never replaced by a cached shape snapshot.

The existing selection capture holds engine/document identity, live generation,
attachment epoch, selection membership and accepted engine revision. Only a
successful owned endpoint command advances that revision. The same capture also
stores this Line's initial minimum length. Any external document mutation
conservatively invalidates the capture; cleanup never writes an obsolete
baseline over that external edit.

Release produces one Undo step for any number of accepted moves. Escape,
focus loss, pointer cancel, button loss, plugin disable/uninstall, outer-tool
switch/disable/disposal and denied/invalid later command cancel a still-owned
gesture. Foreign pointers cannot operate it; pointer leave alone retains capture.
Space-pan and a live Text session retain ownership. The native adapter follows
the existing loaded default outer-tool identity; it does not add an independent
inner editor session or claim a custom tool-widget factory.

## Pinned source alignment

Read-only comparison used the existing independently pinned upstream snapshots:

- [LineEditTool](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/LineEditTool.ts): true from/to edits, inner-coordinate drag movement, dominant-axis constraint and two endpoint handles
- [Line](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Line.ts): default width/rotation form, points mode and optional curve/closed data

Leafer's LineEditTool also branches on path/points input and supports around
movement of the opposite endpoint; its default Line can encode length/angle as
width/rotation. This Rust slice deliberately uses the actual existing explicit
start/end Line API, never assumes that an arbitrary Path is a straight Line,
and does not reproduce every upstream representation. It adds an explicit
minimum-length safety contract. Around/Alt-center, control-point/curve editing,
multiple endpoint selection and general inner editor widgets remain absent.
This is an independent implementation; no upstream code is copied and no license
is changed. New public enum variants affect exhaustive downstream Rust matches.

## Validation

Local Linux validation on 2026-10-01:

- 21 focused fixtures passed. The final affected editor/interaction aggregate
  passed 263 tests including those fixtures, existing selection resize/rotation,
  Text ownership, history, policy and tree-router regressions
- 98 UI library tests passed, including native double-click rejection of the new
  endpoint hit target. Workspace/all-targets strict Clippy, formatting and the
  Linux `line_endpoints` example build/link passed with the final source
- Locked offline Rust 1.98.1, the restored existing cache, one-job/debug0 profile
  and monitored 3 GiB disk floor were used; no dependency was added
- A real cloud-desktop window showed two true handles on an initially horizontal
  Line. An off-center End drag changed only End by the delivered pointer delta,
  with Start and the identity transform unchanged. One Undo restored the exact
  baseline and one Redo restored the complete multi-move gesture
- A subsequent Shift drag of Start kept its X coordinate exactly unchanged while
  moving Y, preserving End and the transform. An initially vertical Line also
  accepted a true endpoint edit despite its zero-width geometry bounds
- Space-pan starting on an endpoint preserved the document and selection. After
  pan and 1.5x zoom, a Line under reflected, nonuniform, rotated and skewed
  matrices accepted a 60/-30 screen-pixel End movement; its recorded local
  coordinate change mapped back to approximately 40/-20 world units, while the
  opposite endpoint and local matrix remained unchanged
- Read-only hid the two endpoint controls while keeping selection outlines.
  Selecting a quadratic Path showed the existing box/rotation controls instead
  of Line endpoint controls. Final settled frames and accepted geometry traces
  were checked; titlebar close removed the window and the launcher exited 0

Initial regressions exposed two fixture assumptions, not product failures:
remove/reinsert in one atomic Scene transaction preserves the existing generation,
and direct-engine detach prunes stale selection IDs during reconciliation. The
fixtures now perform a real two-transaction destroy/recreate and expect the
existing selection cleanup. A native UI test-only import path and a nested-if
strict lint in the example were also corrected before the passing final runs.
Read-only review caught a finite-screen-to-infinite-world press case; the plugin
now checks the converted press before capture, with a passing regression.

Some immediate screenshots showed the earlier queued frame, so native acceptance
used fresh settled screenshots plus the trace. Terminal paste was unavailable;
the documented keyboard-input fallback launched the app. A window-bound drag
produced no verified result; actual acceptance used the full-desktop pointer API.
None of these intermediate attempts is counted as passing input coverage.

Not freshly hand-tested: in-flight Escape/focus-loss/outside-release timing,
minimum-length overlap/dead-zone and crossing samples, actual Text input/IME,
locked and multi-selection presets, DPI changes, Windows/macOS or large-document
performance. Their applicable pure regressions do not replace native coverage.
Unchanged whole-workspace media/document/headless suites were not rerun.

Focused regression source: `crates/flow-interaction/tests/line_endpoints.rs`.
The native example is `cargo run --locked -p flow-studio --bin line_endpoints`;
`-- --trace` logs accepted endpoint coordinates/transforms only. It includes
horizontal, vertical, diagonal, reflected/nonuniform/skewed-parent Lines,
curve exclusion, multi-selection fallback, locked target, Text, Undo/Redo,
read-only and zoom controls. No GitHub Actions, deployment or user-computer work
is part of this slice; cross-platform acceptance remains deferred.
