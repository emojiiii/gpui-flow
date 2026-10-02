# Canvas component and owned runtime pixels

This slice implements a distinct retained `display::Canvas`, not an
Image alias or the existing whole-SceneView `OwnedCanvas` compositing mode.
It is a bounded implementation, not a completed Leafer parity claim.

The pinned [Leafer Canvas component](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Canvas.ts)
owns a canvas/context, draws images/subtrees, resizes and destroys its resource.
The pinned [canvas base](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/canvas/canvas/src/LeaferCanvasBase.ts)
preserves physical pixels at the top left during safe resize. This slice reuses
locked tiny-skia, Kurbo, the existing compositor, resource tickets and GPUI image
upload; no upstream implementation or new dependency is copied.

## Source contract

- Independent `flow-scene/src/display/canvas.rs`: logical size, DPR, smoothing,
  safe-resize and scalar/independent corners. Defaults are 100x100, DPR 1,
  smooth true and safe-resize false. It is a leaf and factory kind `canvas`.
- `CanvasSurface` in flow-composite owns immutable pixel snapshots and atomic
  `clear`, `fill_rect`, `draw_image` and captured-subtree `draw` operations.
  `capture_subtree` and `draw` remove the source root world transform before the
  supplied local transform, exclude ancestor clips/opacity, and preserve the
  actual subtree/effects. Unsupported input fails without publishing partial ink.
- Logical dimensions are finite and at least 1e-6; DPR is finite in [1e-6,8]. Physical dimensions ceil
  logical dimensions times DPR, at most 4096 per axis and 4194304 pixels. Logical
  drawing scales to the allocated extent. This intentionally rejects zero sizes
  instead of upstream's zero-to-one fallback. Existing compositor limits apply.
- Runtime surface bytes stay outside Scene JSON/history. Config edits use the
  normal Shape transaction and Undo; imperative pixel edits do not become Undo
  entries. Loading/importing/cloning a document creates a fresh transparent
  runtime surface, rather than serializing or secretly sharing its pixels.
- `SceneResourceAdapter::prepare_canvases` initializes surfaces without a Window.
  `SceneView::edit_canvas` applies a pixel transaction and requests repaint.
  Handle/attachment/config tickets plus content generation prevent stale jobs.
  Runtime ownership lives in `flow-ui/src/resources/canvas.rs`; detached,
  destroyed or retyped nodes drop owned references at the engine change boundary.
- A new size/DPR clears by default. Safe-resize preserves only the physical
  top-left intersection, without scaling. Intermediate committed safe shrinks
  still remove cropped pixels even if the view did not render between commits.
  Shape corner/smooth-only updates preserve content. Retained surface + upload
  buffers are capped at 64 MiB per adapter, with 64 entries and candidate admission.
  Capacity failures are explicit resource failures; `retry_canvases` requests fresh
  tickets after a host frees capacity, without discarding existing live content.
- SceneView uses the real CPU domain for Canvas content, fills, strokes, affine
  transforms, opacity, inherited clips and supported effects. Standalone native
  DisplayElement reports the required compositing path instead of painting an
  empty rectangle. CPU content uses nearest/bilinear filtering from `smooth`.

This is a bounded explicit drawing API, not a browser CanvasRenderingContext2D
replacement. No arbitrary JS context, automatic URL/base64 loading, export,
recording, arbitrary filters or content persistence is claimed. Hosts provide
already-authorized decoded pixels; external snapshot owners and GPUI's private
GPU atlas remain outside the adapter's explicit CPU-byte accounting. Picking uses
the component rectangle/corners and stroke geometry, not runtime alpha sampling.

## Validation

Fresh Linux/Rust 1.98.1 local validation on 2026-10-02:

- 24 new regressions: three scene, two engine, one editor, two layout, nine CPU
  surface/compositor and seven native-adapter tests.
- Actual coverage includes default JSON/no pixel persistence, finite/DPR/pixel
  limits, factory rollback, layout grow/ratio and oversized derived rejection,
  config Undo/Redo/cancellation, decoded-image filtering, fill/content/stroke
  ordering, independent corners, normalized subtree drawing, failed-draw rollback,
  memory/work admission, intermediate safe shrink, unsafe A-B-A reset, immediate
  detach/destroy/retype/disposal, per-view ownership, and stale asynchronous CPU
  success rejected after a pixel-only edit without a document revision change.
- Affected native UI/compositor libraries: 298 tests pass. Selected scene, engine
  resources, editor and layout integration tests: 58 pass. Total 356, zero failures
  and zero ignored for these commands; this is not a rerun of every workspace test.
  After the final allocation-admission cleanup, the 44 Canvas-focused native/CPU
  tests and affected Clippy were repeated successfully.
- Strict all-target Clippy passes for the six directly affected packages. The
  `canvas_element` Studio example compiles and links with the real locked GPUI.
  Workspace rustfmt and `git diff --check` pass.

The native example is prepared for a bounded interactive check; live-window
results are still pending. No Actions, deployment, merge or cross-platform run is
launched.
