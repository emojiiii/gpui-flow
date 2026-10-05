# Integrated bounded shadow-modes fixture and physical contract

> 历史验证记录：旧 Studio 原生演示已于 2026-10-05 移除。本文的源码路径、命令、交互矩阵与结果仅对应当时版本，不是当前可运行入口或新工作台验收。当前入口见[入门](getting-started.md)，测试保留范围见[迁移说明](studio-retirement.md)。

Integrated 2026-10-03. **Final strict compilation, build, formatting and all three helper modes passed. The separately frozen15-row native gate is now recorded in [native acceptance](shadow-modes-native.md).** The original author draft contract (preserved as `author-contract.md` in the evidence package), author Rust draft (preserved as `author-draft.rs` in the evidence package), first compilation failure and first helper failure are preserved unchanged. The parent owns source publication, native execution, independent audit and final package acceptance.

This is the exact contract for `shadow_modes.rs`（历史源码已移除）, SHA-256 `6b625d2935c54ebaf821e3d6b4efea03f1fc8ea476352cbfff423f828d34839c`. Final broad source inventory: 535 files, `bd3da34d1ffd098703070ca03dd93e51fb34c75d23b071d765db73bbf4aa7149`. Final executable `/workspace/shared/rust-target-native/debug/shadow_modes`: `d3341d1be0ee3602277d9ddc1eb2cc36ca37f571770589f7724776b11f70d2cf`. Each helper receipt records its own executable hash before and after, beginning with the very first failed helper 04 rather than a later replay. The different historical hashes remain in their own receipts.

## Actual integration changes and preserved failures

1. Initial strict 01 failed with Rust E0382 because one conditional variant moved `base` before another conditional used it. The integration changed that first use to `base.clone()`; strict 02 and build 03 then passed. This is fixture ownership handling, not a runtime change.
2. Helper 04 exited 1 after 48 saved events. The zero-width Box edit had already rejected atomically with `SceneError::Validation` and an exact node issue; the fixture incorrectly expected `InvalidGeometry`. No matrix request was reached. The corrected fixture requires the exact `Validation` variant, single issue path/message and unchanged canonical source. This correction strengthens specificity and does not bypass a failed model update. Original source, log, receipt and 149,045-byte partial session remain.
3. Strict 05 / build 06 and helpers 07/08/09 passed on the corrected source. Their historical manifest still called the source an unrun draft; its CPU manifest also mislabelled platform-free status. These historical fields are not used as validation authority.
4. The final source corrects those labels, keeps native acceptance separate, and saves the actual dimensions, absolute placement and fingerprints previously returned but omitted from tiny-A8, crop, Text and retained-output records. `final-fixture-delta.patch` shows the complete post-preflight delta. It changes evidence and metadata only: no rendering algorithm, geometry, font, limit or assertion changes. Strict 10/11 and intermediate build 12 are retained; final strict 13 / build 14 and format 18 apply to the final source.

All 534 nonfixture inputs in the broader build 03 inventory are byte-identical to the final inventory. The changed path is only `examples/studio/src/bin/shadow_modes.rs`; no runtime source, Git index or publication was modified by this integration worker. The runtime owner's 499-input manifest remains separately identified by `201e73d2ace7b2d6f9a714f9c1aa79db14deb5c525d33de7d4ad4528205f2747`. Neither inventory claims to be an exact compiler dependency graph.

## Measured final results

- Headless 15: 222/222 default-budget requests, 45 geometry/hit invariance checks, eight atomic invalid hidden/zero-opacity layer edits, exact safe-edit Undo/Redo, exact Domains/OwnedCanvas PassThrough rejection and Normal recovery, exact zero-width Box validation, and three late physical-request failures preserving the same 2,688-byte valid retained frame
- CPU 16: 28/28 independent raw and genuine PNG cases; maximum raw error 1 byte and recovered premultiplied PNG error 1 byte, below the unchanged predeclared allowances. Tiny sigma versus zero is exact, with 39 nonzero lattice pixels and zero independent-reference error. All three broad/tight crop checks pass; all four canonical PNGs and Path-paint-zero PNG exist, with complete decoded Path equality
- Text 17: 48/48 requests using real host glyph inputs. Complete offscreen source alpha sum is 17,971; intentionally pre-cropped input yields no frame and zero alpha. The output record explicitly stores the absent frame as null
- Saved/printed numbered event reconciliation: headless 274, CPU 38, Text 49; all exact. Final session sizes: headless 9,580,173 bytes, CPU 1,220,891 bytes, Text 1,632,480 bytes. Each is below 16 MiB and 384 events
- Maximum platform-free matrix work 6,472,682; peak tracked bytes 590,800; retained output 77,076 bytes. Text maximum work 2,155,000, peak 462,736 bytes, retained output 106,000 bytes, glyph producer work 23,584. All outputs are discarded to tracked zero at request end; all source invariance checks pass. These measurements are not whole-process/GPU memory or timing guarantees
- Minimum recorded free disk across all stages: 23,258,210,304 bytes, above the enforced 3,221,225,472-byte reserve. No limits or tolerances were raised; no font was installed; no native window was opened. Process RSS was not measured

fixture validation summary (historical record in Git history) gives exact outcomes and session paths. The independent auditor's findings remain a separate evidence class. Do not sum preflight and final repeats as different coverage.

## Accepted semantics used by the integrated fixture

Shadow adds finite spread in ±4096 local units, Boolean box and Boolean scale_fixed. Defaults remain 0/false/false. Each field is retained on every layer, including hidden, zero-opacity and transparent layers. Outer box attenuates only its own projected layer with the inverse original, unoffset source alpha. Inner box is retained and inert. Box never substitutes a bounding rectangle or binary alpha occupancy.

With logical camera × ancestry scale `s`, compensation is `f = scale_fixed ? 1 / max(1,s) : 1`. DPR does not enter f. Flow retains its existing transformed-local-vector offset direction. Physical offset is `linear(P) * (x,y) * f`; sigma is `blur * physical_similarity_scale(P) * f / 2`. Positive sigma below .001 or above 64 rejects. The source remains at full logical/device scale.

Spread uses canvas axes around the transformed center of fixed pre-effect reference geometry, with denominators from owner-local reference width/height. Outer factors are `1 + 2*spread*f/W,H`; inner factors are `1 - 2*spread*f/W,H`. Finite negative factors reflect. An exactly zero factor projects zero alpha: outer disappears; inner tints the original source. This singular rule is a declared Flow choice, not browser pixel parity. Nonempty required sources with unusable reference geometry reject; no epsilon, morphology, alpha-bounds pivot, viewport crop, downscale or fallback geometry is permitted.

The fixture's zero-width fixed Box update rejects nonpositive dimensions with `SceneError::Validation([SceneIssue { path: "nodes.plain", message: "rectangle size must be finite and positive" }])` through `Scene::update_object`, before a public composite source exists. This fixture checks that typed model guard and its atomicity. It does not manufacture an unreachable zero-width public Box to claim an executed `CompositeError::InvalidShadowReference`; that internal/planner error remains a separate runtime test obligation.

Complete alpha is sampled before resampling, then Gaussian, then offset sampling. Original alpha remains available for knockout/inner tint. Independent layers sample the same original source. Paint phases, source/clip ownership, geometry/hit behavior and once-only opacity/blend are retained. Group reference includes Box overflow geometry but excludes descendant effects. A paintless Frame uses its declared box even when its only contributing source is a clipped child halo.

## Five small native presets

The logical canvas is 256 × 160 pixels in an intended 1120 × 560 logical-pixel window. Scene geometry generally lies inside 128 × 64 world units; zoom 2 is the inspection view. Three 36-pixel control rows, canvas and 215-pixel footer total 483 logical pixels. Full fit still needs a physical observation. Screenshot fringes are small and do not establish numeric pixels.

| Preset | Deliberate source | Control or variant |
| --- | --- | --- |
| A Leaves | Four 12 × 8 holed vector paths, opaque blue channels with alpha 128. Asymmetric 4 × 4 hole at local (2,2). Pair outer plain/box and inner plain/box. Red effects, offset (2,1), default spread +2 | Spread 0/+2/−8; Collapse uses outer −4 and inner +4; Box flips all flags; Fixed flips all flags; Variant equals Box |
| B Scale | Two 90-degree 12 × 8 paths, free/fixed pairs, plus a child under a 1.25-scale Group source root. Offset (3,−1), blur .5 and spread +2 distinguish cap and canvas axes | Zoom .5/1/2; Fixed or Variant swaps fixed flags. Root transforms are real scene transforms |
| C Containers | Truly paintless 10 × 10 Frame at (12,24), no fill or stroke. Child ink ends at x=11 and blur 1 alone enters its clip; parent green spread/offset projects it. Separate Group contains a 6 × 6 paintless Box with child at x=8..12 and asymmetric red child effect. Small Box and Frame have independent outer and two inner layers plus stroke and overflowing children | Variant flips both Box and Frame between BeforeChildren and StrokeAfterChildren, retaining all other source. No guide paint enters tested sources |
| D Consumers | Half-alpha mask/Pixel/Path columns. Mask and Pixel include shadow and foreground blur; Path retains those effects but uses only geometry. Later green remains. Separate Normal guard Group has a Multiply child over earlier red and later magenta | Variant selects full-opacity PassThrough and must produce UnsupportedShadowBackdrop("guard"); second Variant is explicit Normal recovery |
| E Text | Real DejaVu Sans “Hi”, size 16, 24 × 20 alignment boxes. Visible source at x=48 and fully offscreen source at x=−30, with rightward projection. No fake glyph rectangles | Fixed or Variant toggles cap. Bridge is installed on every fresh SceneView, using the same GpuiTextRasterizer application |

The installed file `/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf` was found by file inventory; no font was copied, downloaded or installed. Actual host glyph production passed both Text helper invocations using the installed native backend. No fake rectangles were used. The complete source produced positive projected alpha; intentional viewport pre-crop removed it. This remains CPU/host-producer evidence, not a native-window inspection or proof of a cross-platform font match.

## Public native host and bounded controls

Every preset creates a fresh SceneEngine and readonly SceneView, resets host undo/redo and pointer text, and replaces observation and InputTrace subscriptions. The shared actual Text bridge is reattached each time. Authored changes use `editor.transaction`; Undo/Redo use real editor history and compare full canonical JSON. Navigation is a real SetViewport command and must preserve source and engine revision.

Invalid model performs exactly eight late transparent-layer edits: spread below −4096, above +4096, NaN and infinity, each while hidden and at opacity zero. Every attempt must retain canonical source, revision and immediate public state. The helper checks the same late-layer cases around one safe edit and one exact Undo/Redo.

Every pointer record comes from an actual PointerDown InputTrace. It keeps traced viewport, actual world/canvas coordinates, complete routed target lists, router errors, strict source/presented candidate lists and tolerant source/presented lists. Adapter tolerance is four logical screen pixels divided by actual traced zoom. A nearby tolerance hit is not a shadow-halo geometric hit. None of these lists may be replaced by only their first candidate.

Every Record keeps exact canonical source, source FNV-1a, engine revision, entity, fixture token, actual window DPR/viewport, full public effect fields and diagnostics. Public `presented_geometry()` may return canonical geometry early; its existence and pending=false do not certify private upload generation, a GPU fence or framebuffer contents. Native visuals/public correlation remain a different evidence class from CPU buffers.

A burst is four frame callbacks and ends at spread +2. Only A permits it, and the invocation allows at most two starts, eight callbacks total. Every source/preset/history/control/navigation change retires the token; callbacks compare it before touching flags, source, history or logs. A completed burst before replacement provides no evidence of cancellation timing. Do not repeat it to manufacture timing. At most 32 host edit-history entries are allowed per preset.

## Executed helpers and exact coverage

Final helper invocations 15, 16 and 17 all exited 0. They used unchanged `CompositeLimits::default()`: 32 million work units per request, 128 MiB tracked pixel storage, 4,194,304 pixels per surface, 4096 per dimension, eight domains, depth 16 and existing geometry caps. There is no tolerance or runtime-budget escalation path.

- `--headless-tour` returns before creating any GPUI Application. It checks model/history, geometry/hit invariance, Domains and OwnedCanvas exact rejection, Normal recovery, typed zero-reference model rejection, and three late invalid physical requests while a valid output stays retained and unchanged. It then actually rasterizes the platform-free state matrix.
- `--cpu-tour` also creates no GPUI Application. It runs independent numerical raw/PNG oracles, exact A8 tiny-sigma comparison, four genuine canonical scene PNGs plus Path-paint-zero PNG, and broad/tight crop comparisons for the paintless Frame, Group overflow and nested source root.
- `--text-cpu-tour` creates a headless native GPUI Application using `GpuiTextRasterizer::application(current_platform(true))`. It opens no window, but **is not pure platform-free headless**. It produces real host TextInputs, measures the Text state matrix, and compares complete offscreen text against intentionally wrong viewport-pre-cropped inputs. The latter must remove the only source of the projected halo, giving zero alpha while complete glyph input gives positive alpha.

Each successful state is reconstructed from the preset baseline independently, never inherited from the prior request. Common states per preset: default, spread 0, spread +2, spread −8, box flipped, fixed flipped, shadow blur zero and shadow blur .004. Additional A states: exact collapse, spread-zero plus blur-zero, spread-zero plus blur-.004. Additional C state: both paint phases BeforeChildren. Additional D state: only Path retained shadow/blur zero. Normal D baseline is the recovery source. Text uses eight common states.

This gives **37 platform-free states × zoom .5/1/2 × DPR 1/2 = 222 actual successful requests**, and **8 Text states × the same six transforms = 48 actual successful requests**. These are measured final-run success counts; the earlier corrected preflight repeated the same schedule separately. Do not add the two invocations into a broader coverage claim. During each request, one default PixelBudget is shared across every actual Domain; all successful frames remain retained until the whole request ends, then explicit discard must return tracked output to zero. Actual work, peak, retained bytes, source, placement, bytes and samples are retained. Text producer work is charged into that request; all TextInputs are accounted by the raster API. Backend font/shaping caches are outside the compositor memory claim.

On a request failure, later domains in that request stop, current frames/diagnostics are retained, outputs are discarded, and the helper proceeds to record other matrix requests before finally failing. A planner/producer/file-write failure may stop earlier and leaves partial files. Never erase the failed run or describe unreached states as passed. If the matrix or evidence cap fails, report the actual source, parameters and diagnostics; do not raise limits. The parent must review any proposed geometry reduction as a new source version with preserved prior failure evidence.

The finite native matrix below uses fresh baselines or explicit Undo before changing to another single variant. Arbitrary control combinations outside that sequence are exploratory and are not covered by these schedule counts.

## Independent arithmetic, raw premultiplication and PNG

Twenty-eight successfully executed numerical cases use a mathematically constructed, blue half-alpha 12 × 8 holed path, not captured alpha from production as a golden. Sixteen cases cover outer/inner, box false/true, zero/+2/−8 and exact collapse. Twelve cover the rotated nonsquare source with blur 1, zoom .5/1/2, DPR 1/2 and fixed false/true.

The reference constructs original A8 ink on an absolute physical-center lattice, applies signed backward bilinear spread around the fixed geometric center, independently computes a finite f64 separable Gaussian with transparent exterior, samples the local-vector offset, applies `(a*b+127)/255` attenuation and composes premultiplied blue/red. It never invokes image::blur or private compositor routines. Each actual raw frame and independent expected full-grid raw array is saved with dimensions/origin. The fixed allowance is one byte for bilinear/f32 half-tie differences when blur is zero, two with the additional f64/f32 Gaussian rounding stage. It is chosen from those arithmetic stages, not observed compiler/runtime output, and must never widen during execution.

Every numerical case also exports a genuine SceneExport PNG and decodes it as **straight-alpha RGBA**. Its dimensions and original encoded bytes are retained. The decoded channels are explicitly premultiplied once to compare to the same independent reference; that conversion adds at most one byte to the raw allowance. This is a separate true-PNG oracle, not an encoding renamed from JPEG and not a production raw frame used as expected pixels. Premultiplied channel ≤ alpha invariants are checked independently.

A separate exact comparison uses shadow blur 0 versus .004 at zoom .5, DPR 1, nominal sigma .001. It compares every physical lattice sample, allows different transparent padded surface dimensions, requires zero difference and positive coverage, and compares to independent sigma-zero mathematics with the predeclared one-byte sampling allowance. It records the unshifted source coordinates. This exercises the intended A8 exact-identity correction, not browser parity. PNG or CPU success cannot establish native display bytes.

The three crop comparisons reuse the same immutable public captured source at broad and tight viewports, retaining both outputs under one default budget. They require exact overlapping premultiplied pixels and positive tight alpha. Path effect-zero versus retained-effect scene exports require exact complete decoded PNG equality. These checks supplement, rather than replace, the independent numerical reference.

## Finite physical matrix: 15 rows, one normal close

The parent assigns the desktop slot. Run one physical invocation at its actual reported DPR only after reviewed strict/build/helper gates. Each numbered row requires operator verdict, saved Record/public receipts and original target-window image(s) at its meaningful checkpoints. Preserve failure/inconclusive outcomes. Settled pending=false with current source and diagnostics is an intended observation, not a private upload proof. If settle fails in a reasonable observation window, record and investigate; do not loop forever or manufacture success.

| Row | Required procedure and observations |
| --- | --- |
| 01 | Fresh A at zoom 1. Record actual DPR/viewport and full control/footer fit. Inspect the four sources, holes and half-alpha overlap. Click plain ink and its hole; retain complete strict/tolerant/routed lists |
| 02 | A Zoom 2. Inspect ordinary outer versus boxed overlap and inner pair. Box, Record, Undo, Record. Inner box must remain visually inert; independent CPU oracles supply exact arithmetic. Click a halo-only geometric probe and distinguish tolerant routing |
| 03 | From A baseline, Spread −8: reflected asymmetric holes, not erosion. Record; Undo; Redo; Undo, each settled. Collapse A: outer projected layer disappears and inner covers original alpha while holes remain. Record, Undo |
| 04 | A Spread 0, Blur 0, Zoom .5, Record; Blur .004, Record at the same source placement. No one-pixel lateral jump is intended. JPEG is only visual evidence; exactness belongs to CPU. Fresh A, make Spread −8, Invalid model, then Undo/Redo/Undo. All eight invalid edits must be atomic and not add history |
| 05 | Fresh A, start first four-frame burst and let it finish at +2; Record. Start the second and promptly select B. Record old/new entity, token and observed remaining steps. If already completed, cancellation timing is unobserved; no extra burst or reopen |
| 06 | Fresh B. Record at zoom .5, 1 and 2. Inspect rotated nonsquare source and nested 1.25-scale source; actual source/revision stay unchanged during navigation. DPR is actual window DPR, not the synthetic matrix |
| 07 | B baseline at zoom 2. Fixed (swaps flags), Record; Undo, Record. Offset/blur/spread parameters cap above logical scale 1; original source geometry remains full-sized. Click nested geometry. The rotated offset keeps Flow's local-vector direction |
| 08 | Fresh C, Zoom 2. Inspect clipped entering red child halo and green parent projection on the left. Record exact Frame fill=None/stroke=None. Inspect Group's Box overflow and descendant asymmetric effect; their capture support must not move the geometric pivot |
| 09 | C baseline: Variant to BeforeChildren; Record both small Box and Frame phases, then Undo. Inspect separate inner layers and outer layer. Click child and parent geometry; retain complete candidates. Do not treat visual disappearance beneath child paint as an absent source |
| 10 | C baseline: Spread −8, Record the finite reflected/container projection; Undo. Spread 0, Record, Undo. Observe stable geometric bounds and source ownership. Exact crop checks remain CPU evidence |
| 11 | Fresh D, Zoom 2. Inspect softened mask and Pixel versus hard Path cut and later green. Click Pixel/Path geometry even where their paint erased destination. Spread 0, Record, Undo. Path retained effect paint must remain inert; complete PNG equality is checked separately |
| 12 | D baseline: Variant to PassThrough. Record exact UnsupportedShadowBackdrop("guard"), source, revision, actual pending/presented state and complete diagnostics. Earlier/later paint or public canonical geometry does not make the rejected guard a success |
| 13 | D Variant to Normal, Record recovery; click guard child. Undo must restore typed rejection, Redo restore source-exact Normal recovery; Record both. These are real editor history steps, not fresh presets |
| 14 | Fresh E at zoom 1 then 2. Inspect actual “Hi” glyph shape/gaps, visible effect and green projection from the fully offscreen text alignment box. Record bridge-bearing fresh entity and errors; click visible text geometry. The font/helper bridge must not be silently unavailable |
| 15 | E baseline: Fixed, Record, Undo; Home 1 then Zoom .5 and Record navigation/source invariance. Final Record must preserve all diagnostics and actual DPR. Close the target window normally **once** and retain external process exit receipt. No optional reopen belongs to this invocation |

No row claims two physical DPR configurations. Synthetic DPR 1/2 helpers are separate. New sessions are permitted only as separately retained invocations assigned by the parent; they never overwrite or strengthen an old session retroactively.

## Evidence and source provenance

Every invocation exclusively creates its own PID/nanosecond session directory below the caller-supplied absolute existing root. Files use create_new. The cap remains 16 MiB per session and 384 numbered events. The final event additions preserve complete raw dimension/origin/FNV metadata without loosening any oracle. The shell runner retains stdout/stderr, exact command, UTC times, exit, source inventory before/after, executable SHA-256 before/after each helper and sampled disk reserve. Existing failed evidence is neither overwritten nor omitted.

`execution-environment-provenance.json` records Rust 1.98.1, the exact feature arguments and hashes of the environment script, runner, inventory script, Cargo.lock, Studio manifest and installed DejaVuSans.ttf. Its selected environment values were collected retrospectively from the same environment script; it is not falsely described as an original full process environment dump. Every build used locked/offline Cargo with one job. The helper binary itself is not included in the nonbinary artifact target.

`artifact-accounting.json` reports actual retained bytes, including historical invocations, logs, source inventories, contracts and validation scripts. Slightly exceeding the approximate 30 MiB uncompressed target is disclosed; all original evidence remains. Parent packaging may losslessly compress the files. This integration performs no Library upload or source publication.

## Remaining acceptance limits

The entire 15-row physical matrix above is still required and unperformed. Actual native DPR, window/control/footer fit, real pointer routing, public pending/presented correlation, burst cancellation timing, host history interaction, Text visuals, target-window screenshots and normal close/exit belong to that separately retained invocation. A completed burst before preset replacement gives no cancellation timing evidence. Public geometry may be canonical and never proves a private upload generation, GPU fence or native framebuffer bytes.

No Leafer/browser golden, browser Gaussian equivalence, singular Canvas parity, cross-platform success, performance guarantee, upload-generation race coverage or missing APP-04 requirement is claimed. Default-budget success does not replace cap-minus-one, adjacent-f32 kernel transition, producer cases outside actual Text, fixed-stroke reference internals or stale-publication runtime tests. Native APP-04 acceptance and owned-background-blur work remain partial.

## Subsequent native evidence

The [native record](shadow-modes-native.md) identifies the exact separately frozen invocation, original captures, normal close and saved-data audit. It does not turn synthetic DPR2 into physical coverage or the CPU arrays into GPU readback. The independent CPU artifact review (historical record in Git history) is retained separately from the native operator's data checks.
