# Bounded grayscale

This document defines the implemented scalar grayscale increment. APP-04 remains partial. It does not replace the task ledger's mandatory acceptance or claim a native/cross-platform validation result.

## Model and compatibility

Flow adds `Style::grayscale: f32`, a finite scalar in `[0, 1]`, default `0`. Native Flow JSON always serializes the scalar, including zero. Older documents that omit it read as zero. This is missing-field reader compatibility; old binaries are not promised to display or preserve grayscale, and serialized old/new document bytes need not be identical. Negative zero is a no-op. Invalid values and nonnumeric forms reject through normal atomic Scene/Engine/editor entry points, including retained hidden state. `PropertyKey::Grayscale` exposes `style.grayscale`, Number min 0/max 1/default 0, with PAINT-only invalidation.

This Flow scalar contract is deliberately narrower than Leafer's declared number/object union. The pinned upstream examined for this work declares grayscale but does not install an executed grayscale processor in the inspected effect/filter paths. No exact Leafer grayscale rendering or numeric-validation parity is claimed. Object-form effects, a generic filter graph and automatic Leafer grayscale interchange remain unsupported.

## Pixel rule and composition

For premultiplied sRGB-encoded bytes `(R,G,B,A)`, compute in f64:

`Y = (2126R + 7152G + 722B) / 10000`

Each RGB channel becomes `round(channel + amount * (Y - channel))`, defensively clamped to `[0,A]`; A is unchanged. There is no gamma decoding and no 8-bit straight-alpha roundtrip. This homogeneous matrix commutes with alpha and preserves transparent black, gray and white. The existing final GPUI channel conversion and low-alpha precision limitation remain unchanged.

Positive grayscale isolates the node's complete result. Own outer shadows, own paints, child effects/masks/erasers, inner shadows and strokes retain their existing phases. Grayscale runs once after these contents and before the node's opacity, effective blend/Pixel erasure and inherited clip. Own shadow-source capture excludes own grayscale, while each descendant's completed grayscale is replayed whenever the ancestor renders that descendant for a shadow source. Parent grayscale includes descendants' finished effects. Export backgrounds and OwnedCanvas backgrounds/grids remain separately owned and keep their color.

A new full-opacity PassThrough Group/Box/Frame isolation rejects with `UnsupportedGrayscaleBackdrop(id)` if descendants need the external artistic backdrop it would sever, including inside OwnedCanvas. Explicit Normal, existing opacity isolation, direct eraser-child boundaries and translucent mask runs preserve their current safety rules. Pixel eraser containers keep their existing transparent-source semantics. Pixel grayscale leaves erasure alpha unchanged. Path erasers retain the geometry-only early branch and prepare grayscale as zero, with no grayscale work. Alpha masks extract alpha only; grayscale never converts a mask into a luminance mask or creates a new mask-run flush boundary.

## Bounds, damage and publication

Grayscale has no spread. Geometry, ItemBox, effect extents, clips, selection/hit bounds, culling and layout sizing stay unchanged. The engine extends existing effect-ancestor paint damage propagation to active grayscale, so descendant paint edits invalidate a fixed-size grayscale ancestor even when geometry is identical. This changes appearance invalidation, not sizing.

Domain snapshots include the scalar. Placement equality conservatively treats any grayscale amount change as incompatible, withdrawing old accepted images and rejecting superseded work. Regression coverage includes domain appearance/disappearance, positive amount changes in subtree and OwnedCanvas, current-upload acceptance and undo restoring original pixels. StatePaint retains base grayscale without adding a new state-style API.

## Consumers and bounds on work

SceneView promotes active grayscale to a complete CPU domain. Missing Text bridges, unavailable images, unsupported Custom and budget/backdrop failures reject the domain. Direct native RenderObject/DisplayElement reports rejecting `CompositingDomain`. Canvas subtree draw and PNG/lossless WebP/JPEG export reuse the CPU path, retaining immutable snapshots and atomic failure. Mixed SVG rejects active visible grayscale when planning, before host resource preparation. Its existing hidden/zero-opacity/non-mask skip policy remains. Strict Leafer JSON rejects grayscale input as an unsupported leftover and rejects nonzero retained state at both upfront admission and node export, including hidden nodes; zero exports by omission.

The in-place pass allocates no pixel scratch. It charges one current-surface pixel-work unit per pixel before touching the layer and checks cancellation at each row. The new isolation uses the existing RGBA layer and normal composite charge. Pre-allocation admission replays the same sequence and every descendant pass repeated by ancestor shadow sources. The existing limits remain: 4096 pixels per axis, 4,194,304 pixels, 128 MiB explicitly tracked live pixels, 32 million cumulative work units and 16 levels, plus existing input/geometry limits. No cache, dependency, downscale or tiling is introduced. Private backend, GPUI cache and GPU allocations retain their existing exclusions from process-memory guarantees.

## Evidence boundary

The sibling validation directory preserves focused and full-check logs and exact source/build-input manifests. The first consumer run failed because the new Canvas budget test configured a work limit below the initial surface allocation; the test now admits initial allocation and checks the intended transactional draw rejection. The failed log is retained. Two intermediate strict-check failures are also retained: the current toolchain required fixed-array chunk iteration, and the first test conversion needed explicit array dereferences. Final-source checks passed after both corrections. Strict Clippy subsequently required constant RGBA chunks to use the current toolchain’s array-chunk API; the lint failure and an intermediate test-reference type-check failure are also retained, followed by corrected-source reruns.

Native window QA and integration fixture receipts are separate gates. This source contract does not assert a native pass. Remaining APP-04 work includes blur/backgroundBlur, spread/box/scale-fixed shadow behavior, and broader upstream filter semantics. Luminance-mask semantics remain outside this scalar appearance effect; grayscale on alpha-mask nodes is supported and preserves extracted alpha coverage.

## Verified source checks

The final 505 source/build inputs have aggregate SHA-256 `6f3337234d8554bd8206ac95095e850aca6fbc8eb4a01c58690f7bfbe99a744d`. The [validation summary](validation/2026-10-03-grayscale-source-summary.json) records 1,701 headless tests/doctests with 10 existing ignored, 279 UI tests, strict all-targets checks for both existing feature graphs, and formatting. No native run is inferred.
