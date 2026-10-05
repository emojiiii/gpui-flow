# Native Canvas stroke-text acceptance fixture

> 历史验证记录：旧 Studio 原生演示已于 2026-10-05 移除。本文的源码路径、命令、交互矩阵与结果仅对应当时版本，不是当前可运行入口或新工作台验收。当前入口见[入门](getting-started.md)，测试保留范围见[迁移说明](studio-retirement.md)。

Status: the initial frozen `0338Z` revision passed cached build, strict Clippy and
the no-window helper tour; the build summary (historical record in Git history)
records those checks. Native scenarios 1–3 accepted on both resources. Its
original one-batch scenario 4 accepted at DPR 1 but returned `WorkLimit` at DPR 2
under the unchanged default 32,000,000 surface-write ceiling. The preceding
independent clear left DPR 2 empty. That failure and the original frozen
source/binary remain untouched historical evidence. Although its subsequent
rejection suite reported 8×2 preserved results, the empty DPR 2 baseline does
not establish rollback of visible stroke ink.

The current source deliberately separates scenario 4 into two balanced batches,
records every public resource edit, saves failed partial states, and disallows an
empty rejection baseline. Cached build, strict Clippy and the revised no-window
helper tour passed. The focused fourth-scenario, nonempty-rejection and
regeneration native gate then passed; see the
[combined native evidence](canvas-stroke-text-native.md). This document remains
the fixture contract; runtime/adapter, original broader controls and native font
identity have separately scoped evidence.

## Scope and opt-in

Only these fixture files are changed:

- `examples/studio/src/bin/canvas_stroke_text.rs`
- `examples/studio/Cargo.toml`
- `docs/canvas-stroke-text-fixture.md`

`canvas-stroke-text-x11` reuses Studio's existing
`outline-font-binding-x11` feature plus `gpui-flow/canvas-stroke-text`. The
`canvas_stroke_text` binary requires that feature and is absent from ordinary
Studio builds. No new dependency, lockfile change, platform graph, or runtime API
is introduced. No existing identity-fixture font is modified. This fixture has
no Wayland or cross-platform acceptance claim.

The application and bridge come from one
`application_with_static_outlines` call containing exactly these two immutable,
first-party CC0 sources:

| Source under `crates/flow-text/tests/fixtures/` | Bytes | SHA-256 |
| --- | ---: | --- |
| `stroke-quadratic.ttf` | 2900 | `baa84cec7f5f8b51981063d8ce98ba04e166c6998b18595df063248cbc4f04e3` |
| `stroke-cubic.otf` | 2724 | `0d85275cdd5deb518954403700fdb76bd7ec9749c968e386694eee05e66889f1` |

Their generator is `generate_stroke_fonts.py`. Each has 24 glyphs, including
quadratic/cubic curves, genuine O/B counters, space, period, fi, acute combining
mark, and é. TTF é uses components; CFF é has explicit contours. OpenType tables
contain fi ligation, AV kerning, and mark positioning. These are deliberately
geometric test fonts, not typographic reference faces.

Source and privately renamed registration bytes are saved in the session. The
startup receipt includes both source and registration SHA-256 identities, private
family aliases, App-resolved FontIds, and same-App Arc identity. The startup
factory verifies its direct-native-to-App invariant internally; the fixture does
not expose or independently inspect private direct-native IDs. Fresh private
aliases and registration hashes differ across launches.

The source manifest of a frozen acceptance build must include the **actual binary
font bytes**, their generator, the three fixture files, Cargo.lock, and all
implementation/dependency sources used by that binary. A font hash listing is
not a replacement for the source fonts. No ambient fallback font bytes are
extracted or redistributed.

## Real drawing and readback

Both scene Canvas resources start at 440×340 logical units, with DPR 1 on the
left and DPR 2 on the right. Every text sample submits actual
`CanvasCommand::StrokeTextBox` through
`SceneView::draw_canvas_commands_with_cancel`. The production adapter shapes with
the owning App, lowers actual glyph outlines to Paths, and publishes through the
resource guard. The fixture does not substitute a screenshot, text-box frame, or
pre-rendered glyph image for stroke geometry. Typography's thin rectangular
guides are separate ordinary vector commands.

The app opens at 1100×800 with two short control rows, the scene, scenario notes,
and status. Gold is TTF and cyan is CFF inside **each** resource. The dark panel,
border and external labels are scene presentation, not Canvas pixel content.
The PNGs therefore retain transparent counters and background. A drawable
surface is not a promise that the native window was visibly inspected: that
requires the physical tour and captures.

Every action creates a new numbered folder in a unique session under
`FLOW_CANVAS_STROKE_DIR` (default: a unique child of the OS temporary directory's
`gpui-flow-canvas-stroke`). Files use `create_new`; the session is not overwritten.
Every completed action exports both actual committed resources:

- `settled-one.premul-rgba`, `settled-two.premul-rgba`: exact row-major,
  premultiplied RGBA8 bytes; these are authoritative for equality
- `settled-one.png`, `settled-two.png`: true PNG files made by the existing
  `StaticImage::encode_png` row-wise demultiplying encoder, in straight RGBA8
- `receipt.json`: actual dimensions, configuration/DPR, generation, path storage,
  export filenames, named FNV-1a64 checksums, nonzero ink bounds, transparent /
  partial / opaque alpha counts, alpha histogram, and invalid-premultiplication
  count

The fixture first captures the surface through a pixel/path-neutral resource edit
to settle pending configurations, then obtains
`SceneResourceAdapter::canvas_snapshot` and requires Arc identity between these
two observations. It exports that committed resource snapshot. It does not
claim public access to GPUI's uploaded GPU allocation identity. Per-action
receipts are also JSON lines on stdout. Regeneration first writes
`regeneration-plan.json` with deliberate batch names and full command lists.
Each attempted clear or draw writes an append-only
`regeneration-NN-one.json` or `regeneration-NN-two.json`, also emitted as a
`regeneration-step` JSON line. These record acceptance/error, generation before
and after, snapshot publication, pixel checksums, dimensions/DPR, ink presence
and retained current-path size. Unattempted later steps have no result receipt.

Failed actions are marked `complete: false`, stop without retry, and attempt to
persist `failed.json` plus `failed-state-{one,two}` Canvas and cloned replay
raw/PNG exports. These describe the actual cleared or partially drawn state,
never a completed gallery. File-write/readback failures are reported explicitly.

FNV-1a64 is a compact, noncryptographic receipt checksum, not a cryptographic
identity proof. Exact raw-byte comparisons establish rollback. The retained
files can independently be SHA-256-hashed when freezing acceptance evidence.
Font source/registration hashes in the receipts are SHA-256. PNG demultiplication
cannot preserve hidden premultiplied rounding information; never use a PNG
round-trip to replace comparison of the original raw buffers.

## Exact physical control and scenario matrix

All controls remain in fixed positions as scenarios change:

| Row / position | Button | Required observation |
| --- | --- | --- |
| 1 / 1 | Next scenario | Cycle 1→2→3→4→1; accepted actual stroke batches and PNG/raw export for both DPRs |
| 1 / 2 | Regenerate | Clear each resource separately, then submit each planned batch to both resources; all planned steps must succeed |
| 1 / 3 | Reject suite | Require nonempty pixels and retained paths on both resources, then check eight rejected submissions/resource and one helper-level owner check |
| 1 / 4 | Invalidate + recover | Old-epoch binding lookup rejects; actual valid stroke commands recover under the fresh epoch |
| 1 / 5 | Export settled | Export the current committed resources without a drawing operation |
| 2 / 1 | Safe shrink/grow | Toggle logical width 440↔280 with safe physical intersection preservation; current path resets |
| 2 / 2 | Reset shrink/grow | Toggle width with reset semantics; pixels and current path clear |
| 2 / 3 | Swap DPR | Toggle each resource between DPR 1/2 using safe resize; physical pixels copy without scaling and path resets |
| 2 / 4 | Undo config | Undo one configuration transaction; pixels are not document history and no commands are replayed automatically |

The bounded gallery has four scenarios:

1. **Curves + counters:** `m O B é` with thin hollow strokes, followed by
   `fi AV ḿ é`; TTF is above CFF. Inspect curved boundaries, O's inner contour,
   B's two counters, TTF composite é, and gaps inside ordinary hollow glyphs
2. **Typography:** wrap, centered horizontal/middle vertical alignment, explicit
   padding, hard-newline paragraph spacing; adjacent right/top or bottom-aligned
   uppercase/lowercase boxes with three-period end ellipsis. The text boxes have
   independent thin guides. The period glyph is bound in both fixture fonts
3. **Pens + affine:** three rows of dashed glyph strokes using butt/miter,
   round/round and square/bevel, finite miter limit, odd dash array and changed
   dash offsets. Bottom samples combine rotation, skew, reflection, and
   nonuniform scale. All pens are centered in local outline space
4. **Clips + alpha:** a transformed persistent path establishes an even-odd outer
   clip with a rectangular hole, then a nested curved explicit clip; inner
   Restore permits a right-side sample while retaining the outer clip. Outer
   Restore permits bottom samples. Wide translucent strokes overlap between
   contours within one command and with a second independent translucent draw.
   Alpha is source-over per draw, not group opacity. The upper clipped block and
   lower translucent block are separate, deliberate resource command batches

Scenarios 1–3 still contain one gallery batch each. Scenario 4 has exactly two:

1. `current-path-nested-clips`: all original path/clip construction and three
   upper stroke-text jobs, both original Restores and both font faces
2. `alpha-overlap-and-sentinel`: the original four lower translucent text jobs,
   explicit opacity changes and final sentinel, again with both font faces

No visual commands are removed, reordered or automatically retried; this boundary
is planned before drawing and is independent of errors. The first batch ends
after the outer Restore at identity transform and full opacity. The second starts
with the same default state and explicitly sets its translucent opacity. Current
path geometry persists across the boundary until the final sentinel replaces it.
Each batch receives the unchanged default limits. A successful two-batch gallery
does not erase or overturn the historical one-batch `WorkLimit`, nor establish
that arbitrary larger batches fit the ceiling.

Each accepted gallery finishes with the same small curved-band current path and
its visible thin stroke at the bottom. A rejection replaces that path in its
candidate, so exact replay comparison can detect geometry rollback failures even
when the old pixels remain unchanged. The clips scenario constructs its own
current clipping path in the first batch before the second restores the sentinel.

Recommended bounded tour, recording each action and checking each resulting
PNG/raw pair:

1. Capture the initial native window and verify scenario 1 exports. Inspect all
   source/registration identities and startup native shaping diagnostics
2. Click Next scenario three times, pausing to inspect/export scenarios 2, 3, 4.
   For revised scenario 4 require all four named batch/resource draw receipts to
   accept and publish nonempty new snapshots. Inspect completed DPR 2 clips,
   hole, both fonts and lower alpha samples in the final PNG/raw pair
3. Click Reject suite, inspect every rejection receipt and before/after/replay
   file, then Regenerate for an explicit valid recovery
4. Click Invalidate + recover and inspect the fresh-epoch stroke export
5. Click Safe shrink/grow, Undo config, Regenerate. Verify crop/path reset and
   regenerated pixels. Repeat Safe shrink/grow twice if physical grow behavior
   is being accepted; growing alone must not resurrect cropped pixels
6. Click Reset shrink/grow, Undo config, Regenerate. Undo restores configuration
   but not cleared pixels; regeneration restores the selected scenario
7. Click Swap DPR, Export settled, Regenerate, Undo config, Regenerate. Verify
   actual dimensions, DPR and path reset in exports, then redraw at the restored
   DPR. Do not infer device DPR from a window screenshot
8. Cycle back to scenario 1 and capture the final window. Close normally only
   after all required receipts and exports are saved

The initial paint has origin `startup`; controls have origin `physical_button`.
Eight rejection substeps are synchronous consequences of one physical button,
not eight independently clicked buttons. The UI presents the aggregate result;
each receipt retains the individual outcomes. An unexpected successful batch or
wrong error class is a failure, not an expected rejection. Each clear and named
batch/resource edit is an independent transaction; neither the entire gallery
nor both resources commit atomically. Regeneration visits clear/one, clear/two,
then each named batch on one and two in order. It stops at the first failed
resource step, retains earlier accepted edits, and exports the actual failure
state. Successful draw calls must also publish a new nonempty snapshot to count
as complete gallery steps. This is not automatic fallback after `WorkLimit`.

## Rejection and shaping evidence

The Reject suite runs these prospective batches through the actual SceneView
route on each resource. Each batch starts with replacement current geometry and
a prospective white fill before its text/error. For each step, the fixture
requires the intended error class, identical committed Arc, unchanged generation,
exact premultiplied RGBA equality, unchanged retained path byte count and exact
current-path replay bytes. Before/after Canvas and cloned replay images are
saved. The clone replay clears only the clone's pixels and fills/strokes its
retained path; it does not paint into the live Canvas.

Both initial resources must have nonzero-alpha pixels and nonempty retained path
geometry. Otherwise the suite fails before submitting rejection cases and
requires complete regeneration. This prevents the original empty DPR 2 baseline
from being presented as successful visible-ink rollback.

| Step | Content / change | Expected error and additional proof |
| --- | --- | --- |
| fallback-first | `WQ m O`, TTF | `Binding(UnboundFont)`; real unbound first run plus real bound run(s), no missing glyphs |
| fallback-middle | `m WQ O`, CFF | Same error; actual bound runs both before and after an unbound run |
| fallback-last | `m O WQ`, TTF | Same error; real unbound final run plus bound run(s) |
| wrong-weight | Weight 700, CFF | Text binding rejects `StyleMismatch` or native resolution's unbound style ID; separate lookup of the actual bound ID must give `StyleMismatch` |
| wrong-slant | Italic, TTF | Same explicit wrong-style conditions |
| decoration | Underline, CFF | `UnsupportedTextStyle` |
| cancel-entry | Cancellation flag true before call | `Superseded` |
| late-restore | Valid text followed by unmatched Restore | Exact `InvalidGeometry("Canvas restore without save")` |

`WQ` is absent from the source fonts. Ambient native fallback must produce real
unbound runs. A host without usable fallback fails the check; the fixture never
fabricates a mixed candidate. Every independently shaped diagnostic run and glyph
position is recorded before the mixed-run decision, including later runs after
an unbound first run. Actual production adapter validation is still exercised by
the submitted Canvas command, independently of these diagnostic probes.

The same physical suite also reuses a real numeric App FontId against a separate
Noop TextSystem Arc and requires `WrongTextSystem`. This is clearly identified as
a binding-level check. No invalid bridge is installed in SceneView and no second
native App is created.

Startup shaping probes include `fi` / `f i`, `AV` / `A V`, combining acute,
precomposed é and counters for both faces. Receipts record actual runs, glyph IDs,
indices, positions and widths. Source IDs 19/20/21 mean fi/acutecomb/é. Use the
actual native observations to determine whether liga/kern/mark features were
applied; source-table presence alone is not an acceptance result. These probes
are diagnostic shaping, not the exact line objects painted by the Canvas adapter.

## Deliberate limits

- Cancellation is set **on entry**. The fixture does not inject a flag change
  mid-raster or between candidate upload and resource publication. Those guard
  timings belong to automatic runtime tests and must be cited separately
- Late Restore fails during batch validation/admission; it is not evidence of a
  partially rasterized/uploaded candidate. Pixels/path rollback are still
  physically checked at the public operation boundary
- Caller-reduced outline budgets are not exposed by the public SceneView command
  API. This fixture does not bypass the route with synthetic glyph output to
  claim native lower-budget acceptance; automatic adapter tests cover it
- Generic bridge/feature-off rejection is covered separately. The required
  fixture feature intentionally cannot run a feature-off native mode
- Raw/PNG exports exclude the scene's backdrop, border, corner masks, camera and
  other objects. They are actual committed Canvas pixel content, not a whole
  window capture, GPU atlas dump, editable outline export, or vector file
- Desktop captures are JPEG in this environment. They establish physical visual
  inspection only; exact alpha, holes, clipping and rollback must be audited
  with the raw and PNG files
- Native hinted text fills are not expected to be pixel-identical to unhinted
  vector strokes. No browser or cross-platform equivalence is implied

## Deferred build, helper checks and cost

The revised source is ready for independent review. The source was
parsed/formatted with the installed `rustfmt --edition 2024` and its `--check`
form. Cached build, strict Clippy, the revised helper tour and the focused native
recheck passed; no separate unit-test harness was linked for the two-batch remedy.
The original partial native results remain separate
evidence. Parent coordination owns the single build slot, free-space watchdog,
native desktop slot, frozen copy, and any Git publication.

After an exact build slot/disk allowance is granted, use only the existing native
environment and cached Studio X11 graph:

历史命令（当前分支不可运行；仅供对应历史版本复现）：

```text
. /workspace/shared/gpui-flow-native-env.sh
CARGO_PROFILE_TEST_DEBUG=0 cargo check --locked --offline -p flow-studio \
  --features canvas-stroke-text-x11 --bin canvas_stroke_text
CARGO_PROFILE_TEST_DEBUG=0 cargo clippy --locked --offline -p flow-studio \
  --features canvas-stroke-text-x11 --bin canvas_stroke_text -- -D warnings
CARGO_PROFILE_TEST_DEBUG=0 cargo build --locked --offline -p flow-studio \
  --features canvas-stroke-text-x11 --bin canvas_stroke_text
"$CARGO_TARGET_DIR/debug/canvas_stroke_text" --helper-tour
```

Do not interpret these commands as permission to execute them. Use one Cargo
process, one build job, incremental off and dev/test debug information off. Stop
the approved build watchdog at 3.125 GiB available and preserve the 3 GiB hard
reserve. Source-only edits need negligible space; a new native link does not.

The retained `outline_font_binding` executable is 165,347,416 bytes (about 158 MiB).
Budget roughly **250–550 MiB additional native build/link scratch**, plus about
**160–200 MiB for a separate frozen executable** if needed, and **150–220 MiB for
one full acceptance session** with the above exports. This is a cache-based
estimate, not a reservation or measured build result. Existing runtime objects
may need recompilation after adapter changes. At 3.49 GiB free, available space
above the 3.125 GiB stop line is only about 374 MiB: do not assume it suffices for
linking, retaining a frozen executable and the entire tour. Reassess measured
free space before each expensive step; no deletion is authorized by this document.

At the original dimensions a raw snapshot pair is exactly 2,992,000 bytes.
The rejection suite retains nine before/after pairs and their replay pairs,
plus its final settled pair: 56,848,000 raw bytes, before PNG/JSON overhead.
Repeated clicks intentionally create new evidence and consume more space.
Ordinary gallery actions retain one pair; each configuration action additionally
retains its before/replay pair. Never silently remove session evidence to make
room for a build or repeat tour.

`--helper-tour` branches before any Application/platform/window creation. It
prepares both real source fonts, checks expected quadratic/cubic/counter/composite
outline coverage, validates every gallery Text request and per-batch command count,
requires independent Save/Restore balance and both font faces in every batch,
keeps scenarios 1–3 at one batch with 4/6/8 text jobs respectively, and requires
scenario 4's named two-batch plan with 3/4 text jobs,
checks current-path rollback at DPR 1/2, and round-trips a known premultiplied
sample through the existing PNG encoder/decoder to verify demultiplication.
It does not establish native shaping, stroke rendering or resource upload.

The optional unit-test harness adds a checksum known-vector check:

历史命令（当前分支不可运行；仅供对应历史版本复现）：

```text
CARGO_PROFILE_TEST_DEBUG=0 cargo test --locked --offline -p flow-studio \
  --features canvas-stroke-text-x11 --bin canvas_stroke_text
```

Prefer the existing binary's helper-tour when a second harness link would threaten
the reserve. Only the desktop-slot owner launches the frozen executable:

```sh
FLOW_CANVAS_STROKE_DIR=/absolute/authorized/evidence/root \
  /absolute/frozen/path/canvas_stroke_text
```

Accept the gate only after the exact built bytes, all physically exercised
controls, per-rejection outcomes, genuine native visuals, lossless exports,
configuration/Undo recovery and honest remaining limits have been reviewed and
recorded in a separate acceptance result.
