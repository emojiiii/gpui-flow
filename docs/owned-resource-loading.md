# Owned image and media loading

`ResourceLoadPool` provides shared byte and producer-slot admission.
`OwnedImageLoader` runs local image decoding in an owned subprocess;
`FfmpegBackend::with_load_pool` puts local media players under the same pool.
Every admitted job has an opaque `LoadTicket` containing pool identity, kind and
monotonic generation, and a `ResourceLoadEvent` reports its current status/error.
No runtime job or executable configuration enters Scene JSON.

## SceneView integration

Build the helper alongside the application:

```bash
cargo build --locked -p gpui-flow-media --features image-loader-worker --bin flow-image-worker
```

Construct one `SceneOwnedImageLoader::new(pool.clone(), absolute_worker_path,
ImageLoadLimits::default())`, clone it across SceneViews, and select it with
`SceneView::set_owned_image_loader`. The executable path is trusted host
configuration. The existing image resolver still authorizes each source before
the loader runs. Owned loading accepts ordinary local PNG/JPEG/GIF/WebP files;
URI, embedded, unsupported, corrupt and over-limit inputs fail explicitly.
There is no fallback to a different producer on failure.

Views requesting the same canonical path through one loader share its job and
decoded CPU image. Dropping one consumer leaves the others intact. A completion
future subscribes to a wakeup but does not keep the job alive. Dropping the last
consumer of unfinished work cancels it, kills/reaps the helper and joins its
bounded readers. Codec code need not cooperate. Loaded pixels keep their byte
reservation while borrowed results or managed CPU images remain alive. GPU
textures remain under GPUI's separate ownership.

`reload_owned_images` explicitly starts a new pool generation and engine resource
ticket for that view. Other consumers retain their old immutable result. There
is no implicit filesystem watcher. Source changes, detach, replacement, resolver
revocation and disposal release obsolete consumers during resource reconciliation.
Late wakeups only request a repaint; settlement still checks the engine's current
handle/attachment/resource generation before publishing.

`SceneResourceAdapter::image_load_event` exposes the pool ticket/status for a
current image handle. Existing Scene resource events retain their element-bound
ticket and report loading failures through the normal presentation path. Admission
failure has no pool ticket because no job was created.

## Pool and media ownership

The default pool allows 256 MiB of admitted known buffers and eight producer
slots. An image job reserves bounded input/output/conversion and metadata space
before spawning, then releases transient space after decode. Default image limits
are 16 MiB input, 16 MiB total decoded RGBA, 120 frames, dimension 4096 and ten
seconds. All frames must pass; a late frame failure rejects the whole image.
Frame delays and alpha are preserved. Limits can be configured before creating
the loader, within the API's validated ceilings.

Media retains a slot for its controller lifetime so a later Play cannot bypass
admission. Initial input/probe and bounded decoder/frame-queue capacity are
reserved before opening; transient input reservation is released after loading.
`MediaController::resource_event` and `MediaView::resource_event` expose the same
pool-generation/status protocol. Source replacement creates a new controller and
ticket. Shutdown returns after owned FFmpeg children are killed/reaped and their
reservation is released. Playback snapshots continue to carry revision and frame
sequence for seek/playback changes within that resource lifetime.

These are known-buffer and job limits, not an operating-system RAM sandbox.
Allocator overhead, codec-private memory, GPU uploads and arbitrary copies made
by host application code remain separate. Image decoders receive dimension and
allocation limits; FFmpeg keeps its existing process/format/protocol limits.

The legacy GPUI asset/cache path remains available with its external ownership
semantics. It has no new hard-cancellation guarantee. Selecting an explicit
GPUI cache or owned loader replaces the previous route; Flow never evicts an
externally owned shared cache asset.

## Reproducible checks

```bash
cargo test --locked -p gpui-flow-media --features image-loader-worker --test owned_loading --lib
cargo test --locked -p gpui-flow-media --test playback pooled_video_has_generational_events_and_reaps_before_releasing_budget -- --ignored
```

The first command covers actual PNG/GIF bytes and delays, sharing, reload identity,
late-frame rejection, errors, retirement and budget recovery. On Linux it also
suspends an actual owned decoder, then verifies that the last consumer kills and
reaps it, while a wakeup future does not keep it alive. The second command requires
installed FFmpeg tools and tests actual video lifecycle with audio disabled.

For a short native check, run `owned_loading` with an absolute worker path, a PNG
and a local media file. Inspect two shared views, failure/recovery, a left-only
reload, media creation and Release all. Use Record after a frame to read actual
resource tickets and pool counters. This does not assert other platforms or an
external host cache's cancellation behavior.

The Linux native check observed shared image generation1, a left-only reload to
generation2 while the right retained1, media Ready/Cancelled, all three pool
counters returning to zero after Release all, and recreated images at generation4.
Both functional and the subsequent example-only layout check exited normally.
