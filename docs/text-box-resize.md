# Intrinsic fixed Text-box resize

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

Status: implemented and validated in focused automated checks and a real Linux
window. This is an explicit fixed-box tool, not full Leafer editor parity.

`SceneResizePlugin::default().with_text_resize_mode(TextResizeMode::Box)` opts
named, single-selection, supported plain Text into box resizing. Install it using
the existing ScenePluginHost/SceneView `replace` / `replace_plugin` lifecycle.
The default `Transform` mode, unnamed legacy resize, mixed selections and other
component types keep their previous transform behavior. Replacing the plugin
cancels an active gesture through the existing lifecycle. No extra global mode,
serialization field or dependency is added.

The separate `flow-editor/src/text_resize.rs` geometry reuses ResizeHandle and the
existing signed resize math. Width/height and anchoring translation change;
font size, padding and affine linear terms stay fixed. The opposite local corner
or edge midpoint stays anchored through existing affine parents. Shift preserves
the initial box ratio, not font size. Minimum content extent is one local unit,
or an already-smaller initial extent; padding never collapses the content box.
Each resulting box dimension is finite, positive and at most 1,000,000 local
units. No-motion keeps the original pose and size exactly.

`SceneCommand::SetTextBoxResize` carries the live target, requested dimensions and
pose. It updates the current Text rather than replaying a cached whole Shape.
Command middleware cannot change its kind/target or scale the affine linear
terms. Attachment/revision/selection capture, inherited locks, Edit permission,
plain Text style limits, normal validation and one-step history all apply.
The isolated host adapter checks the final derived candidate too: the requested
box, local pose and world pose must survive layout derivation. Controlled Text
size/position or an ancestor relocation that overwrites them rejects atomically.
Other valid derived changes can remain in the same transaction. This is a fixed
box mode, not an override of responsive size ownership or a new auto-height tool.

Local wrapping and ellipsis use the new content width while the font stays fixed.
Existing Text rendering, resources and measurement invalidation remain responsible
for those pixels. Existing affine Text poses still require the matching font
bridge/capability; native positive-uniform Text box resize does not need it.
Supported styles are the same bounded plain solid/effect-free subset as the
[Text transform tools](text-transform-tools.md); unsupported targets retain their
normal transform behavior.

The pinned Leafer [resize implementation](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/resize/src/resize.ts)
distinguishes intrinsic size, transform-only and Text font-size changes. This
slice adds the explicit fixed Text-box variant alongside Flow's old transform
mode. It does not implement font-size dragging, auto-size alignment rules,
general per-component intrinsic resize, rich text or WYSIWYG editing.


## Validation

Twelve new regressions cover all eight handles through affine poses, opposite
anchors, Shift ratio, exact no-op, padding floors, finite/size budgets, typed
command safety, stale external edits, plugin replacement, one-step Undo/Redo,
native-only/default-mode compatibility and responsive-layout ownership rollback.
Affected checks: **301 passed, zero failed or ignored** (12 editor, 259 interaction,
30 filtered UI Text checks). Strict all-targets Clippy for editor/interaction/UI,
example Clippy and actual native linking passed. These counts are not additive
with earlier slices, and the full UI suite was not rerun for this interaction change.

The `text_box_resize --trace` window ran 2026-10-02 13:30:28–13:34:30 UTC and
closed normally with exit 0. Real pointer checks covered width shrink and reflow
with end ellipsis, increased height revealing more lines, left-edge opposite
anchoring, Shift-preserved box ratio, one-step Undo/Redo, affine-parent width
resizing, default transform-mode compatibility and native-only Text with the font
bridge withdrawn. The font stayed 23 and intrinsic mode preserved the affine
linear terms. With the bridge withdrawn, the separate affine fixture explicitly
reported its missing backend, as designed. Read-only removed resize handles;
a background drag panned the viewport but left the Text model unchanged.
The Undo/Redo artwork region was pixel-identical. The trace had only expected
transform/box metadata (1,288 lines), with no program stdout. No IME, rich-text,
held-pointer capability revocation or cross-platform native coverage is claimed.

Production source was unchanged during acceptance. Exact source commit
`38d1aea4cdb80bf7e59aec321a0102c1365a3720`, tree
`7e62d030642790113ce8906c3825e4581adf6dad`; archive SHA-256
`fc2cc81361f47acda015a001ccf3cc2048339ccc83701342a4ccd9f15a442a40`;
binary SHA-256
`13d17e58ec5a5ec2411805b2a2264fa1544bdf36efd05a4e29ac2391f1bfe18b`.
Screenshots, command logs, trace and normal close receipt are retained with the
validation artifacts. Reproduce with
`cargo run --locked -p flow-studio --bin text_box_resize`.
