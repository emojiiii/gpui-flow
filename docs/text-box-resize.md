# Intrinsic fixed Text-box resize

Status: recoverable source checkpoint. Four geometry and eight interaction
regressions plus strict affected/example Clippy pass; broader targeted checks and
native acceptance are pending.

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
