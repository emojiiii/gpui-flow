# Capability-aware Text transform handles

Status: recoverable source checkpoint. Nine new interaction regressions and
strict affected/example Clippy pass; broad regression and native acceptance are
pending.

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
intrinsic Text width/font-size rewriting, reflow drag tools, custom pivots, skew
handles or WYSIWYG text editing. Existing content-panel sessions still suspend
outer tools. [Affine glyph rendering](text-affine.md) is the separate prerequisite.
