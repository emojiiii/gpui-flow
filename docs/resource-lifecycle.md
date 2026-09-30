# Native text/image resource lifecycle

Status: implemented source slice, **not compiled or executed**. No test run, native
example, build, or CI result is claimed. This adds lifecycle wiring for the
existing GPUI text and image backends; it is not full Leafer resource parity.

## Reuse and evidence

The checked GPUI checkout is
[`gpui-fast@10d005166a9f9e079d55ebfb700035f3860c3e71`](https://github.com/longbridge/gpui-fast/tree/10d005166a9f9e079d55ebfb700035f3860c3e71).
The adapter uses its public APIs:

- `Window::use_asset::<ImgResourceLoader>(&Resource, &mut App)` in
  [`window.rs`](https://github.com/longbridge/gpui-fast/blob/10d005166a9f9e079d55ebfb700035f3860c3e71/crates/gpui/src/window.rs).
  `None` means pending; `Some(Ok(...))` and `Some(Err(...))` are actual completion.
  GPUI subscribes the current view and notifies it on completion. This is normal
  render-time asset consumption, not a timer polling private cache internals.
- Optional `AnyImageCache::load` in
  [`elements/image_cache.rs`](https://github.com/longbridge/gpui-fast/blob/10d005166a9f9e079d55ebfb700035f3860c3e71/crates/gpui/src/elements/image_cache.rs).
  A host-provided cache must honor GPUI's completion-notification contract.
- `ImgResourceLoader` / `ImageAssetLoader` in
  [`elements/img.rs`](https://github.com/longbridge/gpui-fast/blob/10d005166a9f9e079d55ebfb700035f3860c3e71/crates/gpui/src/elements/img.rs).
  GPUI retains ownership of file reads, its HTTP client, decoding, SVG rendering,
  asynchronous tasks and cache deduplication. No second HTTP client, downloader,
  decoder, or home-grown encoded-image cache was added.
- The existing `flow-text::gpui_backend::shape_text`, already shared by text
  measurement and painting, resolves fonts and shapes the canonical request.
  Success/error/panic recovery supplies the real text-resource outcome. There
  is no invented font-ready flag. The adapter does this once per resource
  generation; normal paint still uses the existing text path.

Leafer references were read at UI
[`1e11641d3ea6b0211c83447268a90b8d58feec25`](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Leafer.ts)
and core
[`1d703303ff0759ce4973d54c87eeb8192dddc570`](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/image/image/src/LeaferImage.ts).
Leafer's Ready follows layout end; ViewReady follows render end. Its later
view-completed check uses the global resource tasker's completion, and its image
`completed` includes errors. Flow follows these distinct concepts with explicit
per-view receipts, rather than copying their names onto an unconditional boolean.

## Identity and observation

`flow-engine::SceneResources` is runtime state. It observes committed engine
changes and owns no document data, network service, rendering backend, or history.
Each `ResourceTicket` includes:

- a unique tracker-owner token, preventing cross-view or disposed/recreated tracker completion reuse
- the engine-scoped `ElementHandle` (including slot generation)
- `attachment_epoch`, changed by detach/reattach
- a checked, monotonically increasing source generation
- resource kind (`Image` or `Text`)

A completion must match all fields, the currently attached instance, the current
canonical source and the committed-change invalidation state. Late results after
source change, destroy, ID reuse, detach/remount, or another engine are rejected.
A single weak engine `on_change` observer catches **A → B → A** edits even if no
render occurred between them. It marks bounded entries invalid rather than
queuing an allocation for every edit. Image fit/size changes do not reload image
bytes; changes to the image source do. Text changes include its full canonical
shaping configuration. Reparenting and ordinary name/style changes preserve the
resource ticket.

`SceneResources::synchronize` discovers all attached built-in text/images,
including currently hidden elements. Discovery itself never loads anything.
`complete` accepts one terminal result for a ticket. All resource operations are
runtime-only: they do not mutate Scene JSON, engine revision/layout, Undo or
selection. Host callbacks run outside engine transactions; observer callbacks
only invalidate state and never dispatch or edit the scene reentrantly.

## Actual event bridge

`SceneView::render` invokes `SceneResourceAdapter::prepare` against its live
engine. GPUI image outcomes and native text shaping feed `SceneResources::complete`.
A successful image's exact `Arc<RenderImage>` is passed through
`DisplayElement::with_image` and its `PreparedDisplayElement` wrapper to GPUI
`img`, avoiding a second untracked load. The original public
`DisplayElement { object }` struct literal remains source-compatible.
Pending, Failed and Cancelled presentations cannot fall back to a filename load.

Element listeners receive the existing `EventKind::Custom` names:

- `resource.pending`
- `resource.loaded`
- `resource.failed`
- `resource.cancelled`

They use the engine's normal capture/target/bubble dispatcher. Event data contains
`resource`, `generation`, `attachment_epoch`, `state`, and optional `detail`.
They are not cancelable. Terminal results dispatch once per source generation.
A source supersession can dispatch cancellation to the same still-attached
instance; cancellation of an old detached/destroyed attachment is returned to
the owning view with `dispatched = false`, never routed into a replacement.

`SceneViewEvent::Resource` exposes these typed transitions to host subscribers.
`SceneViewEvent::ResourceView` exposes view milestones and resource counts.
Resource listeners may enqueue existing scene actions; resource callbacks do not
silently execute input defaults or create an edit transaction.

## Ready, ViewReady, Complete

There is deliberately no general-purpose `ready: bool`:

| Milestone | Actual boundary | Frequency |
| --- | --- | --- |
| `Ready` | The current SceneEngine has committed derived layout, and the current resource set has been discovered | Once per document epoch |
| `ViewReady` | The final SceneView canvas paint callback was traversed and its deferred receipt still matches engine revision, document epoch, resource epoch and counts | Once per document epoch |
| `Complete` | Such a paint receipt has zero pending resources | Once per resource epoch within a document |

`Loaded` means decoded image data is available, or the canonical native text
request successfully resolved/shaped. It is not a promise of successful GPU
upload, unrestricted transforms/clipping, placeholder shaping, or visible pixels.
`ViewReady` is **GPUI paint submission/traversal, not an OS/compositor/GPU
presentation fence**. GPUI's current API does not supply that stronger guarantee.
Existing unsupported-render diagnostics remain authoritative.

Failed and explicitly denied/cancelled resources are terminal and included in
Complete's `failed`/`cancelled` counts. Complete therefore does not mean every
resource loaded successfully. Detach/destroy removes a resource from the current
view's required set; its prior pending observation is cancelled. A pending frame
cannot later announce completion merely because an async result arrived: the
receipt is stale and a new painted frame is required. No frame receipt means no
ViewReady/Complete, including for an empty scene.

## Access policy and host configuration

The default resolver is `local_image_resource`. It passes nonempty local paths
as `Resource::Path` and rejects URI schemes, including HTTP(S), data, file and
custom schemes. It never passes scene strings through GPUI's implicit URI
conversion. Ordinary Windows drive paths (`C:\assets\a.png` and
`C:/assets/a.png`) remain `Resource::Path` references. UNC/network-share,
Windows device and NT namespace paths are separately recognized as paths but
rejected by the default policy; a host resolver may explicitly authorize them as
`Resource::Path`. This default does not prove that an approved local filesystem
path is physically local (for example, a host-mounted volume or mapped drive).
Importing a scene cannot authorize a remote request or network-share path.

A host explicitly supplies `SceneView::set_image_resolver(Rc<dyn Fn(&str) ->
Option<Resource>>, cx)` to approve/map selected references. Returning None
settles them as `Cancelled(NotAuthorized)`. The resolver is runtime-only,
invoked once per ticket, and its result is revalidated before loading. Resolver
panics become Failed. A host may additionally supply `set_image_cache`; remote
and credential policy belongs to that host and GPUI's existing services.

Changing resolver/cache invalidates image generations. Ordinary redraws do not
retry a failure forever. Retrying the same cached backend failure may also require
the host to remove/replace that entry using GPUI's cache API; invalidating a Flow
ticket alone does not evict a shared GPUI asset.

After installing/replacing native fonts, the host calls
`SceneView::invalidate_text_resources(cx)`. There is no verified public GPUI
notification covering every font-resource mutation, and no fabricated one was
added. Font download/provisioning remains outside this adapter.

Paint-only state placeholders remain outside canonical resource identity:
`SceneStateStyles::presentation_at` may substitute an empty label in its returned
presentation copy, which normal `text.rs` shapes/paints. That substitution is
never written into the engine and is not claimed as a new canonical resource.

## Budgets and disposal

- Default maximum: **4,096 attached resource entries per tracker**. Headless hosts
  may choose another limit through `SceneResources::with_limit`.
- Over-limit discovery is rejected before allocating source snapshots or creating
  partial pending entries. Pending/terminal counts are derived from current
  entries, never decremented by stale callbacks or allowed to underflow.
- Invalidation state is bounded by the tracked entries; no unbounded per-edit
  event queue or lifecycle timer exists. Per-view prepared images and resolver
  mappings are pruned to current tickets on preparation.
- `SceneView` release explicitly disposes the tracker, unregisters its observer,
  cancels pending observations once, and releases prepared image references.
  Headless hosts must likewise call `dispose` before dropping a live tracker.
  Its observer uses Weak ownership, so an accidentally dropped tracker cannot
  retain its resource map; an inert observer remains until engine destruction
  if the headless host omits explicit disposal.
- Cancelling a Flow observation **does not cancel/evict a shared GPUI load** used
  by other views. The public API inspected here does not provide per-consumer
  cancellation of a shared native asset task.
- This is a count budget for Flow observations, **not a decoded-byte, encoded-byte,
  cache-eviction, or aggregate background-task budget for GPUI**. The checked
  image loader reads response bytes before decoding. Untrusted remote-source
  byte/pixel/concurrency limits require a host-authorized backend/cache policy;
  this slice does not pretend to enforce those absent backend controls.

## Remaining media boundary

`flow-media` and `flow-ui::MediaView` continue to reuse the existing
FFmpeg/FFprobe/FFplay-backed `MediaBackend`/`MediaController` services. This slice
adds no media process, codec, downloader or alternate player. Retained Scene
currently has no built-in video/audio shape; binding media metadata/first-frame
settlement to live scene resource tickets is **not implemented here**. A future
adapter must bridge existing controller results with the same identity rules,
without treating player construction or `Loading` as resource completion.

## Authored regression coverage

`crates/flow-engine/tests/resources.rs` covers live dispatch/bubbling,
once-per-generation outcomes, late completion, detach/remount (including between
observations), source A/B/A, destroyed-ID reuse, foreign engines/tracker owners, failed/cancelled
completion, stale paint receipts, empty scenes, capacity rejection, bounded
pending counts across source churn, disposal, unchanged document/layout/revision,
fit/size preservation, canonical text changes and explicit font invalidation.
`flow-ui/src/resources.rs` includes local/drive/UNC/device policy and canonical text-request
unit cases; `display/mod.rs` has a legacy struct-literal/wrapper case. These are **authored, not run**. Static source inspection and rustfmt
parsing/formatting do not establish compilation, platform behavior or test success.
