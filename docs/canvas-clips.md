# Canvas command clipping

Status: implemented and validated on Linux, with bounded per-draw clipping.

`CanvasCommand::ClipPath { path, rule }` intersects the current clip with an owned
Path using NonZero or EvenOdd. Geometry is frozen at the current transform when
the ClipPath command is evaluated; later transforms do not move an existing clip.
Current opacity does not change clipping. Save/Restore includes clip descriptions
alongside transform and opacity. A new batch starts without a clip. Empty paths
clip all subsequent draws until Restore or the end of the batch.

This is the explicit-Path subset of the pinned core
[clip/path/state contract](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts).
The later [current-path slice](canvas-current-path.md) adds a persistent builder
and frozen `ClipCurrentPath` snapshots; drawing state still remains batch-local.

The separate `canvas_surface/commands/clip.rs` module compiles each draw through
nested existing alpha-mask domains. Each draw receives its complete clip before
source-over, including antialiased edges; it does not mask a whole multi-draw
batch afterward. That distinction matters when opaque draws overlap at a
partially covered clip edge. No new path flattening or scan converter is added.
Saved states retain shared immutable clip descriptions. Emitted per-draw path
copies, mask runs and groups are charged to existing total-points, nodes, masks
and depth ceilings before their construction. Surface masks use the compositor's
existing byte/work admission. The shared 256-command and 16-state limits still
apply; the scene depth ceiling means at most 15 simultaneously active clips with
default limits. Reduced limits reject the whole batch atomically.

Clip-only batches do not publish. A valid draw that becomes fully transparent
through clipping can still publish one generation; the return value reports
publication, not a pixel-difference test. Neither commands nor clip state enter
scene JSON/history. Failed batches preserve snapshot identity and configuration.

Current limitations: clips affect only FillPath/StrokePath within this command
batch. Standalone clear/image/subtree/pixel APIs do not inherit the batch clip.
Arbitrary clip paths remain a Canvas-owned pixel operation; this does not expand
the retained scene's Box/Frame clip schema or native toolkit clipping APIs.

## Validation

Seven new regressions cover frozen transforms, opacity-independent clipping, DPR,
nested intersections, save/restore order, evenodd holes, empty paths, batch state
isolation, late invalid geometry and compiled geometry/mask/depth/byte/work
budgets. A curved-edge test independently checks per-draw source-over alpha and
compares a two-draw batch with two individual clipped draws, pixel for pixel.
Adapter checks preserve document configuration and snapshot identity on failure.

Affected tests: **367 passed, zero failed or ignored** (143 UI + 224 composite).
Strict affected all-targets Clippy, example Clippy and native linking passed.
The exact production source remained unchanged through native acceptance.

The updated `canvas_commands` window ran 2026-10-02 11:57:10–11:59:22 UTC and
closed normally with exit 0 and empty stdout/stderr. Actual DPR 1/2 pixels were
inspected for curved clips, nested evenodd holes, no clipping, an unclipped next
batch, late-invalid rollback, safe resize/configuration Undo and regeneration.
Saved artwork-region comparisons were exactly identical before/after rejection,
between the initial and regenerated clip, and between no-clip output and the
previous verified command-batch build. Save/Restore clip-state transitions and
per-draw edge-alpha arithmetic have focused CPU tests; the native controls do not
separately exercise every state-stack combination.

Source commit: `25776373accb59e7c966da31c89fa6ce1613cdb1`, tree
`4824d1a5756fe3fb9567fad11d3d9160e9315422`. Archive SHA-256:
`72cc7387a746e85dfea321c8b58da71da074ecf8a612ed0787ec05ccf6735710`.
Binary SHA-256:
`2376df89247679da8d2beb63c649e8b486ed4b4c12e047e7ba3cbae957ec5949`.
Screenshots and close receipt were retained with the local validation artifacts.
This is Linux coverage, not browser or cross-platform pixel equivalence.
