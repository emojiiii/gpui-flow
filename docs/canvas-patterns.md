# Canvas runtime image patterns

Status: runtime implementation with automated validation and a completed bounded
[Linux native control tour](canvas-pattern-native.md): 23/23 trace checks and
48/50 decoded-JPEG screenshot checks, with two exterior samples unresolved.
The later [outline-stroke route](canvas-stroke-text.md) and its
[native evidence](canvas-stroke-text-native.md) are recorded separately.

## Fixed API and coordinate contract

`CanvasSurface::create_pattern(&DisplayId, &ImageInputs, CanvasPatternRepeat)`
resolves an immutable decoded image immediately. The result owns its
`Arc<StaticImage>` snapshot, independently of later input replacement or removal.
`CanvasPatternRepeat::{Repeat, NoRepeat}` supports repetition in both directions
or a single source extent. There is no image loader or persistent pattern registry.

A new pattern has identity placement. `CanvasPattern::set_transform(Affine)`
**replaces** that matrix. Clones share pixels and copy placement; a command owns
its submitted pattern snapshot. The bounded Rust API rejects nonfinite or
out-of-range coefficients (absolute value above 1,000,000) with
`CompositeError::UnsupportedTransform`, preserving the previous matrix. Finite
singular pattern matrices paint transparently. CTMs retain their existing
nonsingular restrictions. Composed coefficients retain the same bound.
Nonsingular shader compositions that the rasterizer cannot safely invert are
rejected before publication. Inverse-mapped source coordinates over the render
target, padded by 16 device pixels on every side for SIMD/filter work, must be
finite and have absolute value at most 2^24 (16,777,216). This is the f32
consecutive-integer texel boundary and keeps tiny-skia's integer-backed
floor/fract operations in range; it does not guarantee fractional-texel precision
or browser pixel equality. A visible Repeat draw with nonsingular
`scale(1e-30, 1e-30)` therefore rejects atomically with either filter rather than
silently painting transparent. True rank-deficient pattern matrices still paint
transparently.

`CanvasCommand::FillPattern { path, pattern, rule }` and
`StrokePattern { path, pattern, options }` accept
`CanvasPathTarget::{Current, Explicit(Path)}`. Stroke options contain width, cap,
join, miter limit, dash pattern and dash offset; geometry remains centered-only.
No paint field is accepted and silently ignored. Existing commands are unchanged.
Patterns do not extend serializable scene `Paint`, scene JSON or editor history.

A decoded image pixel initially spans one logical pattern unit. Samples map
through surface DPR × draw-time CTM × pattern transform. Repetition stays
anchored at the source origin, including negative coordinates. To reproduce the
logical size of a DPR-2 Canvas snapshot, explicitly use a 0.5 pattern scale.
Current geometry remains frozen at append time; changing the draw-time CTM moves
the pattern and changes the stroke pen without relocating the retained path.

## Rendering and ownership

Pattern paints share the ordered vector/image/text command schedule, real filled
paths and the existing consistently wound stroke-mesh union. The rasterizer
constructs a borrowed tiny-skia Repeat shader only during the fill call, while
an owned Arc keeps pixels alive. There are no tiled bitmaps, per-tile draw loops,
unsafe lifetime extensions or per-triangle alpha accumulation. Tile size does not
increase the number of draw operations.

NoRepeat uses the **same wrapped sampler**, intersected with one transformed
source-extent clip. Edge filtering wraps source samples while the extent controls
coverage; Pad sampling would give a different result. Canvas `smooth` selects the
existing nearest/bilinear quality. tiny-skia can select nearest for translation
cases; browser pixel equality and cross-platform equality are not promised.
Opacity is applied once per command through normal composition. Frozen context
clips, evenodd holes, curves, Save/Restore and subsequent batches behave like the
existing vector commands.

Factory and drawing admission check source dimensions and bytes against the
respective Canvas limits, so a pattern created on a larger surface cannot bypass
another surface's reduced limits. Factory lookup retains the existing 512-byte
key ceiling and missing/failed-image errors. Source Arcs are deduplicated with
ordinary DrawImage sources and other patterns. Text preparation receives the
combined retained-image byte charge. Completed Canvas surfaces do not keep
pattern inputs. An externally owned pattern intentionally keeps its own snapshot
alive after a source Canvas is cleared, resized, detached or destroyed.

Explicit/current geometry, snapshots, source-extent rectangles, frozen clips,
nodes, masks, triangles, source bytes, output/candidate/scratch pixels and pixel
work use existing bounded admission before publication. A late invalid command,
missing image/font, reduced budget or failed upload preserves the prior path,
pixels and generation. As with other Canvas commands, a successful transparent
no-op need not publish a pixel generation.

## Scope and references

This implements the fixed runtime pattern portion of the pinned
[ICanvas contract](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts),
its [Canvas bindings](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/canvas/canvas/src/Canvas.ts)
and [image pattern placement](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/image/image/src/LeaferImage.ts).
Immutable capture, replacement transform, repetition and wrapped filtering follow
the [Canvas pattern contract](https://html.spec.whatwg.org/multipage/canvas.html#dom-context-2d-createpattern).

The generic same-App GPUI backend exposes glyph bitmaps and metrics. The later
opt-in [static-font outline adapter](canvas-shaped-outline-adapter.md) adds true
outline strokes under its explicit identity and format limits; a generic bridge
still cannot substitute bitmaps or bounding boxes. RepeatX/Y, pattern text,
arc/rect conveniences and persistent browser styles are exclusions, not added
closure requirements. Earlier current-path, image and text validation remains
separate in the respective Canvas documents.

## Automated validation (2026-10-02)

Using Rust/Cargo 1.98.1 on Linux with the existing native target and one build
job, the final snapshot passed:

- `cargo test --locked --offline`: 1,602 passed, 10 existing ignored
- `cargo test --locked --offline -p gpui-flow --lib`: 258 passed
- `cargo test --locked --offline -p gpui-flow-composite --lib canvas_pattern_`:
  36 passed, including the exact two-axis rejection and sampling-boundary checks
- `cargo test --locked --offline -p gpui-flow --lib canvas_pattern_`: six passed
- `cargo test --locked --offline -p flow-studio --bin canvas_patterns`: one
  control-tour test passed
- `cargo clippy --workspace --all-targets --locked --offline -- -D warnings`,
  `cargo fmt --all -- --check` and `git diff --check`: passed
- `cargo build --locked --offline -p flow-studio --bin canvas_patterns`: linked

Focused tests overlap the full suites and are not added to their totals.
Strict rustdoc with `RUSTDOCFLAGS="-D warnings"` still fails solely on the
pre-existing bare W3C URL at `crates/flow-composite/src/nonseparable.rs:2`.
`RUSTDOCFLAGS="-D warnings -A rustdoc::bare_urls" cargo doc --locked --offline
--no-deps -p gpui-flow-composite -p gpui-flow` passed. Only that lint category was
allowed; the unrelated source file is unchanged.

The previous 34-pattern/1,600-default-test snapshot did not contain the exact
two-axis regression. That experiment exposed transparent bilinear output from
an opaque tile in tiny-skia, justifying the new bounded sampling rejection.
The supported tiny-tile stress, singular transparency and atomic failure tests
remain passing. All 479 repository build inputs were identical before/after
final validation and native link:
`861ddb300d4b516ec1d46dc4b884b407bb3f48bf6187d5c23030787bb00212eb`.
The linked binary SHA-256 is
`9925d8b90c5bcd7afe0666597439721988ca6468e994922b28093abb34113fd4`.
Exact commands, log hashes, evidence provenance and the source allowlist are in
the automated validation summary (historical record in Git history).
That automated summary retains its pre-native, acceptance-pending status. The
subsequent [native report](canvas-pattern-native.md) and
native summary (historical record in Git history) record
the completed physical tour and its two unresolved screenshot checks. The
capture-format erratum (historical record in Git history) explains the lossy JPEG
evidence limits. No Actions, deployment or cross-platform tests were run for
this validation pass.

## Standalone fixture

Run `cargo run --locked -p flow-studio --bin canvas_patterns` on a native GPUI
desktop. The fixture is prepared for side-by-side DPR-1/2 acceptance with actual
controls and JSON witnesses; building it does not establish native acceptance.

The fixture starts at 1230 × 800 with 320 × 260 logical-unit Canvases. The
three control rows and natural-height footer do not shrink into the scene.

1. Regenerate shows an evenodd repeat-filled curve, single-tile NoRepeat extent,
   dashed open patterned stroke, reflected patterned polygon and frozen current
   path with patterned fill and stroke
2. Clear pixels → Paint current reuses retained geometry. Clear pixels →
   BeginPath → Paint current stays empty; Build frozen path restores geometry
   without publishing pixels
3. Paint CTM shifts the tile and changes the dashed pen while the retained
   outline stays fixed. Clear before comparing painted extents
4. Clip + restore demonstrates two nested frozen clips, a stripe after the inner
   Restore, an upper tile after the outer Restore and a yellow square in a new
   batch
5. Replace source regenerates with unchanged captured pixels; Recapture source
   creates new handles and changes the colors. Toggle filter → Regenerate shows
   nearest/bilinear changes without changing geometry
6. Reject late replaces tentative geometry and requests pattern paint before an
   invalid Restore. The report checks unchanged snapshot/generation and replay
   of retained geometry from temporary surface clones
7. Safe/reset shrink/grow and Swap DPR exercise configuration boundaries. Paint
   current afterward has no retained path; Undo config restores settings only.
   Regenerate restores artwork from the still-live pattern snapshots

Each physical control activation increments `button_action_count` once. JSON
records identify the action/origin, both configs, revision, source/capture state
and per-surface result. Startup and internal source regeneration carry separate
origins. Multi-batch Regenerate and Clip actions are not a single cross-surface
transaction; atomicity remains per Canvas command batch/adapter edit. The
control-tour test invokes the same operation helpers but is not a native-window
acceptance result.
