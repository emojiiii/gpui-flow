# Bounded foreground-blur native fixture

This fixture was integrated from a separately retained unrun author draft. The explicit Studio feature graph passed strict checking and native linking; the headless and canonical CPU helpers then exited 0 using a frozen executable with its SHA-256 checked before and after each run. Physical acceptance was **not run** at that source checkpoint. The later [12-row native gate](foreground-blur-native.md) completed with normal exit; its original observations, limits and audit corrections are recorded separately. The [summary](validation/2026-10-03-foreground-blur-fixture-summary.json) records the exact source/binary provenance and intermediate failures.

Source: `examples/studio/src/bin/foreground_blur.rs`. It reuses existing Studio dependencies and public host/editor/input APIs, with no new font/resource/dependency or budget increase. Runtime source is [5d735c76](https://github.com/emojiiii/gpui-flow/commit/5d735c76cff47a0996c6757d5bb1eba80c924286); large cancelling translations and offscreen parent-shadow ownership were repaired and validated there. The new fixture and its physical gate are separate from that runtime validation.

## Effect contract and bounded deviation

`Style.blur` is a finite scalar in `[0,256]`, default zero. Signed zero bypasses the filter. Positive physical blur accepts only the checked positive similarity transform domain; its sigma is local blur times world/camera/DPR similarity scale, with no shadow half-factor. Admitted physical sigma is `[.001,64]`. Model validity does not guarantee admission at every zoom/DPR. No clamp or automatic downscale is promised.

Rendering forms the complete padded source, including own phase-ordered shadows and complete descendants, then applies foreground blur, own grayscale, and finally local opacity/blend/Pixel and inherited clip. Own shadows omit their own node's filters while their source includes finished descendant filters. Frame child clipping happens inside source formation. An inherited clip crops the finished filter. Alpha masks use blurred alpha; Pixel erasers use softened destination-out alpha. Path erasers keep the geometry-only branch and ignore valid retained blur.

The locked image 0.25.10 foreground RGBA one-tap ring-kernel defect has a deliberately narrow correction: return the unchanged source only when the adjacent Gaussian f32 weight computes to exactly zero. Positive nonzero weights are not replaced with an identity, and the source/isolation/conservative admission rules still apply. The tiny `.001` oracle exercises that bounded deviation. Neither this fixture nor the implementation promises general upstream byte parity, CSS/browser equivalence, or an executed Leafer blur correspondence.

All calls use unchanged `CompositeLimits::default()`: 32,000,000 work units per request, 4,194,304 pixels per surface, 128 MiB tracked live pixel storage, 4096 per axis, eight domains, and the existing depth/node/path/mask limits. Tracked storage excludes existing documented backend/private/process/GPU costs. Failures must be retained and investigated. Never raise limits or loosen numeric tolerance to turn this fixture green.

## Five deliberately small presets

Shapes are approximately 6–28 world units with blur .5–1. They are intentionally small to make the four DPR 1/2 × zoom 1/2 combinations feasible with the full repeated-source cost. Native zoom 2 is the preferred visual inspection view; tiny screenshot fringes are not a numeric oracle.

| Preset | Source and discriminating behavior | Blur controls | Variant |
| --- | --- | --- | --- |
| A Leaves | Red reference at x30; blurred red at x60; filtered red at x90 with own magenta outer and blue inner shadow, grayscale 1 and opacity .5. Right complete result becomes gray after blur and opacity applies once | Color and focus leaves, 0/.5/1 | Focus opacity .5/1 |
| B Offscreen | Root red rectangle lies entirely left of viewport. A truly paintless Frame at (80,35), 20×20, contains red child x=-7..-1, y=4..12, blur 1. Only the child's completed halo enters its clip; the parent casts that entering coverage in green at offset (+6,0) | Edge and Frame child | Move root edge source inside/restore outside |
| C Nested | Fractional rounded ancestor Frame, small parent Frame with blue fill, magenta stroke, green outer shadow and blur .5; overflowing red child blur .5. Own child clip and inherited clip exercise different stages | Parent and child together | Parent Frame/Box, preserving its style and children |
| D Alpha | Three small Normal parents: blue half-alpha blurred mask over red, blue half-alpha Pixel source over red, and Path source. Green later children survive in the Pixel/Path columns | Mask/Pixel/Path 0/.5/1 | Path blur only, 1/0, leaves all rendered bytes unchanged |
| E Reject | Earlier red root; full-opacity PassThrough blur-1 group with a blue Multiply descendant; later magenta root; actual OwnedCanvas mode | Focus group | Explicit Normal recovery/PassThrough rejection |

B has no Frame fill, stroke, or internal guide. No nonempty own paint can conceal the offscreen-child source bug. CPU probes require positive red at (80,43) and a positive green projection at (86,43) with the same alpha, alongside root viewport-entry alpha at (0,44). A separate root guide could be added only if it does not enter the tested source; this draft includes none.

A has two active domains, B two, C one, D three, E one OwnedCanvas on valid recovery. These are source expectations, not measured runtime acceptance. The helper audits the actual public planner and performs real default-budget raster requests before claiming its own bounded CPU result.

## Public host, history, and input

Each preset builds a new SceneEngine and readonly SceneView, then replaces observation and input subscriptions. It never routes LoadScene through readonly `set_scene().unwrap()`. Trusted host edits use `editor.transaction`. Undo/Redo exercise real SceneEditor history and compare full canonical authored JSON. Eight hidden/zero-opacity invalid scalar attempts must reject atomically without changing revision, source, or immediate public state. Navigation is a real host viewport command and must keep authored source/revision unchanged.

Actual clicks are recorded from SceneViewEvent::InputTrace and the PointerDown operation. Each record keeps the actual traced viewport and routes, router errors, public strict queries, and adapter-tolerant queries. The adapter tolerance is four logical screen pixels divided by traced zoom; a nearby geometric tolerance hit is not an enlarged blur hit target. Strict halo probes outside geometric bounds must remain empty, and blur-zero/positive geometric bounds and strict-hit signatures must agree. A native halo click can legitimately route to nearby geometry within the adapter tolerance; do not mislabel that as halo picking or silently demand a different routing policy.

A burst schedules at most four frame callbacks and ends at blur 1. Token retirement happens before every preset, direct blur edit, history, invalid-input, variant, or navigation change. Callbacks compare the token before changing queue flags, source, history, or logs. The matrix allows two bursts, at most eight callbacks in total. Actual remaining callback count is logged; a completed burst observed before preset replacement cannot establish cancellation timing.

Every observation records full canonical source, source fingerprint, engine revision, entity identity, fixture token, actual viewport/window DPR, public pending/presented state, public geometry effect values, complete composite diagnostics, render diagnostics, and sticky interaction error. `presented_geometry()` may take a canonical early-return path. Its availability, even with matching source and pending=false, **does not prove private CPU upload generation or a GPU fence**. Physical visuals and public correlation are recorded with that limitation; controller/runtime tests must prove exact generation/obsolete-upload behavior.

## Helpers and separate CPU artifacts

`--headless-tour` returns before constructing GPUI Application. It checks five public builders/planners, exact unsafe PassThrough rejection inside Domains and OwnedCanvas, explicit Normal recovery, readonly host denial, trusted amount edit/history, invalid retained scalar atomicity, and navigation invariance. It also compares geometric bounds/strict queries with positive versus zero blur for A and D.

For each safe/default preset and E Normal recovery, it executes four public raster requests at zoom 1/2 and DPR 1/2 using the native 440×220 logical canvas. A–D use their actual public scene_units Domains, and E uses an owned canvas with the native background. Each request shares one default PixelBudget across domains, retains all output frames together, records cumulative work/peak/retained bytes, then discards every frame and requires retained bytes to return to zero. Expected-visible missing output or any admission error fails the helper, preserving diagnostics. This CPU schedule is independent of native accepted/pending buffer retention and cannot certify native peak memory.

The helper also requires typed failures for sigma below .001, above 64, and a nonuniform camera. Those are CPU admission checks; no physical transform-limit gate is claimed. Actual domain premultiplied RGBA bytes are written as `.premul-rgba` with dimensions, logical placement, DPR/zoom and samples. Native direct paints are not part of individual Domain frame files.

`--cpu-tour` produces six actual 180×110 canonical scene PNG exports: A/B/C/D defaults, D with Path blur zero, and E Normal. PNGs are decoded before their straight-alpha samples/dimensions/fingerprints are recorded. Unsafe E must fail exactly with `UnsupportedBlurBackdrop("focus")`, without a PNG and without source mutation.

Separate tiny raw-premultiplied oracles use public capture_subtree/rasterize against an independent scalar f64 separable convolution with transparent exterior. Sigmas are .001/.5/1/2; source is half-alpha red with half-alpha blue overlap. Each sigma is checked both plain and with grayscale then half opacity. The documented allowance is at most one byte for the plain f64 versus production f32 Gaussian, and two for the additional quantized grayscale/opacity chain. Alpha/RGB premultiplication invariants must hold. These allowances are fixed in advance; no auto-widening exists.

Additional CPU checks require:

- B: nonzero root halo inside viewport, positive red child halo at (80,43), and green own-parent projection at (86,43) with identical positive alpha
- D: partial mask alpha outside mask geometry, partial Pixel erasure outside Pixel geometry, hard transparent Path interior, fully opaque red just outside Path geometry; changing only Path blur 1→0 leaves complete decoded PNG bytes identical
- Canonical CPU crop equivalence: full and tight rendering of B edge source, B paintless Frame source, and C ancestor subtree yield identical premultiplied bytes inside each tight viewport, with nonzero alpha. All outputs use the same immutable source and return budget retention to zero

No helper calls the production blur helper as its numeric oracle. No helper substitutes native pixels or claims that decoded PNG bytes are premultiplied. CPU-tour success would still be separate from production automated tests and physical QA.

## Finite physical matrix: 12 required rows

All rows need operator verdicts, saved public records, and original target-window captures at the indicated checkpoints. Preserve failures/inconclusive states. On intended success, observe settled pending=false plus current source and diagnostics; do not infer a private upload proof from public geometry. If an expected settle does not occur in a reasonable observation window, record and investigate rather than looping indefinitely.

The proposed window is 1060×600 logical pixels. Three 36-pixel control rows, a fixed 440×220 canvas, and 215-pixel footer total 543 pixels. The operator must confirm the full footer/controls fit; source sizing is not a native layout pass.

| Row | Required physical procedure and observations |
| --- | --- |
| 01 | Fresh A, zoom 1. Record actual window DPR/viewport and full-footer fit. Inspect reference/color/gray-opacity triplet. Click the color leaf interior; preserve actual InputTrace and strict/tolerant results |
| 02 | A zoom 2. Inspect visible leaf halo and gray own colored-shadow result at usable scale. Record strict halo probes. Click one halo-adjacent point, then Blur 0 and the same screen point; distinguish geometric pointer tolerance from blur coverage. Restore Blur 1 and Record |
| 03 | A Blur .5, Undo, Redo, with settled observations. Run Invalid model once: eight hidden/zero-opacity attempts must reject atomically. Undo/Redo must still restore the one safe edit exactly |
| 04 | A Variant opacity 1, inspect right-hand once-only opacity change and unchanged middle reference; Undo to .5. Home 1 then Zoom 2; source and engine revision stay unchanged under navigation. Record |
| 05 | Fresh A, run one four-frame burst to completion, final blur 1 and settled current presentation. Start a second burst and promptly select fresh B. Record old/new entity/token and actual retired remaining count. Preserve cancellation timing as unobserved if already completed; do not repeat to manufacture timing |
| 06 | B Zoom 2. Inspect red root halo at viewport left edge and red child halo entering the paintless Frame. Inspect its green parent shadow six units to the right. Record exact source with fill/stroke absent and current diagnostics. Tiny JPG halos remain visual evidence only |
| 07 | B Variant brings root source inside, Undo returns it outside. Blur 0 removes entering filtered coverage and its derived parent-shadow projection, Undo restores. Record each settled checkpoint; no unrelated Frame paint may remain as a substitute |
| 08 | Fresh C, Zoom 2. Inspect nested child/parent filter, own stroke/shadow, and rounded inherited crop. Click child interior. Variant switches Frame to Box so child overflow contributes before parent blur; Undo restores Frame. Record both states; use CPU crop audit for exact crop bytes |
| 09 | Fresh D, Zoom 2. Inspect soft mask and Pixel edge against hard Path cut; later green in cut columns survives. Click Pixel and Path geometry, which remains eligible geometrically even where paint is removed. Variant toggles only Path blur to zero, then Undo; visible coverage is unchanged |
| 10 | D Blur 0, then Undo to positive blur. Mask/Pixel coverage changes while Path stays geometric; strict hit/bounds signatures stay fixed. Record current source, settled errors, and actual pointer trace |
| 11 | Fresh E. Preserve exact UnsupportedBlurBackdrop diagnostics and actual pending/presented state. Retained source is valid but OwnedCanvas rendering rejects. Public geometry availability or unfiltered content does not count as success |
| 12 | E Variant to Normal. Inspect blue recovered child over earlier red and unchanged later magenta. Click child and Record. Undo restores typed rejection; Redo restores valid presentation. After final record, close the target window normally once; QA retains the external process exit receipt |

One physical invocation at the actual reported DPR is the gate this matrix describes. Synthetic helper DPR 1/2 is not evidence of two physical display configurations. No optional reopen is included; a requested reopen would need its own separate invocation and exclusive logs.

## Exclusive evidence, source identity, and scope

The parent supplies an existing absolute `FLOW_FOREGROUND_BLUR_EVIDENCE_DIR`. The program makes one unique PID/nanosecond session directory with exclusive create_dir. Every output uses create_new. It never chooses repository-relative output or overwrites existing evidence. Evidence is capped at 8 MiB/program session and 192 numbered events. A failure is visible, preserves partial files, and exits helper mode nonzero. Events print to stdout before file writing; QA must match printed events to actual numbered saved files and retain write failures. A printed event alone is not retained-evidence success.

Each session includes the exact compile-time fixture source bytes via include_bytes, its FNV-1a fingerprint, and a separately labeled optional caller `FLOW_FOREGROUND_BLUR_SOURCE_REVISION`. The parent must freeze and record complete integrated source/diff and executable SHA-256 externally. A claimed revision environment variable and a fixture-only source fingerprint cannot substitute for that freeze. Source code can change during this drafting phase; only the final integrated pin is eligible for a gate.

QA manages raw target-window screenshots and shell logs separately. If screenshots arrive as JPEG, preserve the original JPEG encoding and name. Do not rename to PNG, claim framebuffer capture, compare its lossy channels to raw-premul bytes, or fabricate native RGBA fingerprints. Program CPU PNGs, raw-premul files, and native screenshots remain distinct. Keep a parent-managed combined evidence budget around 30 MiB, but claim its measured size only after auditing all external files. The program's 8 MiB cap does not include operator images or shell logs.

No Image, Canvas, or native Text producer case is in this physical fixture. No font bridge, new font, dependency, resource, GPU readback, native framebuffer, browser golden, performance or cross-platform claim is introduced. Automated producer, parameter-boundary, model/schema/interchange, export-format, exact-budget, cancellation, damage, large-translation and publication-controller tests remain separate and must actually execute before any pass is claimed. APP-04 remains partial; this fixture addresses only the bounded foreground increment.


## Integrated prevalidation provenance

The 529-file conservative code/assets/Cargo inventory was unchanged through final strict, link and helper calls: `61413cb6eef31855db361366a93376d42b587d3e8c5e3c33931f87d56f604fd1`. It is not a compiler dependency graph. The frozen executable is 180,366,392 bytes, SHA-256 `164a5130ff32d177f49a1ac382ff175fffa13bb1046c57feb7c69a89d3ce9beb`. Both final helper receipts bind that value before/after running the immutable copy.

The first integration strict check failed after converting constant RGBA chunks to the toolchain's fixed-array API without adding `.iter()` before `.all()`. Only the new fixture was corrected; the failed source/hash/log remain. Final strict then passed. The original helper calls did not capture the executable hash at execution, so their original receipts are not retroactively strengthened. After copying and hashing the executable, both inexpensive helpers were repeated with before/after executable checks. All 56 actual premultiplied/PNG payloads were byte-identical to their original counterparts; both batches are retained with distinct sessions.

Twenty default-budget raster requests cover five successful presets/recovery scenes at DPR1/2 × zoom1/2. The observed maximum request work was 10,993,880 and maximum tracked peak was 2,013,424 bytes; each released all retained output after explicit discard. These measurements are specific to the helper schedules, not native request/candidate overlap or whole-process memory. Eight independent convolution oracles, three exact crop comparisons, paintless parent-shadow and mask/eraser checks, six genuine PNG exports and three typed admission rejections succeeded.

Legacy A8 tiny-sigma shadow displacement remains the separate known limitation recorded in the [runtime contract](foreground-blur.md). This fixture's own shadow radii are zero; it does not physically validate that inherited defect. Native acceptance, exact target-window sizes, actual display DPR, original screenshots and saved input traces require the separate finite matrix above.

An [independent artifact audit](foreground-blur-fixture-cpu-audit.md) passed 136 original checks and128 frozen replay/provenance checks. It independently decoded all actual pixels and reimplemented the numerical reference; the source/runtime tests and physical gate remain separately scoped.
