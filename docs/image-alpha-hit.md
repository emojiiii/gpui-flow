# Runtime Image alpha picking

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

The existing `SceneEngine::hit_test_all` supplies the ordered candidates and
continues to enforce inherited visibility/opacity, locks, hit policy, custom
picking, singular transforms and exact ancestor clip containment. The optional
filter cannot add a target excluded by that geometric stage. In particular,
document `natural_size` can still constrain the engine's Painted candidate box;
applications should keep that metadata consistent or omit it.

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
composite/sibling-mask alpha, configurable hit-radius sampling, or crop/inner
image editing. Pure `Scene`/`SceneEngine` picking remains geometric.

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
