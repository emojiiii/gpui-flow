# Intrinsic Text font-size resize

Status: implementation and 12 new focused regressions pass, strict editor/
interaction and example Clippy pass. Isolated final-source checks and native
acceptance are pending.

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
