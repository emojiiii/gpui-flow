# Bounded vector, image and text compositing in SceneView

Status: bounded vector, static-image, elapsed-time animated-image, host-opt-in same-backend text and explicit owned-canvas backdrop implementation. Explicit [Box/Frame paint phases](container-paint-order.md) passed the current local Linux aggregate and targeted native smoke checks; see the [phase-order checkpoint](#container-paint-order-validation-2026-10-01) for scope and limits. Validation checkpoints below distinguish this batch from earlier local Linux passes. The vector native-window smoke test below was recorded on 2026-09-30; static-image validation is reported separately. This is a bounded Leafer-inspired slice, not complete Leafer or GPU-backdrop compatibility. The first headless checkpoint is [9bd37d4](https://github.com/emojiiii/gpui-flow/commit/9bd37d479cf546e0d58b123465a2df07cf13bc3c); the subsequent integration adds the production SceneView consumer.

## Actual rendering path

`SceneStateStyles::presentation_at` produces one presented Scene. The adapter derives both native objects and ordered compositing boundaries from that same tree. Each supported domain replaces its entire native subtree. It is never drawn alongside flattened copies of its children.

The new headless `gpui-flow-composite` crate directly calls locked tiny-skia 0.11.4 for path filling, gradients, premultiplied RGBA surfaces, alpha masks and SourceOver/Multiply/Screen. Shared `Shape::stroke_mesh` supplies the existing Kurbo/Lyon centered stroke geometry; consistently wound triangles form one compound fill so shared edges do not repeatedly apply alpha. The production worker completes the whole domain before publishing bytes.

At the GPUI boundary, those bytes pass once through `swap_rgba_pa_to_bgra` into a real `RenderImage`. `Window::paint_image` uploads the candidate to the atlas and inserts the native sprite. Only a successful call accepts the candidate. This is not GPUI `paint_layer`, a backend trait without a producer, or a screenshot/readback workaround. The locked GPUI conversion can truncate an unpremultiplied channel by one byte; low-alpha conversion is not lossless.

## Public model and supported semantics

- `Style.blend_mode`: `PassThrough` (JSON `pass_through`, legacy default), `Normal`, `Multiply`, `Screen`, `Overlay`, `Darken`, `Lighten`, `ColorDodge`, `ColorBurn`, `HardLight`, `SoftLight`, `Difference`, `Exclusion`, `Hue`, `Saturation`, `Color`, `Luminosity`
- `DisplayObject.mask`: `None` (legacy default), `Alpha`; a mask must be a direct Group child
- `DisplayObject.eraser`: `None` (legacy default), `Pixel`, `Path`; [bounded scene erasers](scene-erasers.md) isolate the direct parent, use complete producer alpha for Pixel and filled geometric leaf contours for Path
- Engine `PropertyKey::BlendMode` marks PAINT; `PropertyKey::Mask` marks PAINT | GEOMETRY, without redefining picking
- Group opacity and multi-paint primitive opacity apply once after the object's complete contents, including overlaps
- Normal groups isolate; pass-through groups reuse their actual CPU parent. Internal multiply/screen blend against preceding pixels in the same domain
- Alpha masks are hidden and affect following stable z-ordered siblings until another mask. Opaque-mask independent blends flush against the CPU parent; translucent-mask runs composite first, then apply alpha once
- Rect, rounded Rect, Ellipse, Line, Polygon, Star, Path, Pen, nested Group and fixed-size Box/Frame are supported, including current fills/gradients and centered stroke meshes
- Each selected Image frame uses decoded pixel dimensions for Contain/Cover/Fill, with bilinear sampling, layout-box clipping for Cover, full affine placement, image alpha masks and the same group/blend/clip path as vectors
- Fixed-size Box/Frame default to own fills → own strokes → children. Explicit `ContainerPaintOrder::StrokeAfterChildren` selects own fills → children → own strokes in both native SceneView and CPU domains. Existing constructors and omitted JSON keep the legacy order. Opacity applies once to the complete container; rounded child clips do not clip its own outer stroke
- Box overflow is visible by default; Frame clips descendants by default. Nested rounded child clips intersect under affine transforms, without turning pass-through containers into isolated blend groups. Inherited clips outside the domain still apply to the finished output

Missing JSON fields retain old defaults. Rust `Style` and `DisplayObject` exhaustive literals need the new fields or constructor/default syntax; Box/Frame literals now need `paint_order`, and exhaustive `SceneUnit` matches must handle `NativePhase`. `RenderObject` and `DisplayElement { object }` construction shapes remain unchanged. See the [paint-order migration contract](container-paint-order.md#rust-api-and-serialization-compatibility). This is backward JSON reading, not Leafer JSON import compatibility or compatibility with older renderers.

### Additional artistic blends

The 13 modes after Screen are implemented for all existing domain producers,
including owned-canvas roots, using the same isolation, alpha/clip and budget
rules. JSON names use snake_case (`color_dodge`, `hard_light`, etc.). Every
artistic mode requires an actual backdrop: default Domains mode rejects it at
an exposed root; OwnedCanvas supplies its own background and preceding scene
pixels. This does not capture arbitrary host UI.

Separable modes reuse tiny-skia 0.11.4. Tests compare them to independent channel
equations with both opaque and half-opacity sources. The four non-separable
modes use a small scalar adapter for the [W3C non-separable equations](https://www.w3.org/TR/compositing-1/#blendingnonseparable),
because the locked tiny-skia highp `clip_color` lower-gamut condition checks the
maximum rather than minimum component. An opaque red-over-blue Hue fixture
produced [207,0,0] through that implementation instead of the expected [94,0,0].
The adapter retains the existing surfaces/rasterizer, applies source opacity,
source-over alpha and clip coverage once, checks cancellation per row, and
allocates no extra surface. It uses encoded RGB, not linear-light or ICC-aware
color management. [Scene erasers](scene-erasers.md) add an internal destination-out operation; unrestricted Porter-Duff copy and filters remain separate work.

`owned_canvas` has a Next blend control cycling all 15 artistic modes and shows
the selected blue object's mode in its footer.

## Explicit owned-canvas backdrop

Set `view.compositing_mode = gpui_flow::SceneCompositing::OwnedCanvas` while constructing/updating the view, then notify it normally. `Domains` remains the default. This runtime setting is not serialized into the Scene.

OwnedCanvas rasterizes the view's opaque `theme.background`, optional `show_grid` / `theme.grid`, and the complete presented document into **one bounded CPU domain**, in that order. Roots retain stable sibling z-order and tree order. Top-level Multiply/Screen and blends inside pass-through groups can now read the actual preceding roots, grid and background. Explicit Normal groups and opacity isolation still isolate their children, while masks and rounded clips use the existing implementation. The native background/grid/document paint is replaced, never duplicated. Selection, resize handles, marquee and diagnostics are drawn afterward as overlays and are not blend backdrops.

Any root eraser marker, including a hidden marker, forces a separate document surface within that domain. Document erasure then reveals the separately retained background/grid; root artistic blends see that isolated document surface. See the [eraser ownership and mask schedules](scene-erasers.md).

The host still supplies the existing image resolver/cache and, for visible Text, the explicit same-native-backend text bridge above. Current static and animated images and supported text use the same immutable inputs, clock, worker, upload acceptance and retirement lifecycle. Visible Custom, missing text capability, unsupported text styles/transforms, missing/denied/failed image input or any budget error rejects the entire canvas. There is no partial native fallback or reordered native overlay. Hidden/zero-opacity subtrees do not require producers; hidden mask markers retain zero-alpha runs. Ordinary host controls outside SceneView are not part of the domain.

This is **not arbitrary external GPUI backdrop capture**. A toolkit widget or host surface behind the view cannot become a blend input. The headless `owned_canvas(scene, CanvasBackdrop, limits)` API requires an opaque background and rejects transparent backgrounds with `UnsupportedExternalBackdrop`; treating transparency as a captured host background would be incorrect. `scene_units` keeps the original per-domain external-backdrop rejection. No GPUI fork, GPU/readback path, screenshot producer or new rendering dependency is involved.

The grid uses one-logical-pixel dots, canvas-relative pan offset and spacing `max(24 * zoom, 12)`. It is generated only for the visible allocation while retaining the full canvas anchor, including fractional origins and DPR. One bounded compound tiny-skia path draws the grid, rather than one full-surface operation per dot. Grid points join the geometry budget; background/grid surface writes are charged. CPU antialiasing is not promised identical to native quads at fractional edges.

The complete canvas allocation is intersected with its viewport and parent content-mask bounds, snapped outward to physical pixels, and uses the existing limits: 4096 pixels/axis, 4,194,304 pixels/domain, 128 MiB explicit live storage, and 32,000,000 conservative surface writes/request. The runtime canvas wrapper counts as one node/depth level. A 1920×1080 logical canvas at DPR 2 exceeds the pixel cap and fails explicitly; no automatic tiling or downsampling is performed. Supported offscreen prepared ink is conservatively culled, including stroke reach and a one-pixel antialias guard, before intermediate layer allocation. This does not skip producer validation/decoding for other scene-visible nodes merely because their current coordinates are offscreen, and it does not promise large-scene frame rates.

Theme/grid, mode, tree/presentation, resources/fonts, pan/zoom, window placement, viewport, parent mask and DPR participate in exact request identity. Mode/theme/grid or geometry changes hard-cancel stale work and hide incompatible accepted frames; time-only image samples still coalesce without starving admitted work. The runtime canvas cache ID is empty (a validated Scene cannot contain an empty ID), is never inserted into the canonical document, and is excluded from document/resource identity enumeration. Diagnostics label it `canvas`.

Run `cargo run --locked --offline -p flow-studio --bin owned_canvas`. The example includes top-level blends over background/grid and earlier roots, real text and an animated GIF, theme/grid/mode/blend controls, whole-canvas Custom rejection/recovery, drag/pan/zoom and Undo. Compilation or source fixtures alone are not native-window validation; the actual validation checkpoint is recorded separately below.

## Fixed-size painted containers

Box and Frame now enter the real planner, tiny-skia vector preparation and raster worker. They reuse existing shape contours, fill gradients and centered stroke meshes; there is no second renderer or replacement hit tree. SceneView consumes the whole domain at its normal subtree substitution point. Presented hover/focus styles and Undo use that same path.

Child clipping uses an 8-bit mask on each actual paint or isolated-layer composite, not a synthetic isolated child surface. A pass-through Frame's Multiply/Screen children therefore still see the preceding CPU backdrop. Isolated children and alpha-mask runs receive the inherited clip once when returned to the parent. Nested masks and their intersection scratch storage/work are budgeted. Clipped child bounds reduce output allocation; own fill/stroke bounds remain outside this child restriction. Clip bounds retain f64 world precision before viewport cancellation, including large translations.

`Box.paint_order` and `Frame.paint_order` explicitly select `BeforeChildren` (own fills → own strokes → children) or `StrokeAfterChildren` (own fills → children → own strokes). Existing constructors and missing JSON fields retain `BeforeChildren`; rounded corners, clipping, opacity and blending do not silently change it. `SceneUnit::NativePhase` schedules native entry/exit paints, while a captured domain still substitutes one whole subtree. CPU preparation splits its existing paint vector after the emitted fills and submits each layer once inside the same container isolation. `Scene::paint_list`/`paint_order`, geometric picking and event ancestry remain structural, with one entry per object. A child can still be picked beneath a visually overlaid parent stroke.

This is a bounded explicit option, not complete Leafer Box/Frame parity. In the pinned source, `BoxData.__drawAfterFill` is conditional on `__single || __clipAfterFill`; Leafer does not universally put container strokes after children. Its picker also prefers descendants over an overlapping Box/Frame. Flow does not reproduce that automatic phase-selection policy. Frame keeps the existing Scene style defaults rather than adopting Leafer's default white fill. This change adds no auto-sizing, scrolling, path-input boxes or other Leafer-only rendering properties. Container shadows are a separate [bounded extension](group-shadows.md). See the [full contract, pinned source citations and validation scope](container-paint-order.md).

Alpha masks still require a direct Group parent. A Box/Frame can itself be that Group's mask, including its painted children, and it can contain a Group with mask runs. Direct mask children of Box/Frame remain a validation error. No mask/disclosure/resource permissions are widened.

Both paint phases use only the container's inherited ancestor clips; the child clip is applied to descendants and released before the final own stroke. Mask runs inside children complete before that stroke. An enclosing mask and container opacity operate on the complete container, without one mask/opacity application per phase. Changing the order is an incompatible composite placement change, including when the container is nested: accepted stale frames cannot be reused across the change. Phase ordering adds no isolation boundary or extra CPU surface of its own, and all existing geometry, clipping, allocation and surface-work limits remain in force. The [phase-order checkpoint](#container-paint-order-validation-2026-10-01) records the current verification separately from the unchanged historical fixed-container checkpoint.

Run `cargo run --locked --offline -p flow-studio --bin composited_containers` for Box overflow, rounded Frame clipping/opacity, pass-through clipped blending, and Undo controls. Labels remain native outside domains.

## Host-opt-in same-native-backend text

The application host captures the public `PlatformTextSystem` before constructing its GPUI Application. Flow's helper passes that **same Platform instance** to `Application::with_platform` and retains the resulting native `TextSystem` together with its raster backend. It never creates a second font system, resolves glyph IDs against a guessed font file, or replaces shaping/fallback. Use the locked desktop platforms, whose `text_system()` returns the same Arc; custom Platform implementations must honor that stable-backend contract.

```rust,ignore
let platform = gpui_kit::platform::current_platform(false);
let (app, rasterizer) = gpui_flow::GpuiTextRasterizer::application(platform);
app.run(move |cx| {
    // In the host's normal view-construction closure:
    let scene = gpui_flow::scene::Scene::new();
    let _view = cx.new(|cx| {
        let mut view = gpui_flow::SceneView::new(scene, cx).unwrap();
        view.set_text_rasterizer(Some(rasterizer.clone()), cx).unwrap();
        view
    });
});
```

This is an explicit capability, not automatic support for arbitrary existing App/Window instances: GPUI keeps their raster handle private. `set_text_rasterizer` rejects a bridge belonging to a different Application. Passing `None` withdraws the capability. No new font dependency or GPUI fork is introduced. The current helper is for locked desktop platforms; Web raster calls are explicitly rejected. The default SceneView still has no CPU text producer.

The supported subset is the current plain-text model with zero/one solid fill, explicit family/weight/size/line height, newline/empty-line handling, optional automatic wrap, and start/center/end alignment. Positive axis-aligned uniform text keeps its existing raster path. [Bounded affine Text](text-affine.md) also supports rotation, skew, reflection and nonuniform scale through maximum-scale local glyph pixels and the existing image transform. Text strokes, gradients and multiple fill layers remain explicit whole-domain errors; effect-specific transform restrictions are unchanged. Hidden/zero-opacity subtrees need no raster input. A fully transparent/no-fill text produces no ink.

`flow-text::shape_text` supplies the same `WrappedLine` layouts used by native text, retaining actual fallback FontIds, glyph IDs, ligatures/clusters and GPUI wrap boundaries. A thin paint walker follows GPUI's baseline `(line_height - ascent - descent)/2 + ascent`, per-glyph vertical offsets and wrap/alignment origins. Fonts, glyph selection, bidi and language behavior remain whatever the locked GPUI backend supports; this does not claim new shaping-language support. Device-pixel origin quantization follows GPUI's 4×1 grid, with the integer-origin emoji special case.

Transparent CPU layers use grayscale A8 coverage, never LCD RGB coverage. Normal glyph ink is multiplied by the solid color into premultiplied RGBA. Color glyphs retain backend RGB; the text fill alpha modulates their alpha, and complete group/node opacity is applied once by the compositor. This explicit color-glyph alpha behavior avoids tinting emoji; native GPUI's direct emoji API itself ignores text-run color. Emoji with a monochrome-only bitmap use its A8 coverage. macOS's possible extra fractional-origin row/column is accepted; unexpected dimensions/byte formats reject. Native opaque LCD text is not promised pixel-identical to grayscale CPU output.

The immutable text bitmap enters `TextInputs` / `rasterize_with_inputs`, so actual transparent glyph gaps, rounded ancestor clips, text-as-alpha-mask, internal Multiply/Screen, and complete-domain opacity use the existing pixel implementation. Placement is generated at final screen DPI and mapped back into local coordinates. It does not draw the text layout rectangle as a mask, capture screenshots, or add a native overlay above the domain.

The request key includes the backend identity/epoch, presented source, per-element text ResourceTicket, attachment, document, viewport, window and DPR. Source/font/capability/geometry changes cancel old work; stale font epochs are checked during glyph production, before accepting the job and immediately before paint/upload. Call `SceneView::invalidate_text_resources` after host font changes and notify each consuming view. Measurement/reflow caches still use their existing explicit invalidation API; this raster bridge does not secretly change layout or editor history.

Text input is bounded to 64 KiB UTF-8, a 512-byte family, 8,192 shaped glyphs, font size ≤512 device pixels, glyph dimensions ≤1,025 pixels including fractional padding, and a clipped output ≤4,096 per axis / 4,194,304 pixels. Each raster call has a job-local cache of at most 256 glyphs / 4 MiB (larger admissible single glyphs remain uncached), which includes font/glyph ID, size, DPR, fractional variant, emoji and dilation via GPUI's `RenderGlyphParams`; backend/epoch cannot vary inside the call. No unbounded persistent Flow glyph cache exists. Combined image/text converted inputs and active glyph scratch stay under the 32 MiB input ceiling; producer pixel writes share the existing 32-million request work ceiling. The 128 MiB explicit working set still includes retained outputs, inputs, scratch and compositor surfaces. GPUI/platform-owned font/shaping/internal raster caches and GPU allocations remain outside that explicit Flow accounting.

Run `cargo run --locked --offline -p flow-studio --bin text_raster_probe` for real-font/headless pixel checks and `cargo run --locked --offline -p flow-studio --bin composited_text` for the native comparison, text alpha mask, rounded clipping/Multiply, bridge withdrawal/recovery, font invalidation, alignment, opacity and Undo controls. Runtime verification is recorded separately below; API/source inspection alone is not a cross-platform test. Browser/Web text rasterization and macOS/Windows execution are not covered by the Linux evidence.

## Static decoded-image input

SceneView reuses its existing authorized resource resolver and GPUI public asset/cache subscription. A current resource ticket and `RenderImage` identity enter the immutable request key. The background worker accepts exactly one decoded frame, converts the pinned GPUI straight-BGRA contract to premultiplied RGBA once, and uses tiny-skia's bilinear image pattern. There is no new downloader, decoder or decoded-image cache. Animated resources use the runtime clock described below.

- Contain, Cover and Fill use decoded dimensions, matching GPUI; `natural_size` does not override real decoded aspect. Cover crops to the image layout rectangle
- Image pixels participate in full affine transforms, group/local opacity, internal Normal/Multiply/Screen, image or vector alpha masks, and inherited rounded clips
- Image style fills/strokes remain ignored, consistent with the native image path
- Pending, failed, cancelled, denied, malformed or oversized selected-frame inputs reject the whole affected domain. Hidden/zero-opacity images and ancestors do not require pixels; hidden masks retain their zero-alpha masking semantics
- Source, resource generation, resolver/cache authorization, detach/reattach and document changes invalidate old image pixels. A/B/A requests and cancelled workers cannot publish obsolete results
- Shared decoded `(ImageId, frame_index)` snapshots convert once per worker request; converted copies have a 32 MiB batch cap and count against the explicit working set during each raster operation
- Working-set failures retry only after real adapter-owned bytes are released. Current accepted/candidate successes and terminal failures survive these budget-only retries; successful domains cannot manufacture a repeating retirement/upload loop. Dropped over-cap outputs release their byte charge without resetting cumulative work

The static-image example is `cargo run --locked --offline -p flow-studio --bin composited_images`. It provides SVG/PNG, missing/restore, deny/authorize, Multiply/Screen and Undo controls. Text labels remain native outside the domains. Static SVGs are rasterized by GPUI before this adapter receives pixels. GIF and animated WebP use the same decoded-resource producer and selected-frame path.

The headless producer accepts `StaticImage::from_straight_bgra`, `ImageInputs` keyed by display ID, and `rasterize_with_images`. It validates dimensions and byte length before allocating the premultiplied RGBA copy; zero-frame or out-of-range selected-frame inputs return `InvalidImage`. The legacy `UnsupportedAnimatedImage` error variant remains available to external input producers, but SceneView no longer emits it for supported decoded animations. The default resource resolver permits local paths only; a host must explicitly authorize a URI. Host resource completion remains separate from successful composition/upload.

## Animated images and runtime controls

SceneView uses GPUI's public `RenderImage::frame_count`, `delay`, `size` and `as_bytes` methods. The existing GPUI GIF/WebP decoders supply full decoded frames, including GIF disposal. A host-supplied `RenderImage` must also contain complete BGRA frame canvases; this adapter does not reconstruct partial rectangles. APNG remains static under the pinned GPUI loader. No source-file loop-count metadata survives that loader, so playback deliberately loops indefinitely; finite loop counts, playback-rate controls and media/editor-timeline synchronization are not implemented.

One runtime `Playback` per current `ResourceTicket` selects a frame from elapsed monotonic time and an immutable shared timing table. Delay boundaries use real per-frame durations, with a documented 10ms minimum for zero/short delays. Selection uses a bounded prefix table and binary search, not one frame advance per refresh. Large elapsed durations are reduced modulo the cycle before addition. Limits are 4096 frames per image and 65,536 timing entries per view, shared between instances of the same decoded image. Aggregate admission is retried after actual timing-table capacity is released. These limits apply after GPUI decoding; they cannot prevent the loader allocating a large encoded animation first.

Both SceneView's ordinary native Image route and CPU-domain Image route consume that same selected index. Native paint calls the public `Window::paint_image(index)` directly; it does not guess GPUI's private image cursor. Standalone `DisplayElement` keeps GPUI's native image cursor, now with an ID on the image element itself; loaded identities include `RenderImage.id`. Standalone controls are not shared with a SceneView.

```rust,ignore
let handle = view.editor.engine.handle(&"image".into()).unwrap();
view.set_image_playing(handle, false, cx); // pause at its current frame/phase
view.seek_image(handle, std::time::Duration::from_millis(250), cx);
view.set_image_playing(handle, true, cx); // resume, preserving frame remainder
```

These methods return false until the current handle/resource generation has a loaded admissible image. They are runtime-only and do not change Scene JSON, history or canonical properties. Source changes, resolver/cache replacement, detach/reattach and document replacement reset playback. Pause/resume and seek advance a playback generation, so an older CPU job cannot overwrite the newly requested position. A/B/A resource changes retain their existing ticket protection.

Node/ancestor hidden or zero opacity, rejected native images, inactive/occluded windows and reduced-motion mode freeze the phase and stop animation-frame requests. Window visibility/activation observations wake the view on return. Viewport-only clipping currently does not freeze an otherwise visible node. Each instance can be paused/seeked independently even when it shares decoded pixels. Static images never request an animation frame.

The worker converts only each admitted `(image, selected frame)` snapshot, including its own dimensions. It never preconverts all frames. Animation-only ticks cannot cancel that snapshot or clear an unuploaded candidate; the next admission samples the latest playhead, allowing slow jobs to make visible progress while skipping obsolete time samples. Resource/scene/geometry/authorization/seek changes still hard-cancel. Successful unchanged domains are reused. Each accepted domain retains only its own source observations, so it cannot pin a removed neighbor's decoded animation.

Frame-local invalid/size/memory failures are memoized per selected-frame vector (memory failures also include the actual byte-release epoch), allowing a later valid frame to proceed without retrying the identical bad sample every cycle. This metadata is capped at 128 failed samples per domain; exceeding it rejects that domain until a semantic change. Upload and other domain-wide terminal errors stop its animation driver until recovery. Existing 32 MiB input/output/retirement and 128 MiB explicit working-set limits still apply; the GPUI-owned full decoded animation retained by an Arc remains outside that accounting.

Run `cargo run --locked --offline -p flow-studio --bin animated_images`. The original three-frame fixture uses 200/400/600ms delays, visible color/shape changes and transparent disposal. Four panels show native, group-alpha, internal Multiply and an animated image alpha mask, with pause/play, three seeks, hide/show, static/animated source and deny/allow controls.

## Honest boundaries

Custom inside a required domain rejects the entire domain. Text requires the explicit same-platform bridge described above and rejects unsupported styles/transforms; without that bridge, `TextBackendRequired` rejects the entire domain. Static and animated Image nodes have a real selected-frame pixel producer. The view shows a diagnostic; it does not silently omit the unsupported descendant or fall back to individually faded native children. Ordinary text/image/custom siblings outside domains retain their native route.

In the default `SceneCompositing::Domains` mode, a domain root that needs multiply/screen against external content returns `UnsupportedExternalBackdrop`. A mask alone does not isolate a pass-through opacity-1 Group: a blend escaping an opaque mask run is rejected too. Explicit Normal groups and opacity isolation are supported, as are translucent mask runs that genuinely isolate their content. Put a required vector background inside the same Normal Group. Native siblings, host background, grid and selection decorations are not captured by that default mode. The explicit owned-canvas mode below includes only pixels owned by SceneView; arbitrary host GPUI content remains unsupported.

[Bounded scalar grayscale](grayscale.md), [foreground blur](foreground-blur.md), [scale-fixed strokes](scale-fixed-strokes.md) and [Image alpha hit](image-alpha-hit.md) now have their own explicit contracts. Beyond these and the [bounded scene erasers](scene-erasers.md), no unrestricted eraser, spread/box/scale-fixed shadows, general filter graph/background blur, luminance/path/clipping mask modes, automatic resolution reduction or tiling is claimed. The single leaf-shadow subset is described below and [bounded subtree shadows](group-shadows.md) extend it; supported text, artistic blends and closed aligned strokes are covered in their dedicated sections. The engine's hit testing and conservative selection/layout bounds still include geometry beyond transparent mask pixels. Alpha-mask bounds do not become Leafer-style layout bounds. Rounded contours and stroke expansion retain the existing 0.1 local-unit approximation; gradient/AA bytes need not match the older native renderer. There is no wide-gamut, ICC or linear-light guarantee.

## Budget, invalidation and lifecycle

Per request: at most 8 domain boundaries; each domain has at most 512 nodes, 16 tree levels, 64 mask runs, 65,536 source commands/points per shape, 262,144 total source commands/points and 262,144 generated stroke triangles. Gradients have at most 256 stops; paints retain the existing 32-layer limit. Domain snapshots beyond the count limit are rejected before cloning.

A domain and each converted static input are at most 4096 pixels per axis and 4,194,304 pixels total. Adapter-owned converted pixels, explicit CPU surfaces, accepted/candidate images and retired images share a 128 MiB ceiling. GPUI/cache-owned decoded originals, including originals kept alive by request keys, are outside this adapter budget; it is not a total process-memory bound. SceneView additionally limits converted static input copies to 32 MiB, each accepted/candidate output batch to 32 MiB, and retirement to 32 MiB. Identical `(ImageId, frame_index)` pairs share one converted input per worker request. Discarding an over-cap output releases its surface charge before considering later domains. Capacity is checked immediately before upload using the live state after earlier domain paints. One CPU job runs at a time. Semantic changes replace/cancel its request; newer animation-only samples coalesce while one admitted snapshot finishes and uploads. Cancel is checked between kernel calls, not inside a tiny-skia call or the bounded image Gaussian call.

Returning to an earlier request after supersession invalidates the completion marker even when canonical revision is unchanged. Cleared/unaccepted candidates, prior failed completions and blocked uploads can therefore retry after an A → B → A viewport/presentation sequence; two regressions reproduced the bug before the fix and pass afterward.

A request permits at most 32,000,000 conservative full-surface write pixels and shadow-filter work units across domains. Full-surface accounting can reject a large surface with many tiny paints. Nonfinite/singular matrices, tiny-skia-rejected inverses and control points outside ±1,000,000 device pixels fail explicitly. Dimensions and surface buffers use checked arithmetic and fallible reserve before allocation. Private scan-conversion allocations, geometry snapshots and GPU atlas/driver allocations are not a hard whole-process memory guarantee.

Keys use the complete presented domain values/order, ancestor clips, all relevant ancestor/descendant engine handles and attachment epochs, document epoch/revision, window, viewport/origin/content mask and DPR. There are no cross-view global composite caches. Paint-only presentation changes cannot be hidden behind a canonical revision. Geometry, clips, order, attachment or document changes cannot reuse a stale placement.

A previous complete frame may remain for conservative paint/opacity-only changes at unchanged placement/identity, accompanied by `StaleFrame`. Normal animation-only lag retains the last complete frame without a stale-error badge; only fully admitted snapshots are published. Upload failures preserve that frame. Incompatible/removed frames are never displayed and retire on the next frame before `drop_image`; retirement pressure delays candidate upload. Release cancels work and drops only this view's owned composite images. A repeated unrecoverable upload is blocked until a new semantic request, rather than retrying each animation tick. A real retired/candidate byte release can retry WorkingSetLimit domains once at the new budget epoch, including a release during a worker run. Successful current-domain outputs/candidates and terminal error diagnostics survive that retry; they are not republished to manufacture another retirement/retry loop.

`SceneView::composite_diagnostics` and `compositing_pending` expose status independently from resource readiness. Resource `Complete` is still its existing resource/paint receipt; it is not redefined as composite success or a GPU fence.

The loading badge distinguishes a missing displayable frame from a background
refresh. Animation resampling can leave `compositing_pending` true without
showing `Compositing…` when every requested domain already has a compatible
paintable frame. Errors remain visible independently. First load, missing
domains, revoked resources/fonts, and incompatible geometry still withdraw
pixels and display loading/error status; this does not reuse stale pixels across
those boundaries or eliminate the hard-invalidation blank interval.

Raster work uses GPUI's background executor, but full Scene validation, snapshot preparation, existing native resource-preflight text shaping and some shared stroke validation still happen on the UI thread. This slice does not promise a frame-time bound, incremental damage rasterization or large-scene interactive performance.

## Validation and reproduction

```sh
cargo test --locked --offline
cargo test --locked --offline -p gpui-flow --lib
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo build --locked --offline -p flow-studio --bin compositing
cargo run --locked --offline -p flow-studio --bin compositing
```

The earlier vector integration checkpoint recorded 583 headless tests including doctests pass (8 existing ignored tests remain ignored in this run), 47 UI library tests passed, workspace/all-targets strict clippy passed, and the native `compositing` binary linked. The 18 CPU fixtures and 3 new scene/engine contract cases are included in the 583; 9 new UI cases are included in the 47. No old test count is added again. This run did not repeat the earlier separate ignored-media checks or all standalone documentation binaries.

Focused coverage includes overlapping/nested opacity, multiple paint layers, internal blend backdrops and Normal/pass-through differences, alpha gradients/multiple mask runs/opaque-vs-translucent blend flush, inherited rounded clips and fractional DPR, unsupported entire domains, empty/offscreen/cancelled work, input/surface/work budgets, exact key changes, attachment/document/window changes, stale completion rejection, upload acceptance/failure and retirement pressure. Two review regressions explicitly reject the pass-through masked external backdrop and tiny-skia near-singular gradient inverse.

The native `compositing` example was opened on the cloud Linux desktop and inspected through target-window screenshots, not a full-desktop capture. The following visible flows were exercised:

1. Initial red/blue overlap has uniform 50% group alpha; overlap does not accumulate opacity
2. Opacity control changes the complete group to 100%, then back via Undo
3. Multiply over the internal yellow rectangle is dark; Screen changes the overlap to a light pink/white region
4. The ellipse mask shows only clipped red/blue contents; switching its opacity from 100% to 50% dims the whole run once
5. Three consecutive Undo actions restore the original opacity, blend and mask states
6. Wheel pan and Ctrl+wheel zoom move native labels and composite output together and respect canvas clipping
7. Rapid mask toggles followed by window close remove the test window; reopening displays the stable initial scene

Native controls retain initial descriptive labels; their button text describes the toggle choices. A brief `StaleFrame` notice during recomputation clears on acceptance. These are smoke observations, not pixel-perfect screenshot goldens or a performance benchmark. Live GPU failure injection, explicit resize/multi-monitor/DPR switching, exhaustive race schedules, macOS/Windows and CI were not exercised. No user computer, remote resource fetch, new image decoder or independent text shaper was used.

## Static-image validation, 2026-10-01

Final local Linux source checks: 695 headless tests/doctests passed (including 31 CPU-composite tests; 8 pre-existing ignored tests remain ignored), 56 UI library tests passed, and workspace/all-targets strict Clippy, formatting, and the native `composited_images` build passed. Counts describe each aggregate once, not a sum of repeated focused runs. Builds use the locked dependencies, one job, debug info/incremental compilation off and a 3 GiB free-disk guard. CI stays paused.

Image fixtures cover decoded-aspect Contain/Cover/Fill, premultiplication/channel order, alpha/group opacity, affine placement, real image masks/blends, inherited rounded clipping, hidden/zero-opacity dependencies, hidden blend/mask planning, overflow and malformed/missing/animated inputs. UI fixtures cover resource-ticket/source A/B/A invalidation, cancelled workers, actual shared-source conversion and output-cap budget recovery, mixed success/permanent-memory failures, terminal upload diagnostics, and releases before/during retries. The mixed-result fixture verifies successful image identity survives retry and no new retirement loop can be manufactured. GPU failure is simulated at the controller boundary; it is not injected into a live driver.

Native smoke test on the dot cloud Linux desktop used only the target app screenshots. After relaunching the freshly linked binary, SVG/PNG switching and a rapid PNG/SVG/PNG sequence settled on the requested pixels. Missing-file and revoked-resolver states removed all three affected domains and showed the expected diagnostic; restoring each state recovered complete SVG output. Multiply changed to Screen against the internal yellow backdrop, and Undo restored Multiply. The test process was then stopped and its window disappearance confirmed. These are visible smoke observations, not pixel-golden comparisons; this pass did not exercise pan/zoom, native resize or close-during-work races.

The static-image pass did not rerun the separate ignored-media tests, standalone documentation executables, all old example interactions, cross-platform checks, performance benchmarks, DPI/multi-monitor changes or live GPU error injection. No dependency versions, CI workflow settings, source schemas or user-computer files changed.

## Source references

[Leafer blendMode](https://www.leaferjs.com/ui/reference/UI/blendMode.html), [mask](https://www.leaferjs.com/ui/reference/UI/mask.html), [pinned mask implementation](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/mask/src/index.ts), [pinned Box implementation](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Box.ts), [pinned BoxData phases](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/data/src/BoxData.ts), [pinned UIRender fill/content/stroke ordering](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/render/src/UIRender.ts#L55-L85), [pinned descendant-first Picker](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/selector/src/Picker.ts#L53-L83), [pinned Frame defaults](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Frame.ts), [pinned LeafData isolation](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/data/src/LeafData.ts), [GPUI decoded frame API](https://github.com/longbridge/gpui-fast/blob/10d005166a9f9e079d55ebfb700035f3860c3e71/crates/gpui/src/assets.rs), [GPUI GIF/WebP loader](https://github.com/longbridge/gpui-fast/blob/10d005166a9f9e079d55ebfb700035f3860c3e71/crates/gpui/src/elements/img.rs), [GPUI paint_image](https://github.com/longbridge/gpui-fast/blob/10d005166a9f9e079d55ebfb700035f3860c3e71/crates/gpui/src/window.rs). The [container paint-order contract](container-paint-order.md#pinned-leafer-reference) ties the conditional source branches to the bounded implementation. These sources establish the intended bounded semantics; they do not turn this adapter into full Leafer compatibility.

## Container validation, 2026-10-01

The fixed-size container slice passed 709 headless tests/doctests (46 CPU-composite fixtures included; 8 existing ignored tests remain ignored) and 58 UI library tests. New coverage includes fill/stroke/children ordering, outer-stroke preservation, visible overflow versus rounded clipping, local opacity, nested transforms and fractional DPR, inherited hover and unchanged canonical hit/focus, Undo pixel restoration, container alpha masks, grouped masks inside a clipped container, Multiply/Screen backdrops, bounded allocation, large translated clips, static-image descendants and hidden dependencies, and working-set/work/input failures with no retained temporary bytes. Earlier static-image retry, cancellation and source-authorization fixtures remain in the aggregate.

Formatting, workspace/all-targets strict Clippy, and the `composited_containers` native binary build passed using locked/offline dependencies, one build job, debug info/incremental off and the existing 3 GiB disk guard. A final UI command took 254 seconds end-to-end during shared-host load; its test harness reported 58 passes in 14.45 seconds. Earlier and final receipts are retained locally; no timeout was changed and this is not a performance result.

The linked native app was opened on the cloud Linux desktop and inspected only through its target-window screenshots. The smoke check verified initial Box overflow versus rounded Frame clipping, parent outer strokes, Frame clip off/on, complete Frame alpha 50%/100%, Multiply/Screen against the actual internal yellow backdrop, and three Undo actions restoring the original pixels. Wheel pan and Ctrl+wheel zoom kept native labels and composite output aligned and canvas-clipped. Four repeated clip toggles settled on the correct rounded output. A temporary `StaleFrame` notice cleared after recomputation. The app window was closed and its disappearance verified.

These are visible smoke observations, not pixel-golden or latency tests. This slice did not rerun standalone documentation executables, ignored-media tests, all older example interactions, close-during-work/reopen, native resize, CI, user-computer, cross-platform, DPI/multi-monitor, performance or live GPU-failure checks.

## Animated-image validation, 2026-10-01

Final local Linux checks passed: 709 headless tests/doctests (46 CPU-composite fixtures included; 8 existing ignored tests remain ignored), 73 UI library tests, workspace/all-targets strict Clippy, formatting, and the native `animated_images` build. The UI aggregate includes unequal/zero-delay timing, loop boundaries and large elapsed jumps, pause/hidden remainder, backward seek, independent instances, GIF disposal/color/alpha decoding, actual selected-frame CPU pixels with differing dimensions, shared decoded images at different playheads, admitted slow-worker progress, seek/resource/geometry invalidation, sample-local failure memoization, removed-neighbor source release, reclaimed timeline capacity and no-worker semantic recovery. Existing static-image resource and budget-loop regressions remain in that same run. Locked/offline dependencies, one job, disabled debug info/incremental and the existing 3 GiB disk guard were used; CI remains paused.

The linked demo was opened on the cloud Linux desktop. Its native and CPU panels visibly changed beyond frame zero. Global CUA input was required because target-bound X11 button events did not activate the controls. Pausing changed the control to Play. Paused red, green and blue seeks settled to the corresponding numbered/color pixels in native, 50%-alpha group and internal Multiply panels; the animated alpha mask adopted the matching full/left/right footprint. Hide removed all four panels; show restored the paused blue state. Resolver denial removed native and CPU image content with explicit diagnostics, and authorization recovery restored animated content. The test window was closed normally and its disappearance verified.

These are visible asynchronous-upload smoke observations, not synchronized pixel-golden comparisons or frame-time guarantees. CPU output can lag the native playhead while an admitted job finishes. This pass did not exercise the static/animated switch button, live minimize/reduced-motion toggles, playback-rate/finite-loop controls, DPI changes, native resize, live GPU error injection, cross-platform behavior, ignored-media tests or standalone documentation executables. Unit coverage of resource A/B/A and freeze/resume does not replace those unrun native scenarios. No dependency, serialized scene schema or CI setting changed; the GIF is an original repository fixture.

## Same-backend text validation, 2026-10-01

The local Linux checkpoint passed 714 headless tests/doctests (51 CPU-composite fixtures included; 8 existing ignored tests remained ignored), 76 UI library tests, 8 GPUI-enabled font/measurement unit tests and the text doctest, workspace/all-targets strict Clippy, formatting, and the `composited_text` native build. Counts describe each aggregate, not a sum of repeated runs. The headless default-members suite does not enable the optional GPUI text feature; its eight tests were run separately. Locked/offline dependencies, the existing single target, one build job, disabled debug/incremental and the 3 GiB disk guard were used. No CI configuration or dependency version changed.

The `text_raster_probe` used 258 installed font families and exercised Latin ligatures/combining marks, mixed CJK/Latin, ZWJ/color emoji, Arabic/Hebrew, a deliberately missing family with native fallback, automatic wrapping, explicit/empty/trailing lines, spaces and empty content under all three alignments. CJK and the other nonempty fixture runs reported no missing-glyph IDs; installed color emoji produced real non-gray pixels. Empty/space-only fixtures contained no ink. Row/glyph counts matched GPUI's own `WrappedLine` results. A real single-glyph grayscale reference from the exact retained platform backend matched every output pixel at DPR 1, 1.25 and 2. A worker-thread call using the same Arc pair and rejection/recovery after font-epoch invalidation passed. These checks preserve native shaping behavior; they do not independently certify full Unicode, every font, or all color-glyph font formats.

CPU fixtures verify real input alpha holes, one-time group opacity, text alpha-mask silhouettes, internal Multiply against preceding siblings, hidden/empty dependencies, unsupported styles, premultiplication validation and cumulative memory/work failure with no partial output. UI tests cover font/resource generation, wrong-Application identity, bridge withdrawal/restoration, stale completion and actual real-font pixels entering the compositor at fractional scale/DPR (at most one byte of bilinear rounding difference from the producer input).

The demo was opened on the cloud Linux desktop and its target-window screenshots were inspected. Native and CPU panels showed matching Latin/CJK/newline/wrap placement and actual emoji. The bottom panels visibly demonstrated the two-color glyph-alpha “REAL INK” mask and text clipped at a rounded Frame with internal Multiply. The group-alpha control dimmed the complete CPU text panel, including emoji, to 50%. Withdrawing the bridge removed all three affected domains and displayed `TextBackendRequired`, while the ordinary native panel remained. Re-enabling the bridge, invalidating fonts and Undo restored the complete original output after the pending frame settled. Center alignment moved the CPU lines within their original text box without changing wrapping. Target-bound X11 clicks were ineffective, so verified cloud desktop input was used. The demo window was then closed; its disappearance and absence of the demo process were verified.

The native observations are smoke evidence, not screenshot pixel-goldens or latency benchmarks. This pass did not exercise native pan/zoom, window resize, DPI/multi-monitor changes, end alignment via the demo button, close-during-worker/reopen, live GPU failures, CI, macOS/Windows, ignored-media tests or all standalone documentation executables. Web raster calls are explicitly unsupported. Font installation/replacement itself was not performed; epoch invalidation and stale-result handling were tested. Existing GPUI font/shaping caches remain outside Flow's explicit surface budget, and the host must use GPUI's supported font update mechanisms before invalidating consumers.

## Owned-canvas validation, 2026-10-01

Local Linux checks passed: 731 headless tests/doctests (8 pre-existing ignored), 81 UI library tests, workspace/all-targets strict Clippy, workspace formatting, and native `owned_canvas` linking. The CPU compositor total is 68 fixtures, including 17 new owned-canvas fixtures. These counts are aggregate results, not sums of repeated focused runs. The final UI aggregate was rerun after the Clippy-only test-initializer cleanup. The first focused run exposed a one-byte test expectation discrepancy for tiny-skia's 0.5 opacity rounding; the exact expectation was corrected, without changing the blend algorithm. Two example/test formatting lints were corrected before the final strict pass.

New fixtures cover real opaque empty-canvas output, root Multiply/Screen over theme and earlier stable-z roots, Normal/pass-through isolation, single group opacity, alpha masks and unpainted rounded Frames, grid anchoring under pan/zoom, nonzero/fractional canvas and crop origins, fractional DPR crop equivalence, no viewport allocation outside the content mask, node/resource/geometry/work/surface limits, transparent backdrop rejection, missing input and Custom whole-canvas failure, hidden dependencies, offscreen mask-run continuation, offscreen centerlines with visible transformed stroke ink, enormous-DPR dot clipping, semantic theme/grid/mode/viewport invalidation and animation coalescing.

The actual cloud Linux window showed roots blending with the yellow theme and grid, shared-native-backend text, and visibly changing GIF frames. Yellow/white background switching changed blend pixels; removing the grid and switching Multiply to Screen produced the expected changed overlaps. Showing a Custom node removed the complete canvas and reported `UnsupportedShape("custom")`; Undo restored it. Switching to default Domains mode reported `UnsupportedExternalBackdrop` for external roots, and returning to OwnedCanvas recovered the complete output. Dragging moved the selected object while selection/resize overlays remained above it; Undo, wheel pan and Ctrl+wheel zoom kept the recovered pixels and overlays aligned. The window was closed and return to the launching shell verified.

Hard invalidations intentionally hid incompatible output while the next complete frame was computed, so temporary blank canvas/pending states were visible during theme, geometry and mode edits. The animation workload frequently showed `Compositing…` while complete admitted frames continued changing. This is a correctness/bounded-memory slice, not a smooth-frame-rate claim or damage-tracking implementation. Window-resize/DPR coverage in this batch is automated, not a live multi-monitor test. No macOS/Windows, GPU failure injection, screenshot goldens, large-scene performance, standalone font suite, ignored media suite or full old-example interaction sweep was rerun. CI remains paused; commits use `[skip ci]`.

### Artistic blend validation (2026-10-01)

747 headless tests passed (8 existing media tests ignored), 83 UI tests passed,
strict workspace/all-target Clippy and formatting passed, and `owned_canvas`
linked on Linux. New fixtures cover channel equations, half opacity, HSL gamut
clipping, gray/transparent pixels, coverage, owned background, rounded clipping,
external-backdrop rejection, serialization and engine dirty/invalid-value rules.
In the actual cloud Linux window, Overlay and Hue changed the displayed overlap,
Undo selected Exclusion, and Domains rejected its unavailable external backdrop.
Accepted animation refreshes no longer continuously show a loading badge; edits
can still briefly show the explicitly marked stale compatible frame while a
replacement is computed. No smooth-frame-rate, full color management,
Windows/macOS, or arbitrary host-backdrop acceptance is claimed.

## Bounded leaf outer shadows

`Style::shadow: Option<Shadow>` and `PropertyKey::Shadow` are retained document
properties, not SceneView decorations. Legacy documents default to no shadow.
`Shadow::new(x, y, blur, color)` describes one local-space outer drop shadow.
Unknown shadow-object fields are rejected, including `spread`, `box`,
`scaleFixed` and per-shadow blending. This is a bounded subset of the
[Leafer shadow contract](https://www.leaferjs.com/ui/en/reference/UI/shadow.html),
not a Leafer JSON importer or browser-byte-equivalence claim.

Visible vector leaves, Image and producer-backed plain Text with a declared shadow become real
CPU domains even when otherwise native. Vectors use the composed alpha of every
fill and final stroke mesh, including aligned/dashed geometry. Images use the
current decoded frame alpha and text uses real glyph ink, preserving holes. The
filter does not synthesize a shape/layout rectangle. A zero-alpha shadow adds no
pixels and allocates no filter scratch; declaring it still selects the CPU domain
and its usual producer requirements. Zero blur bypasses the filter. Shadow+source are isolated together so node
opacity, artistic blending and inherited antialiased clips each apply once.
The shadow draws behind its source, then the complete object participates in
stable sibling order. If used as an alpha mask, the rendered shadow coverage is
part of that mask's alpha. Missing producers and unsupported required domains
still reject all output; there is no native fallback that loses the effect.

Blur uses the already locked `image` 0.25.10
[`imageops::blur`](https://docs.rs/image/0.25.10/image/imageops/fn.blur.html) on an
A8 plane. This API chooses `sigma = blur * effective_device_scale / 2`; it does
not promise the same kernel as browser Canvas. The library constructs an odd
kernel from sigma (minimum width 3; the pinned implementation's radius is at
most `ceil(4*sigma)`). Source scratch has `ceil(4*sigma) + 2` transparent pixels
on each side before filtering, so its clamp padding cannot extend opaque edge
pixels. Fractional offsets reuse image's bilinear A8 sampler before tinting and an integer
blit. The locked tiny-skia translate-only pattern fast path rounds fractional
offsets to nearest even when Bilinear is requested, so that path is not used for
subpixel shifting.
The source scratch is not clipped to the viewport or ancestor before filtering:
ink just outside either region can cast an in-bounds shadow. The finished result
is clipped afterward. Source ink plus halo must itself fit the limits; a huge
mostly offscreen source can therefore reject rather than being tiled.

Document inputs require finite offsets within ±4096 and blur in [0,256]. The
renderer additionally permits device sigma 0 or [0.001,64], device offsets within
±8192, and positive similarity transforms (translation, rotation, uniform scale).
Reflections, skew and non-uniform scale reject explicitly. Text now uses the [affine glyph producer](text-affine.md), while these
shadow-specific positive-similarity restrictions still apply. Image,
font and viewport/DPR keys still apply. Changes to any shadow field invalidate
stale placement conservatively. There is no hidden resolution reduction.

`DisplayObject::effect_bounds()` is a conservative local visual extent;
`SceneEngine::effect_bounds(id)` is the cached conservative world subtree extent,
including descendant effects and ancestor-container child restrictions. They are
separate from `paint_bounds`, `local_bounds`, `world_bounds`, layout
`ItemBox::Stroke`, geometric picking and selection boxes. Shadow-only edits are
PAINT dirty, update old/new visual damage, do not run layout providers and are
restored by ordinary SceneEditor Undo. Visual bounds do not imply pixel picking
and may include transparent ink holes or hidden descendants. Device AA support
is handled separately by the compositor's three-pixel output guard.

Before any shadow scratch allocation or filter call, the worker admits its RGBA
source, A8 input/output, known float intermediates, both possible image filter
scratch paths, row/padding/kernel/reference tables and conservative convolution
work. The existing 4096-axis / 4,194,304-pixel scratch limit, 128 MiB explicit
working-set limit and cumulative 32-million work limit apply, including active
outer layers, inputs and retained outputs. The accounting deliberately
overestimates library scratch. The image library's internal allocations are not
fallible APIs, so the budget is admission control, not an OS-allocation-failure
recovery guarantee. Cancellation is checked before/after the bounded Gaussian
and per extraction/tint row; the library call itself is not interruptible. Large
blur, zoom/DPR or combined scenes can fail with `TooLarge`, `WorkingSetLimit` or
`WorkLimit`, leaving no partial published frame.

Group, Box and Frame now support [bounded composed-subtree shadows](group-shadows.md),
including child effects and explicit PassThrough rejection. Custom shadows remain
`UnsupportedShadow`. The validation checkpoint below records the earlier leaf-only slice.
[Bounded multiple outer shadows](multiple-shadows.md) now extend the base property.
Spread, box-only knockout, per-shadow blend,
scale-fixed shadows and general filter graphs remain unsupported; bounded [foreground blur](foreground-blur.md) and [grayscale](grayscale.md) are separate node effects. The native example
is `cargo run --locked --offline -p flow-studio --bin outer_shadows`.

## Outer-shadow validation, 2026-10-01

The local Linux checks passed 786 headless tests/doctests (95 compositor tests,
including 17 shadow fixtures; 8 existing ignored tests remain ignored), 86 UI
library tests, workspace/all-targets strict Clippy, formatting and the linked
`outer_shadows` example. Tests cover alpha holes for images and supplied text ink,
zero blur/alpha, aligned hollow stroke silhouettes, layered alpha, opacity once,
offset/rotation, equivalent blur under zoom/DPR/object scale, offscreen source
sampling, rounded ancestor and fractional-DPR child clipping, owned-backdrop
blending, alpha masks, failure budgets, cancellation, cache placement,
serialization, atomic properties, visual damage versus geometry, layout-provider
invalidation and Undo pixel restoration. These are aggregate counts, not sums of
repeated focused runs. The existing image version remains 0.25.10; this crate now
pins that exact version because its filter allocation accounting was audited
against that source. Locked/offline builds used the shared target, one job,
disabled incremental/debug info and the existing 3 GiB disk guard. CI was not run.

Two implementation defects were caught and fixed before completion: repeated
coverage at antialiased parent clip edges when source and shadow were painted
separately, and tiny-skia's nearest-only translate fast path for fractional
shadow offsets. Dedicated exact-pixel regressions cover both.

The native cloud Linux window showed actual vector+aligned-stroke, transparent
SVG image and native glyph-alpha shadows. Blur 6→0 changed a soft edge to a hard
one; flipping the offset and 50% object opacity changed the complete effect;
three Undo actions restored the original vector pixels. Rounded Frame clipping
could be disabled/restored to reveal/remove child-shadow overflow. A non-uniform
vector transform explicitly removed that domain with `UnsupportedTransform`,
and restore recovered it. Withdrawing the text bridge removed text+shadow with
`TextBackendRequired`, and reconnecting restored both. Clicking shadow-only ink
did not select the source; clicking the source selected only its geometric box.
Dragging eventually moved source+shadow together, and wheel pan / Ctrl+wheel
zoom retained image/text/native-label/effect alignment. The window was closed
normally, its disappearance verified and the process reported exit code 0.
Only exact target-window screenshots were used for verification. CUA's bound
X11 click did not activate the custom controls; focused native input did.

The native checks are visible smoke observations, not screenshot-golden or
frame-rate evidence; worker output can appear asynchronously after interaction.
This pass did not verify reopening/close-during-work, live DPI or monitor changes,
GPU-failure injection, an animated shadow demo, other platforms, every existing
example, ignored media tests or standalone documentation executables. Synthetic
text-alpha fixtures and the native glyph example do not add a new typography
certification. All earlier unsupported-shadow and resource/budget boundaries
still apply.


## Container paint-order validation, 2026-10-01

The final local Linux aggregate exited 0: 807 headless tests/doctests passed
(8 pre-existing ignored), 90 UI library tests passed, workspace/all-targets and
documentation-example strict Clippy passed, executable documentation tests and
workspace/docs format checks passed, and the native `composited_containers`
build passed. These are final-run counts, not summed repeated runs. The
`container-phase-final` run record reports a minimum 8,096,276,480 available disk
bytes; commands used locked/offline dependencies.

Coverage includes preserved JSON/factory defaults, enum validation, structural
picking and stable identities, Undo/Redo, native phase/layer ordering, nested
whole-domain substitution, CPU overlap pixels, transparent/multiple paint layers,
single opacity, masks, blend backdrops, inherited/child clips, fractional DPR,
own-stroke crop/culling and budget rollback. A dedicated controller regression
verifies root/nested order-only edits withdraw accepted stale output and reject
a successfully rendered obsolete completion before accepting fresh pixels.

Target-window screenshots on the cloud Linux desktop verified both paint orders
in a native Box and CPU Frame, Undo, native-to-CPU opacity promotion and return,
Frame clipping/alpha, Multiply/Screen, pan/zoom and repeated toggles. Normal
close returned exit code 0 with an empty native log. This is visible smoke
evidence, not screenshot-golden or frame-rate evidence. Reopening was not run
in this batch; close-during-worker behavior remains unverified. CI,
macOS/Windows, live multi-monitor/DPR changes, live GPU-failure injection, the
ignored tests and a full old-example interaction sweep were not run. See the
[complete paint-order validation record](container-paint-order.md#validation-status)
for the test seams and unchanged limits. Earlier checkpoints above remain
historical and unchanged.


当前新增[有界叶节点多层内阴影](inner-shadows.md)。支持范围、真实paint alpha与Leafer几何shape的差别、fill/inner/stroke顺序、完整有界文字源和拒绝规则见该契约；此前阶段的内阴影缺项记录保留为历史验证范围。

Current [bounded container inner shadows](container-inner-shadows.md) extend the
leaf slice to Group/Box/Frame, preserving explicit paint phases, actual subtree
alpha, independent own effects and existing budgets. See that contract for the
current validation scope; older slice receipts above remain historical.

## State presentation admission and atomic publication

For a SceneView with registered state styles, state ticks now use the coherent
whole-presentation gate described in [state-geometry.md](state-geometry.md#coherent-asynchronous-cpu-presentation-follow-up).
Unlike arbitrary canonical geometry edits, these runtime-only samples may retain
an older placement only together with its complete native paint and matching hit
geometry. One admitted CPU batch finishes without per-vsync cancellation, all
outputs receive actual paint-phase atlas uploads under an explicit zero-opacity parent, then the
whole scene presentation swaps at a render boundary. Canonical/resource/viewport
and ownership changes still invalidate immediately. The default non-state path
and its conservative stale-frame rules remain unchanged.

Cancellation after upload additionally tracks warmed-but-unpublished images until
safe retirement. Those images count against retained surface bytes, and only one
obsolete uploaded batch may block work; it cannot accumulate behind new workers.
Batch publication reserves retirement capacity for all replaced/removed domains
before changing any accepted domain. Upload errors keep the previous coherent
presentation when possible and remain explicit diagnostics.

The gate also retains successful empty/offscreen results for unchanged domains,
so native-only state changes do not wait for a CPU image that intentionally does
not exist. A common subtree guard revalidates actual layout/clip/DPR in prepaint
and paint, covering native and custom children along with CPU domains. Accepted
animated-image samples supply alpha hits while a newer CPU batch is pending.
Upload preflight uses a zero-opacity Div scope with positive image bounds, rather
than a zero-size clip which may round outwards to one device pixel.
