# Bounded vector and static-image compositing in SceneView

Status: bounded vector and static-image implementation, locally validated on Linux. The vector native-window smoke test below was recorded on 2026-09-30; static-image validation is reported separately. This is a bounded Leafer-inspired slice, not complete Leafer or GPU-backdrop compatibility. The first headless checkpoint is [9bd37d4](https://github.com/emojiiii/gpui-flow/commit/9bd37d479cf546e0d58b123465a2df07cf13bc3c); the subsequent integration adds the production SceneView consumer.

## Actual rendering path

`SceneStateStyles::presentation_at` produces one presented Scene. The adapter derives both native objects and ordered compositing boundaries from that same tree. Each supported domain replaces its entire native subtree. It is never drawn alongside flattened copies of its children.

The new headless `gpui-flow-composite` crate directly calls locked tiny-skia 0.11.4 for path filling, gradients, premultiplied RGBA surfaces, alpha masks and SourceOver/Multiply/Screen. Shared `Shape::stroke_mesh` supplies the existing Kurbo/Lyon centered stroke geometry; consistently wound triangles form one compound fill so shared edges do not repeatedly apply alpha. The production worker completes the whole domain before publishing bytes.

At the GPUI boundary, those bytes pass once through `swap_rgba_pa_to_bgra` into a real `RenderImage`. `Window::paint_image` uploads the candidate to the atlas and inserts the native sprite. Only a successful call accepts the candidate. This is not GPUI `paint_layer`, a backend trait without a producer, or a screenshot/readback workaround. The locked GPUI conversion can truncate an unpremultiplied channel by one byte; low-alpha conversion is not lossless.

## Public model and supported semantics

- `Style.blend_mode`: `PassThrough` (JSON `pass_through`, legacy default), `Normal`, `Multiply`, `Screen`
- `DisplayObject.mask`: `None` (legacy default), `Alpha`; a mask must be a direct Group child
- Engine `PropertyKey::BlendMode` marks PAINT; `PropertyKey::Mask` marks PAINT | GEOMETRY, without redefining picking
- Group opacity and multi-paint primitive opacity apply once after the object's complete contents, including overlaps
- Normal groups isolate; pass-through groups reuse their actual CPU parent. Internal multiply/screen blend against preceding pixels in the same domain
- Alpha masks are hidden and affect following stable z-ordered siblings until another mask. Opaque-mask independent blends flush against the CPU parent; translucent-mask runs composite first, then apply alpha once
- Rect, rounded Rect, Ellipse, Line, Polygon, Star, Path, Pen and nested Group are supported, including current fills/gradients and centered stroke meshes
- Static Image uses decoded pixel dimensions for Contain/Cover/Fill, with bilinear sampling, layout-box clipping for Cover, full affine placement, image alpha masks and the same group/blend/clip path as vectors
- Rounded Box/Frame clips inherited from outside the domain are applied to the finished output; transforms are baked into physical pixels

Missing JSON fields retain old defaults. Rust `Style` and `DisplayObject` exhaustive literals need the new fields or constructor/default syntax; new enum variants may require updating exhaustive matches. This is backward JSON reading, not Leafer JSON import compatibility or compatibility with older renderers.

## Static decoded-image input

SceneView reuses its existing authorized resource resolver and GPUI public asset/cache subscription. A current resource ticket and `RenderImage` identity enter the immutable request key. The background worker accepts exactly one decoded frame, converts the pinned GPUI straight-BGRA contract to premultiplied RGBA once, and uses tiny-skia's bilinear image pattern. There is no new downloader, decoder, image cache or animation clock.

- Contain, Cover and Fill use decoded dimensions, matching GPUI; `natural_size` does not override real decoded aspect. Cover crops to the image layout rectangle
- Image pixels participate in full affine transforms, group/local opacity, internal Normal/Multiply/Screen, image or vector alpha masks, and inherited rounded clips
- Image style fills/strokes remain ignored, consistent with the native image path
- Pending, failed, cancelled, denied, malformed, oversized or animated inputs reject the whole affected domain. Hidden/zero-opacity images and ancestors do not require pixels; hidden masks retain their zero-alpha masking semantics
- Source, resource generation, resolver/cache authorization, detach/reattach and document changes invalidate old image pixels. A/B/A requests and cancelled workers cannot publish obsolete results
- Shared decoded images convert once per worker request; converted copies have a 32 MiB batch cap and count against the explicit working set during each raster operation
- Working-set failures retry only after real adapter-owned bytes are released. Current accepted/candidate successes and terminal failures survive these budget-only retries; successful domains cannot manufacture a repeating retirement/upload loop. Dropped over-cap outputs release their byte charge without resetting cumulative work

The static-image example is `cargo run --locked --offline -p flow-studio --bin composited_images`. It provides SVG/PNG, missing/restore, deny/authorize, Multiply/Screen and Undo controls. Text labels remain native outside the domains. Static SVGs are rasterized by GPUI before this adapter receives pixels; multi-frame images are rejected regardless of file extension.

The headless producer accepts `StaticImage::from_straight_bgra`, `ImageInputs` keyed by display ID, and `rasterize_with_images`. It validates dimensions and byte length before allocating the premultiplied RGBA copy; zero-frame inputs return `InvalidImage`, multiframe inputs return `UnsupportedAnimatedImage`. The default resource resolver permits local paths only; a host must explicitly authorize a URI. Host resource completion remains separate from successful composition/upload.

## Honest boundaries

A Text, Custom, Box or Frame inside a required domain rejects the entire domain. Static Image nodes now have a real pixel producer; multiframe animated images remain explicitly unsupported. The view shows a diagnostic; it does not silently omit the unsupported descendant or fall back to individually faded native children. In particular, opacity on a Box/Frame rejects that necessary parent boundary instead of incorrectly isolating only a Group beneath it. Ordinary text/image/custom siblings outside domains retain their native route.

A domain root that needs multiply/screen against external content returns `UnsupportedExternalBackdrop`. A mask alone does not isolate a pass-through opacity-1 Group: a blend escaping an opaque mask run is rejected too. Explicit Normal groups and opacity isolation are supported, as are translucent mask runs that genuinely isolate their content. Put a required vector background inside the same Normal Group. Native siblings, host background, grid and selection decorations are not captured.

No text or animated-image raster producer, eraser, shadows, blur/filter, grayscale/path/clipping masks, arbitrary blend modes, inside/outside or scale-fixed stroke, pixel picking, automatic resolution reduction or tiling is claimed. The engine's hit testing and conservative selection/layout bounds still include geometry beyond transparent mask pixels. Alpha-mask bounds do not become Leafer-style layout bounds. Rounded contours and stroke expansion retain the existing 0.1 local-unit approximation; gradient/AA bytes need not match the older native renderer. There is no wide-gamut, ICC or linear-light guarantee.

## Budget, invalidation and lifecycle

Per request: at most 8 domain boundaries; each domain has at most 512 nodes, 16 tree levels, 64 mask runs, 65,536 source commands/points per shape, 262,144 total source commands/points and 262,144 generated stroke triangles. Gradients have at most 256 stops; paints retain the existing 32-layer limit. Domain snapshots beyond the count limit are rejected before cloning.

A domain and each converted static input are at most 4096 pixels per axis and 4,194,304 pixels total. Adapter-owned converted pixels, explicit CPU surfaces, accepted/candidate images and retired images share a 128 MiB ceiling. GPUI/cache-owned decoded originals, including originals kept alive by request keys, are outside this adapter budget; it is not a total process-memory bound. SceneView additionally limits converted static input copies to 32 MiB, each accepted/candidate output batch to 32 MiB, and retirement to 32 MiB. Identical GPUI ImageIds share one converted input per worker request. Discarding an over-cap output releases its surface charge before considering later domains. Capacity is checked immediately before upload using the live state after earlier domain paints. One CPU job runs at a time with one latest replacement request; superseded results cannot publish. Cancel is checked between kernel calls, not inside a tiny-skia call.

Returning to an earlier request after supersession invalidates the completion marker even when canonical revision is unchanged. Cleared/unaccepted candidates, prior failed completions and blocked uploads can therefore retry after an A → B → A viewport/presentation sequence; two regressions reproduced the bug before the fix and pass afterward.

A request permits at most 32,000,000 conservative full-surface write pixels across domains. Full-surface accounting can reject a large surface with many tiny paints. Nonfinite/singular matrices, tiny-skia-rejected inverses and control points outside ±1,000,000 device pixels fail explicitly. Dimensions and surface buffers use checked arithmetic and fallible reserve before allocation. Private scan-conversion allocations, geometry snapshots and GPU atlas/driver allocations are not a hard whole-process memory guarantee.

Keys use the complete presented domain values/order, ancestor clips, all relevant ancestor/descendant engine handles and attachment epochs, document epoch/revision, window, viewport/origin/content mask and DPR. There are no cross-view global composite caches. Paint-only presentation changes cannot be hidden behind a canonical revision. Geometry, clips, order, attachment or document changes cannot reuse a stale placement.

A previous complete frame may remain only for conservative paint/opacity-only changes at unchanged placement/identity, accompanied by `StaleFrame`. Upload failures preserve that frame. Incompatible/removed frames are never displayed and retire on the next frame before `drop_image`; retirement pressure delays candidate upload. Release cancels work and drops only this view's owned composite images. A repeated unrecoverable upload is blocked until a new request, rather than spinning indefinitely. A real retired/candidate byte release can retry WorkingSetLimit domains once at the new budget epoch, including a release during a worker run. Successful current-domain outputs/candidates and terminal error diagnostics survive that retry; they are not republished to manufacture another retirement/retry loop.

`SceneView::composite_diagnostics` and `compositing_pending` expose status independently from resource readiness. Resource `Complete` is still its existing resource/paint receipt; it is not redefined as composite success or a GPU fence.

Raster work uses GPUI's background executor, but full Scene validation, snapshot preparation and some shared stroke validation still happen on the UI thread. This slice does not promise a frame-time bound, incremental damage rasterization or large-scene interactive performance.

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

[Leafer blendMode](https://www.leaferjs.com/ui/reference/UI/blendMode.html), [mask](https://www.leaferjs.com/ui/reference/UI/mask.html), [pinned mask implementation](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/mask/src/index.ts), [pinned LeafData isolation](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/data/src/LeafData.ts), [GPUI paint_image](https://github.com/longbridge/gpui-fast/blob/10d005166a9f9e079d55ebfb700035f3860c3e71/crates/gpui/src/window.rs). These sources establish the intended bounded semantics; they do not turn this adapter into full Leafer compatibility.
