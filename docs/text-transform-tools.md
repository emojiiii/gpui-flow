# Capability-aware Text transform handles

Status: implemented and validated on Linux, with capability-gated plain Text
transform gestures.

The existing named single-selection resize, shared-root resize and center-pivot
rotation tools can now operate on supported Text when the host explicitly enables
`TransformCapabilities { affine_text: true }`. The default remains false.
`SceneView::set_text_rasterizer` enables it only after validating a matching
same-Application bridge; withdrawing the bridge disables it. Changing capabilities
cancels any active gesture through the existing capture lifecycle before replacing
the capability. An unchanged value does not cancel a live gesture.

The setting lives on ScenePluginHost and in read-only PluginContext, separate
from Edit permission. It is runtime-only and does not enter the scene, history or
JSON. A custom host uses `set_transform_capabilities` and must consume the returned
DispatchReport. Public geometry helpers retain default-compatible entry points;
new `*_with_capabilities` variants match the host's setting for paint/picking.
Existing Rust PluginContext struct literals must add `transforms: Default::default()`
to retain old behavior; repository literals were migrated. Graph contexts keep
that conservative default. No dependency was added.

The first opt-in subset is solid plain Text without stroke or effects, at most
64KiB UTF-8 content and a 512-byte font family. Text requiring other styles stays
uniform-only at native-compatible poses and has no rotation handle. Resource,
glyph and surface limits remain enforced by rendering; this is not a guarantee
that arbitrary fonts or enormous transforms can render. Custom subtrees remain
excluded. Ancestor locks, read-only policy, identities/attachment epochs, stale
external changes, command filters and final post-layout validation still gate
all typed handle commands. The capability does not grant Edit permission or
restrict otherwise trusted low-level host document APIs.

Named handles use the existing transform/frozen-root/history pipeline. Geometry,
plugins, host command checks and post-layout validation all see the same setting.
Text shape, font size and content bounds remain unchanged; Shift retains uniform
resize and snapped rotation. The old unnamed ObjectResize gesture deliberately
keeps its uniform Text/Image behavior. Completed poses are not rewritten when a
bridge is withdrawn; they show the explicit missing-backend error until capability
is restored. An active owned gesture is cancelled without overwriting intervening
external edits, using the existing stale-capture policy.

The pinned Leafer [resize module](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/resize/src/resize.ts)
distinguishes transform-only `editSize: scale`, intrinsic resizing and Text
font-size resizing. This slice is Flow's transform-only mode. It does not add
intrinsic Text width/font-size rewriting, custom pivots, skew handles or WYSIWYG
text editing. The later opt-in [fixed Text-box mode](text-box-resize.md) adds
width/height reflow while retaining this default transform mode. Existing content-panel sessions still suspend
outer tools. [Affine glyph rendering](text-affine.md) is the separate prerequisite.

## Validation

Nine new interaction regressions cover default/opt-in behavior, all eight handles
under rotation/reflection/shear with Shift, intrinsic preservation and one-step
Undo/Redo, snapped rotation, mixed Text/Image selection, per-host isolation,
read-only/forged-command rejection, unsupported Text styles, post-layout rollback
and capability revocation during resize/rotation (including intervening external
edits and retained Redo).

Affected checks: **401 passed, zero failed or ignored** (251 interaction + 150 UI).
Strict affected all-targets Clippy, example Clippy and actual native linking passed.
Production source was unchanged throughout the native runs.

The main `text_transforms --trace` window ran 2026-10-02 12:46:41–12:53:57 UTC
and exited 0. Real pointer checks covered nonuniform single Text resize, 45°
Shift-snapped rotation, affine-parent resize/rotation, Text/Image shared resize
and rotation, selected parent/child shared-root de-duplication, Undo/Redo, bridge
withdrawal/restoration, default uniform resize without the bridge, read-only
movement rejection, zoom and Space-pan. The trace contained only expected matrix
updates; the selected child's local transform stayed unchanged when its parent
root was resized. Affine rotation/Redo artwork regions were pixel-identical.

Double-click opened the content panel and hid outer handles. A focused-input
close recheck ran 12:55:59–12:57:22 UTC with the same binary, exited 0 with empty
stdout/stderr, and confirmed that clicking the textarea then Escape closes the
panel and restores handles. The first Escape sent without textarea focus was
not a verified close. Mid-capture bridge revocation and derived-layout rollback
have focused automated coverage; they were not injected during a held native
pointer drag. No IME or cross-platform input coverage is claimed.

Exact source checkpoint `938f67e38bd61cf1d54bf4b275e13c017928bd99`, tree
`07a0d2ff0640c0776264e079cc0aa86a4417ff49`; archive SHA-256
`65f110b291af80e1a7745d7f139e54872b46b8fa0182fe0e683214718f522084`;
binary SHA-256
`c74bcec5e310aeb914fb03c5cb837030690cb181541895651519f5af07b9b0e2`.
Screenshots, trace and both close receipts were retained with the validation
artifacts. Reproduce with `cargo run --locked -p flow-studio --bin text_transforms`.
