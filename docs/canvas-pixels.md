# Canvas physical pixel exchange and PNG bytes

Status: bounded Linux tests, native interaction and actual PNG readback passed
on 2026-10-02. This is owned-surface export, not a whole-scene export system.

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

The implementation uses the locked png codec and streams one
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

## Validation (2026-10-02)

- Eleven new regressions: nine pure pixel/PNG tests and two resource-adapter
  tests. Existing late-CPU-success coverage now uses write_pixels directly
- 338 affected library tests passed: 209 composite and 129 UI, zero failures or
  ignored. Earlier focused runs overlap and are not additive. Unchanged model
  and editor suites were not rerun wholesale
- Coverage includes physical DPR, alpha replacement, negative/offscreen origins,
  zero/invalid/budget rejection, immutable snapshots, 65536 channel/alpha
  round-trip samples, PNG decode comparison, raw corners, output/work/memory
  failures, adapter rollback and retained snapshots after destruction
- Strict affected all-targets and example Clippy, formatting, locked check and
  real one-job native link passed. PNG 0.17.16 was already present in Cargo.lock;
  no package version was changed
- Native cloud window ran 10:25:49–10:27:43 UTC and exited 0 with empty stderr.
  Verified real Canvas pattern, physical read/invert/write, rejected multi-write
  batch, PNG export loaded through GPUI, Clear with frozen preview, transparent
  re-export and repeated initial-pattern recovery. Final inventory confirmed
  normal closure; screenshots remain in the native tool record
- Four actual native-generated PNG files were independently decoded/read back.
  Each is RGBA 480x320 for the logical 240x160/DPR-2 Canvas. Alpha samples were
  0, 160 and 255. The inverted 80x80 physical region appeared at the requested
  destination, the clear export had zero alpha everywhere, and raw corner pixels
  remained present despite the retained Canvas mask. Low-alpha straight color
  130 becoming 131 is expected premultiplication quantization, not a promise of
  lossless recovery of arbitrary original straight channels
- Recreating the initial pattern produced byte-identical PNG files (8296 bytes,
  SHA-256 `7891c8de56755c88a7ac5b481dcac4e23fe3b0774eb720f32925527f7754e21a`).
  The inverted export was 8437 bytes with SHA-256
  `bdda2c75ae0562f4cee186269428925d0ed80bec87e65d8da14658655a739ecd`;
  transparent export was 4233 bytes with SHA-256
  `a904e88507ffff803f07bbfdf8ea903ef288ef9511503560e4963231a32dd784`

Exact tested source tree: `983b01ac15209d3ee22ea5fa47037dd3af707465`.
Deterministic source archive SHA-256:
`3f9bc3d23bd98e47a116bff7bf3189199ef4742c514fb5242c4754791cc27a2e`.
Linux example binary SHA-256:
`e5c4e0094c46bc2369995520d44184ee54a3c9cafaa9cff719a3b3deab6c7905`.
Only documentation changed after these checks and the native run.

Run `cargo run -p flow-studio --bin canvas_pixels`. The core API returns bytes;
only this demo saves new PNG files to its explicitly selected temporary directory.
Cross-platform codec/GPU behavior, complete browser contexts and whole-scene
export remain outside this acceptance.
