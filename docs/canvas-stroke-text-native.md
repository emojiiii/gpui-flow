# Native Canvas shaped-outline acceptance

> 历史验证记录：旧 Studio 原生演示已于 2026-10-05 移除。本文的源码路径、命令、交互矩阵与结果仅对应当时版本，不是当前可运行入口或新工作台验收。当前入口见[入门](getting-started.md)，测试保留范围见[迁移说明](studio-retirement.md)。

The original broad run preserved a real DPR-2 work-limit failure. A focused
follow-up accepted the same clip/alpha artwork in two explicit bounded batches,
then checked rejection and regeneration against two nonempty resources. Neither
run raised the default 32,000,000 surface-write limit.

The production [StrokeTextBox contract](canvas-stroke-text.md) is implemented in
[`84e016b0`](https://github.com/emojiiii/gpui-flow/commit/84e016b04b8192015000bc0ab0de49bc5e83ad9e).
The original fixture is
[`b507ca8a`](https://github.com/emojiiii/gpui-flow/commit/b507ca8a741f50ecb0b5859a1922575418ab3667);
the explicit-batch remedy is
[`747ecf55`](https://github.com/emojiiii/gpui-flow/commit/747ecf5528429b722014b3c065f2690f4c56b41f).
The [fixture contract](canvas-stroke-text-fixture.md) and
build summaries (historical record in Git history)
retain the source, checks and earlier failed result.

## Original run and preserved failure

The 0338Z package ran 2026-10-03 **03:41:24–03:46:46 UTC**, with normal exit 0,
20 physical button activations across nine controls, 21 stdout receipts and one
failed action on stderr. Curves/counters, typography, pens/affine and the later
epoch, safe/reset resize, DPR, export and Undo controls succeeded. Scenario 4
accepted DPR 1 but rejected DPR 2 with `WorkLimit`.

The fixture had separately committed Clear before that rejected drawing batch,
so DPR 2 was empty. This does not show partial publication by the rejected
batch. Its subsequent eight rejection cases on each resource preserved their
baselines, but the empty right baseline did not prove rollback of visible ink.
The failed run remains failed; normal process exit does not override that result.

The preserved session has 108 actual Canvas raw/PNG pairs and 24 original JPEG
desktop captures. Independent auditing passed 1,240 byte/receipt checks,
17 trace checks and 25 checks of the successful geometry subset. These checks
do not accept the missing DPR-2 clip/alpha result.

## Focused two-batch follow-up

The 0357Z package ran **04:03:37–04:05:56 UTC**, with normal exit 0. Three Next
clicks reached scenario 4; one Reject suite and one Regenerate completed the
five-button targeted tour. The revised fixture requires nonempty pixels and a
retained current path on both resources before testing rejection.

Scenario 4 deliberately submits its balanced upper current-path/even-odd/nested
clip block, then its lower translucent overlaps and sentinel. All four
resource/batch draws were accepted, published new nonempty snapshots and
completed. They are separate public resource transactions, not one atomic
gallery or a cross-resource transaction. Both default work ceilings remain in
force; there is no automatic retry after rejection.

Eight rejected submissions per resource produced the intended errors while
preserving snapshot identity, generation, exact premultiplied RGBA and retained
path replay. Regeneration returned both resources to the exact pre-rejection
raw buffers. The new DPR-1 clip/alpha buffer also equals the originally accepted
DPR-1 buffer byte for byte.

Independent checks of all 48 raw/PNG pairs passed **589/589**: dimensions,
checksums, alpha, premultiplication and PNG demultiplication had no discrepancies.
The **31/31** geometry/alpha checks include the two DPRs and both font faces:
transparent counter regions, clip hole/exterior alpha 0, restored right-side ink,
single-draw alpha 128 and independent overlap alpha 160. These findings use
actual committed Canvas exports, not JPEG screenshot pixels.

An initial audit draft incorrectly compared each saved regeneration step with
the stdout wrapper containing that step. Its 24 schema mismatches are retained.
The corrected audit compares the complete saved object with `stdout.step` and
checks the wrapper separately; no native data or pixel assertion was changed.

## Provenance and supported boundary

Each executable has 490 matching conservative workspace input hashes before
and after its actual link, all matching its frozen archive. Published source
matches 489 code/font/config files exactly; the one contract document changes
only post-link build/helper status text, with its original version preserved.
External dependencies, compiler and system libraries are outside this comparison.
Both executables are retained in verified, reversible zstd archives.

Both runs use the same App-owned Linux X11 bridge with the original CC0 static
TTF/CFF fixtures. App IDs 0/1, actual glyph outlines and committed resource
snapshots are exercised. Native diagnostic shaping observes fi glyph 19,
AV kerning and precomposed accent glyph 21. Combining acute appears at
x≈25.2, y=0 after m; native mark-anchor application is not established. These
diagnostic shapes are separate from the private line objects used by the adapter.

This accepts the bounded solid-color Canvas outline-stroke route. It does not
accept arbitrary system fonts, every OpenType format, positive bold/italic
faces, decorations, patterned text strokes, generic scene Text outlines,
Wayland/other platforms, browser pixel equivalence or GPU allocation identity.
Cancellation is set before entry; late Restore fails during admission. Mid-work
epoch/upload failures and caller-reduced quotas retain their separate automated
coverage rather than a physical timing-injection claim.

Earlier fill/measure, current-path and pattern evidence remains independent.
The historical pattern JPEG exterior observations remain unresolved and are not
reclassified by these lossless text-stroke exports.

## Fixed ELM-10 acceptance audit

The original [45-task ledger](https://github.com/emojiiii/gpui-flow/blob/05c92dcfea58c5bd914808e0b629662ea4fb1e28/docs/leafer-task-ledger.md#L63)
requires fillText, measureText, strokeText; the six named current-path
constructors; and createPattern repeat/no-repeat with transform. Its separate
dependency row requires the same-App font bridge, glyph budgets and real stroke
semantics rather than bounding boxes. It expressly describes bounded contracts
and excludes full upstream/API and cross-platform compatibility. The required
operation list is unchanged by this acceptance update.

| Original required behavior | Retained evidence |
| --- | --- |
| fillText and measureText | [Original fill/measure contract and native run](canvas-text-commands.md) |
| beginPath, moveTo, lineTo, quadraticCurveTo, bezierCurveTo, closePath | [Current-path source, automatic and native evidence](canvas-current-path.md) |
| createPattern repeat/no-repeat and setTransform | [Pattern contract](canvas-patterns.md) and [native report](canvas-pattern-native.md) |
| Actual strokeText with same-App glyph identity and budgets | [Shaped producer](shaped-text-outlines.md), [Canvas adapter](canvas-shaped-outline-adapter.md), [lowering](canvas-stroke-text.md) and the two native runs above |

Native mark-anchor support was an additional shaping probe; it is not named in
that frozen operation list. The outline route preserves the host's actual x/y
glyph positions, including nonzero offsets when supplied; it does not discard
them or promise a different shaping engine. The observed native mark result is
retained as a host-shaping limit, without converting it into a successful mark
test or excluding required stroke geometry.

Pattern clipping itself remains required and covered by the existing actual
RGBA compositor tests and native control observations. The two unresolved
5×5 exterior samples are from lossy JPEG desktop captures: they cannot establish
framebuffer or Canvas edge bytes. No raw-byte browser/GPU equivalence requirement
appears in the original task. Their 48/50 screenshot result remains visible;
this acceptance does not turn it into a lossless pass or prove its cause. A
future confirmed rendering defect would require reopening the affected behavior.
