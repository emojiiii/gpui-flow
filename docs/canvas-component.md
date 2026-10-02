# Canvas component and owned runtime pixels (WIP)

This source checkpoint implements a distinct retained `display::Canvas`, not an
Image alias or the existing whole-SceneView `OwnedCanvas` compositing mode.
Validation is still in progress; it must not be read as a completed parity claim.

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
- Dimensions are positive; DPR is finite in (0,8]. Physical dimensions ceil
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

## Checkpoint validation

The model/compositor/UI library compile against the real locked dependencies.
New regression tests, final focused lint and native example verification remain
pending. No Actions, deployment, merge or cross-platform run is launched.
