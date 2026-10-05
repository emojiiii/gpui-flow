# Bounded foreground blur

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

Scalar foreground blur is part of the fixed APP-04 effect set alongside [shadow modes](shadow-modes.md), [owned background blur](background-blur.md) and [grayscale](grayscale.md). The contract below describes the supported pipeline and explicit rejection domains; it makes no browser-pixel or arbitrary host-backdrop claim.

## Model and mapping

`Style.blur: f32` defaults to zero and accepts only finite values in `[0,256]`. Missing old fields read as zero; native Flow JSON writes the field, including zero. This does not promise old binaries preserve the new field, identical old/new JSON bytes, or Rust struct-literal compatibility. `Shadow.blur` retains its separate existing meaning. Property metadata exposes a numeric paint-only `style.blur`; normal Scene/engine/editor admission validates invalid values even in hidden or zero-opacity state. Zero and negative zero follow the unchanged foreground-free raster path.

An active node requires its complete effective physical transform to be a supported positive similarity: translation, rotation and uniform positive scale. Reflection, singular/nonuniform/skew transforms reject for that filtered node; the existing numerical similarity tolerance remains. Unfiltered descendants retain their existing supported affines. The physical standard deviation is the scalar blur multiplied by this similarity scale, including output/device scaling. Positive sigma must lie in `[0.001,64]`; it is not the existing shadow half-radius mapping. Parameter admission does not promise every source at an admitted sigma fits the unchanged work/size caps.

The renderer filters all four **premultiplied encoded-sRGB** byte channels with the locked image0.25.10 separable Gaussian. It does not gamma-decode or roundtrip through 8-bit straight-alpha. Output RGB is defensively limited to alpha and transparent output remains black. Positive blur, including a mathematically one-tap kernel, still owns a complete isolated source and its allocation/work accounting.

### Exact one-tap correction

At very small positive sigma, image0.25.10's adjacent f32 Gaussian weights can underflow to exactly zero. The dependency trims this to one tap while its ring row storage retains a one-pixel pad; it then samples the wrong horizontal position and can omit the first destination row. Foreground blur reproduces the dependency's adjacent f32 weight expression and returns the original source only when that value is exactly zero. The mathematically correct byte result in this range is identity. There is no broad threshold, sigma clamp or modified nonzero kernel. Adjacent representable f32 values on both sides of the actual transition distinguish this branch from the unmodified dependency. This is a narrow correctness deviation, not a claim to preserve image's buggy output for all sigma or to patch the dependency generally.

The later [shadow-mode implementation](shadow-modes.md) applies the same exact one-tap correction to A8 shadows, including shadows sampled inside foreground blur.

## Complete source and phase ownership

A node's source includes its own outer shadows, paints, completed descendants, mask/eraser results, inner shadows and strokes in the existing explicit container phases. Foreground blur runs once after that complete result, then this node's grayscale, then local opacity, effective blend/Pixel erasure and inherited clip. This avoids per-child opacity and filtering already-clipped destination pixels. Own shadow-source capture excludes this node's own shadows and filters while still rendering each descendant's completed effects; each ancestor shadow can therefore repeat descendant blur work. Child clips are applied while composing child contributions, before the parent's own blur. An inherited clip is applied once when the finished filtered result enters its destination.

Unsafe full-opacity PassThrough container isolation rejects with `UnsupportedBlurBackdrop` when descendants need an external artistic backdrop that the new filter boundary would sever. This also applies inside OwnedCanvas. Existing Normal/opacity/mask/eraser isolation rules remain. No unknown host pixels are borrowed or silently replaced by an unfiltered result.

Alpha-mask blur softens alpha coverage, not luminance. Pixel erasure uses the blurred source alpha within its owning parent. Path erasure retains its geometry-only branch, ignores blur/grayscale for erasure and allocates no such filter work, while retained model input is still validated. Geometry/layout/hit/selection do not grow to the halo. Image, immutable Canvas and host-provided Text ink use the same CPU source bridge; missing/unsupported producers and Custom reject the whole required domain.

## Physical extents, clipping and invalidation

Every blur-bearing domain receives a bottom-up device-space plan on one absolute physical-pixel lattice. Each node's finished support includes descendant filters, own shadow projections and separate device guards before it is consumed by an ancestor. Filtering uses the **entire** finite source with transparent padding. Offscreen source ink that contributes a visible halo remains present; final viewport/content-mask crop occurs only after filtering. A huge mostly offscreen source may reject under the same limits rather than being cropped before convolution. Viewport/content-mask crops under the same physical transform share the lattice and do not change the source filter origin. Arbitrary export-origin changes may change pixel phase; the export crop oracle compares origins separated by an integer physical offset.

Source and destination transforms compose camera, world transform, DPR and integer surface origin before the final f32 narrowing. This preserves padding for large cancelling world/camera translations. Prepared shadow records survive a pre-filter empty legacy extent so a child's halo entering an empty Frame can still cast that Frame's own effects. These are fixes found during independent source review.

Engine analytic effect bounds remain distinct from raster device support. Geometry and ItemBox stay unchanged. `ChangeSet.full_paint_invalidation` conservatively invalidates the whole scene/owning domains for any visual modification when either the old or new scene contains positive blur; DATA-only and no-op changes are exceptions. This is deliberately wider than only blur-ancestor damage because a finite world-space rectangle cannot encode all device guards. It does not claim GPUI performs partial GPU rasterization. Domain source/placement identity includes the scalar and dependencies; incompatible amount/source/viewport/resource edits withdraw stale accepted images and old completions cannot overwrite the current result. Full-source Text preparation propagates through an active blur ancestor so offscreen glyph ink is not discarded before filtering.

## Work, memory and failure atomicity

No global limit is raised: default 4096 pixels per axis, 4,194,304 pixels per surface, 128 MiB known working set, 32 million cumulative work units and depth16, plus existing input/producer limits. Positive-sigma range is only one admission condition. Highly nested effects, repeated ancestor-shadow captures or large full-source padding can exceed the existing budget.

For the admitted sigma, let H=`ceil(4*sigma)+2`, K=`2*H+1`, P=the padded source pixel count and W=its width. The conservative four-channel filter allowance is `56P + 32WK + 32W + 128K` bytes and `P*(16K+96) + 64K²` work units. It covers byte source/output, f32 planes, ring or full-plane intermediates, padding, kernel copies and reference tables. These are bounded accounting estimates, not total process memory or measured wall time; backend-private, GPU, allocator and operating-system overhead retain their existing exclusions.

The padded blur layer replaces that node's ordinary isolation layer. The runtime reserves its filter allowance while recursively composing source contents, then charges source-sized grayscale separately from destination-sized offset composition. The no-pixel admission traversal uses the same source plan, depth and complete replay schedule before the output's first pixel allocation. Exact-cap/cap-minus-one tests exercise both ledgers. A failure discards the entire transaction and restores retained-byte accounting; no partial Canvas snapshot or partial export is published. The dependency filter is bounded but non-interruptible internally; cancellation is checked before/after it and at surrounding row loops, never claimed at every private convolution tap.

## Consumers and evidence

SceneView promotes active blur to a CPU domain. Direct lower-level native painting rejects it as a compositing-domain feature. PNG, lossless WebP, JPEG and Canvas subtree operations reuse the bounded CPU path with their existing output/background semantics. Mixed SVG rejects active visible blur before resource preparation; strict Leafer interchange rejects nonzero blur including retained hidden state. Existing explicit hidden/zero-opacity consumer policies remain separate from model admission.

Final source receipts must distinguish independent scalar convolution, source/crop and effect-order oracles, unchanged-path regressions, real CPU producer/consumer output, model/history tests, UI controller generation tests and physical observations. Native JPEGs establish visibility only; public `presented_geometry` may take a canonical fast path and does not expose private CPU upload generations. The physical fixture is a separate gate with fresh source/binary hashes and exclusive session evidence. The task ledger accounts for the complete fixed effect set.


## Pinned Leafer reference boundary

The inspected [Blur.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/effect/src/Blur.ts) sets a filter from blur multiplied by the current world matrix's `a` component; [LeaferCanvasBase](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/canvas/canvas/src/LeaferCanvasBase.ts) applies pixelRatio and a CSS blur filter. This motivates a physical Gaussian mapping, but Flow deliberately uses positive-similarity magnitude so supported rotation does not turn the radius negative or zero. The finite scalar ranges, encoded-premultiplied CPU rule, explicit phase order and rejection domains above are Flow's bounded contract.

Registration alone does not establish execution. At the pinned UI/core/plugin revisions the inspected UIRender, LeafRender, BranchRender, helper and filter paths did not establish a caller executing Effect.blur; the upstream [BackgroundBlur.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/effect/src/BackgroundBlur.ts) is empty. Consequently this increment does not claim executed upstream effect-order or byte parity. The original task still requires owned-background blur; the [owned-prefix implementation](background-blur.md) supplies that Flow capability.


## Validation

Independent regressions cover scalar convolution, one-tap identity, full-source
crops, shadow/filter ordering, offscreen sources and cancelling camera/world
translations, exact budgets and failure atomicity. The source and UI suites
passed strict checks. The [12-row Linux native check](foreground-blur-native.md)
completed and exited normally. Its frozen binary predates the later A8 shadow
correction; that historical result is not retroactively changed. Run the
`foreground_blur` Studio example for the interactive cases.
