# Canvas physical pixel exchange and PNG bytes

Status: recoverable source checkpoint. Focused pixel/codec/adapter tests passed;
final affected checks and native round-trip preview are pending.

## API and metrics

`CanvasPixels` is owned tightly packed straight-alpha RGBA8 with immutable
physical width/height. `from_rgba` validates dimensions, exact byte length and
budgets; `rgba_mut` exposes only the fixed-size channel slice.

`CanvasSurface::read_pixels(x, y, width, height)` reads physical pixels, padding
out-of-bounds areas with transparent black. Origins are signed, extents are
positive unsigned integers. Zero/oversized extents reject; there is no browser
negative-width normalization or dirty-region overload in this bounded API.
`write_pixels(&pixels, x, y)` clips to the surface and replaces RGBA, including
alpha-zero clears. It is not source-over drawing. A wholly outside write is a
no-op that preserves the generation and Arc snapshot.

These coordinates do not use logical Canvas size, DPR, smoothing, object/world
transforms, scene opacity or corner masks. A 100x50 Canvas at DPR 2 owns 200x100
physical pixels. Existing fill_rect/draw/draw_image keep their logical metrics.

All successful writes atomically publish an immutable premultiplied snapshot;
errors leave current pixels/generation unchanged. SceneView's existing
edit_canvas closure remains an atomic multi-operation transaction, refreshes the
upload generation, and does not enter document Undo. Detached/destroyed resources
reject fresh access; an already owned immutable snapshot remains usable.

Straight/premultiplied conversion reuses tiny-skia's ColorU8 operations. Hidden
RGB under zero alpha is discarded. Low-alpha color precision cannot be recovered
from an 8-bit premultiplied surface; readback returns its representable straight
value. A read/write round trip preserves stored premultiplied bytes.

## Raw PNG export

`CanvasSurface::encode_png()` and immutable `StaticImage::encode_png(limits)`
return complete PNG bytes. No path, upload, browser, URL or filesystem side effect
occurs in the core API. The PNG contains raw surface content at physical extent,
not the retained Canvas fill/stroke/corners, world pose, ancestor clips/effects,
or an entire SceneView screenshot. The host decides where to deliver the bytes.

The implementation reuses the already locked png 0.17.16 codec and streams one
demultiplied RGBA row at a time, with a 16KiB output chunk, rather than allocating
a second complete straight-RGBA image. Source dimensions/pixels and work are
admitted before encoding. A conservative 8MiB codec/scratch allowance is reserved
from max_surface_bytes. The output writer is bounded to the remaining memory
allowance and at most `2 * source_bytes + 65536`; failure returns no partial PNG.
Pixel interchange accounts for source/input/candidate buffers using the existing
limits. External snapshot owners and GPUI's private atlas remain outside known
adapter-owned byte accounting as before.

The native `canvas_pixels` demo writes only after a successful core export, into
FLOW_CANVAS_EXPORT_DIR or a dedicated temporary directory. It creates a new file
and loads it through the existing GPUI image path. This is a demonstration host
policy, not an automatic core save or arbitrary filesystem permission.

## Pinned alignment and boundaries

The pinned [Canvas component](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Canvas.ts)
exposes its owned context. The core
[ICanvas ImageData contract](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts)
includes getImageData/putImageData. Its
[canvas base](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/canvas/canvas/src/LeaferCanvasBase.ts)
separates physical pixels from logical dimensions and exposes export hooks, whose
base implementations are stubs for plugins/platforms. This independent bounded
Rust implementation fills a useful owned-surface contract; it does not claim
upstream export-plugin parity.

No browser CanvasRenderingContext2D, dirty subrect overload, color-space options,
JPEG/WebP/SVG, data URLs, arbitrary file dialogs, asynchronous export, whole-scene
trim/scale/screenshot, or persistence in scene JSON/history is introduced.

## Validation checkpoint

Nine new pure pixel/PNG regressions and two new resource-adapter tests pass.
Coverage includes physical DPR, alpha replacement, negative/offscreen coordinates,
zero/invalid/budget rejection, immutable snapshots, 65536 channel/alpha round-trip
samples, PNG decode comparison, raw corners, output/work/memory failure, atomic
adapter rollback and retained snapshots after destruction. Existing stale-CPU-job
coverage now exercises write_pixels directly. Final counts/native evidence follow.
