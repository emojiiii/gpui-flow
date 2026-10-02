# Canvas command clipping

Status: recoverable source checkpoint; six new CPU pixel regressions, one adapter
regression and strict affected/example Clippy pass. Native acceptance is pending.

`CanvasCommand::ClipPath { path, rule }` intersects the current clip with an owned
Path using NonZero or EvenOdd. Geometry is frozen at the current transform when
the ClipPath command is evaluated; later transforms do not move an existing clip.
Current opacity does not change clipping. Save/Restore includes clip descriptions
alongside transform and opacity. A new batch starts without a clip. Empty paths
clip all subsequent draws until Restore or the end of the batch.

This is the explicit-Path subset of the pinned core
[clip/path/state contract](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts).
There is no implicit current-path builder or persistent browser context.

The separate `canvas_surface/commands/clip.rs` module compiles each draw through
nested existing alpha-mask domains. Each draw receives its complete clip before
source-over, including antialiased edges; it does not mask a whole multi-draw
batch afterward. That distinction matters when opaque draws overlap at a
partially covered clip edge. No new path flattening or scan converter is added.
Saved states retain borrowed clip descriptions only. Emitted per-draw path
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
