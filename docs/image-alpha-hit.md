# Runtime Image alpha picking

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

`SceneView::set_image_alpha_hit(handle, Some(ImageAlphaHit { .. }), cx)` opts one
attached Image instance into source-alpha filtering. `None` restores the existing
geometric behavior. `image_alpha_hit(handle)` reads the current attachment's
setting. This is host runtime configuration; it never changes scene JSON, an
Image shape, selection, or Undo history.

```rust,ignore
use gpui_flow::{ImageAlphaHit, ImageHitFallback};
view.set_image_alpha_hit(image_handle, Some(ImageAlphaHit {
    threshold: 0,
    pending: ImageHitFallback::PassThrough,
    failed: ImageHitFallback::Geometry,
}), cx);
```

The threshold is a byte: a sample hits exactly when `alpha > threshold`.
Threshold 0 accepts every nontransparent source pixel; 255 accepts none.
Pending and failed/cancelled/denied/unavailable resources have separate explicit
`Geometry` or `PassThrough` fallback choices. Both defaults are Geometry so a
loading/error placeholder remains reachable. PassThrough checks the next engine
candidate in painted front-to-back order, rather than sending every hole to the
canvas background.

## Geometry and sampling contract

The engine supplies ordered candidates and continues to enforce inherited
visibility/opacity, locks, hit policy, custom picking, singular transforms and
exact ancestor clip containment. Runtime alpha opt-ins with a current, valid
decoded sample use `hit_test_all_with_image_candidates`: only those explicit Image
IDs may additionally use their rounded outer box. This prevents stale
`natural_size` metadata from rejecting visible decoded pixels before sampling.
The default `hit_test_all` path is unchanged. Pending, failed, cancelled or invalid
frames do not receive expanded candidates: Geometry fallback retains the original
metadata-based hit shape. See [source crop](image-crop.md) for shared fit mapping.

For an opted-in Image, its own rounded silhouette is an additional strict gate,
even if its document hit policy is Bounds or pointer tolerance is nonzero.
Actual decoded frame dimensions determine the centered Contain/Cover/Fill
mapping, matching the dimensions used by native painting and the CPU image
compositor. Contain letterbox and Cover's cropped source region are respected.

Sampling is nearest decoded physical source pixel, not GPU bilinear coverage.
Pixel centers are `(x + 0.5, y + 0.5)`; inverse-fit coordinates are floored, and
an exact nearest-center tie chooses the higher pixel index. Source and layout
rectangles have half-open right/bottom edges. The engine-provided local point
has already passed through the world inverse affine transform; viewport zoom
and window DPR are not multiplied into this source coordinate again. BGRA's
fourth byte is the alpha channel. No image bytes are copied or cached for hits.

The sampled frame index is the resource playback index shared with native and
CPU render requests, including pause and seek. This is not a GPU presentation
fence: an asynchronous CPU domain can temporarily display its previous uploaded
frame while a newer playback frame is being prepared. Picking follows current
resource playback, not a readback of that old composited texture.

## Lifecycle and reuse

The adapter reuses `SceneResourceAdapter::composite_image`, which validates the
canonical source, live handle, attachment epoch and resource generation and
returns the same existing decoded GPUI `RenderImage` and selected frame.
It performs no download, decoding, asset load, new alpha raster, polling, or
independent image cache. Unknown/invalid decoded frames use the failed policy.

The opt-in is keyed by engine-scoped handle plus attachment epoch. Detach,
destroy, document replacement and remount cannot inherit it, including ID reuse.
Source replacement preserves the current attachment's policy but immediately
rejects previous pixels, including A→B→A edits before the next prepare pass.
Resolver/cache invalidation likewise cannot expose the old loaded generation.
Reattaching the same handle requires the host to install a policy again.

## Scope and reference

The pinned Leafer [`UIHit.ts`](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/hit/src/UIHit.ts)
separately enables pixel fill/stroke testing and renders a bounded hit canvas.
This Flow slice implements only runtime Image source-alpha picking using its
existing resource/render infrastructure. It does not implement that entire hit
canvas, pixel stroke picking, Canvas pixel picking, effects/shadow hits, final
composite/sibling-mask alpha, configurable hit-radius sampling, or a general-purpose photo editor. The separate [fixed source-crop editor](image-crop.md)
reuses the same mapping. Pure `Scene`/`SceneEngine` picking remains geometric.

## Regression coverage

`cargo test --locked -p gpui-flow image_hit --lib` exercises pure fit/byte sampling
and actual resource-ticket/runtime candidate filtering. Cases cover Contain,
Cover, Fill, half-open edges, corners, invalid buffers, transparent holes and
threshold equality, front-to-back clickthrough, pending/failure/cancellation,
animation frame/pause/seek, source A→B→A, authorization revocation, destruction,
remount, document reset, nested clips, affine reflection/skew/rotation, zoom/DPR,
and unchanged document revision/serialization/Undo.

Native window validation and aggregate verification are reported separately;
these focused regressions do not imply full Leafer Image/editor parity.


## Verified Linux checkpoint

The 18 new alpha-hit tests pass. The complete affected UI library passes 171
tests, zero failed/ignored; strict UI all-targets and example Clippy, example check
and actual native linking pass. This is not a rerun of unrelated workspace crates.

The frozen `image_alpha_hit` window ran 2026-10-02 14:55:16–15:00:42 UTC and
closed normally with exit 0. Real clicks verified transparent clickthrough to the
underlay, acceptance at alpha 128 above threshold zero, rejection at threshold
128 equality, opaque acceptance, default geometric compatibility, current-frame
alpha changes after seek, pending pass-through versus geometric fallback, failed
source pass-through, affine Cover mapping, rounded/ancestor clip rejection, and
opt-in expiration/reinstallation after remount. Pointer target and selection
readouts agreed. The stderr log contained exactly 13 expected pointer-down target
lines; stdout was empty. Fill, zoom/DPR, threshold 127/255, repeated A→B→A and
invalid-frame permutations have automated coverage, not separate native claims
from this window. No crop editor or final composited-alpha picking is implied.

Exact source checkpoint `17f92d2fec9b136027eceb3c8d8a063fc84cb013`, tree
`ed04add17394263c367cd081d91ff6a214d8a8b0`; source archive SHA-256
`31129f56edb7747fbcace061f25d4f8b464832a94cb6d09e52a547c5dfb8caff`;
binary SHA-256
`05e4ddb3b85c1fd302e79a9e8508d20f753bc8b22010087e750cc7a0c1bcd2eb`.
The prebuilt executable was frozen before parallel query-index/font-resize work;
those later sources are not covered by this native receipt. Screenshots, target
trace and normal close receipt remain with the validation artifacts. Reproduce:
`cargo run --locked -p flow-studio --bin image_alpha_hit`.
