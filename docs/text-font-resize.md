# Intrinsic Text font-size resize

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

Status: implementation and 12 new focused regressions pass, strict editor/
interaction and example Clippy, isolated final-source checks and real Linux
window acceptance pass.

`SceneResizePlugin::default().with_text_resize_mode(TextResizeMode::FontSize)`
opts named single-selection plain Text into font-size resizing. Transform remains
the default; the independent fixed Box mode still reflows without changing font.
Unsupported targets and shared selections keep their existing transform behavior.
Plugin replacement cancels an active owned capture using the existing lifecycle.

The separate editor geometry reuses the signed eight-handle resize kernel. Like
pinned Leafer `scaleResizeFontSize`, top/bottom use the Y ratio while the other
handles use X. Shift does not change this choice. Font, fixed box and four padding
values scale together; affine linear terms remain unchanged and the opposite
corner/edge midpoint is stationary through affine parents. Flow line_height is a
dimensionless font multiplier, so it stays unchanged: the resulting absolute line
height scales once through font_size, not twice. Canonical content and all other
Text style fields stay current. Each preview uses its captured initial geometry,
not the previous preview's already-scaled values.

`SceneCommand::SetTextFontResize` carries a live target and only the requested
font/box/padding/pose values. It reuses the existing intrinsic Text adapter:
identity/attachment and revision capture, selection membership, inherited locks,
Edit capability, middleware kind/target protection, unchanged affine linear
terms, Text validation and post-layout requested size/pose checks. Invalid values,
layout ownership conflicts or singular/stale targets reject atomically. The
normal gesture completion is one Undo; replacement/cancellation restores only an
owned unchanged baseline. External content edits cancel capture without replaying
old text.

This explicit tool caps font size at 512 logical units, box dimensions at 1,000,000,
with the existing positive resize-factor bounds. Padding must leave a positive
content area. Existing initial font sizes above the tool ceiling do not opt in.
Rendering may reject earlier at high DPR/world scale under its separate 512-device-
pixel glyph and surface budgets; this is not a promise that every allowed model
pose can render. No-motion keeps original values exactly. The current mode does
not override auto-size/responsive ownership or add arbitrary component intrinsic
resize, character spacing, WYSIWYG, skew or custom pivots.

References: fixed Leafer [resize dispatch](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/resize/src/resize.ts)
and [scaleResizeFontSize](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/resize/src/scaler.ts).
The source distinguishes scale, intrinsic dimensions and font-size policies;
Flow retains explicit modes and typed atomic transactions rather than JS mutation.


## Validation

Twelve new regressions cover all eight affine anchors, pinned axis choice,
Shift behavior, exact no-op, bounded inputs, font/box/padding proportionality,
noncompounding previews, default Transform/Box compatibility, native-only and
existing affine poses, one Undo/Redo, plugin cancellation, stale external content,
read-only/forged linear changes, middleware kind protection and derived-layout
rollback. Final-source checks passed **282 tests**: 14 selected editor tests and
268 interaction tests, zero failed/ignored. Strict editor/interaction all-targets
Clippy, example Clippy and actual native linking passed. Unchanged unrelated UI
suites were not rerun; the example is checked through the real UI dependency.

The exact source tree was exported to an isolated source directory before final
validation/linking, using the shared existing toolchain/cache. Parallel query-index
and new paragraph-style changes in the main checkout were excluded from this
snapshot, not silently counted as validated by it.

The `text_font_resize --trace` window ran 2026-10-02 15:23:27–15:29:05 UTC and
closed normally with exit 0. Real drags changed font 23→34.5→46 with matching
fixed-box/padding changes and unchanged affine linear terms. Right/bottom handles
preserved the opposite anchor; Undo/Redo restored a whole gesture and the Redo
artwork region was pixel-identical. Default Transform mode retained font 23 and
intrinsic 220×100 while stretching the matrix. Existing affine-parent Text resized
its real font; native-only Text could shrink with the CPU bridge withdrawn.
Read-only hid the intrinsic handles. The separate affine fixture showed the
expected missing-backend diagnostic when the bridge was withdrawn. The blue Rect
fixture continues to occlude overlapping enlarged text by ordinary paint order.

Stdout was empty and stderr held 808 expected transform/box trace lines only.
Held-pointer cancellation, all corner permutations and invalid budgets have
automated coverage, not separate native claims from this window. No WYSIWYG or
cross-platform/IME acceptance is claimed.

Exact source commit `9b12aac930a5e2678dbc5afe0146c42c0e87ae8a`, tree
`8baa5b9644beb02ed188355d7a114f9664bea3f9`; archive SHA-256
`d4dee791e6b6359e0617d98829c1963f464ce0045f98782c98afb99776d8a009`;
binary SHA-256
`63426cd4b219ade9cf7c03510f0e182b1a902555e575e0da0ea6c4bd23a81ee0`.
Screenshots, trace and normal close receipt are retained with the validation
artifacts. Run `cargo run --locked -p flow-studio --bin text_font_resize`.
