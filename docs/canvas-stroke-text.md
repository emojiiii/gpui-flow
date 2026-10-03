# Canvas shaped outline strokes

`CanvasCommand::StrokeTextBox` draws genuine host-supplied glyph outlines using
the existing centered, local-space stroke mesh. Its `Text`, top-left `position`,
solid `color` and `CanvasStrokeOptions` preserve the explicit content-box
contract of `FillTextBox`. Width, caps, joins, miter limits and dashes use the
same vector stroke implementation. This is not an HTML alphabetic-baseline API.

`draw_commands_with_fonts` supplies typed fill and outline jobs to the host.
Every outline result must match exactly one compiler-created job ID. Only its
private placeholder can become a Path; ordinary scene Text, generic TextInputs,
and export validation retain their existing behavior. The legacy fill-only
callback rejects stroke-text commands before invoking a fill producer.

The returned Path is in Text-local logical coordinates. The compositor applies
the Canvas transform and DPR once, clips to the exact transformed content box
and all active path clips, and paints the stroke as a triangle union so shared
edges do not multiply opacity. Empty, hidden and fully clipped jobs still need
valid outline capability and identity. Decorations are explicitly unsupported.
The optional [same-App adapter](canvas-shaped-outline-adapter.md) uses the
[bounded shaped producer](shaped-text-outlines.md) and registered static fonts.

Preparation admits outline commands and retained Path bytes before geometry
expansion. Shared limits bound flattening, generated points, dashes, triangles,
temporary geometry, pixel work and working storage. Lower caller limits remain
effective. Host-reported outline preparation work is checked and added to fill
work before rasterization. These are operation budgets, not total-process RSS.

`draw_commands_with_fonts_guarded` checks its validation callback before
preparation, before rasterization and immediately before committing pixels,
generation and current path. `SceneResourceAdapter::edit_canvas_guarded` checks
again after private resource upload and before replacing the current resource.
Any rejected batch preserves the old snapshot, generation and current path.
The check is a publication boundary, not a lock against an epoch change after
the validator returns.

## Checked scope

The full compositor suite passed 362 tests. The final focused stroke suite
passed 12 tests, including exact job IDs, Q/C paths, affine/DPR/clip/alpha/dashes,
reduced geometry/pixel/working quotas, work overflow, cancellation, late errors
and persistent-path replay after rejection. Four scene unit tests and 52 path,
alignment and stroke integration tests passed, with strict Clippy for both
crates. UI adapter/resource checks and the producer have separate receipts.

These tests use supplied outline geometry or a modeled text platform. The
sealed native font-identity gate establishes the startup binding separately.
The later [native acceptance](canvas-stroke-text-native.md) records the original
work-limit failure and the focused bounded-batch follow-up. That separate result
supplies the physical evidence for the fixed ELM-10 operation collection.

The validation summary (historical record in Git history)
records commands, log hashes and this checkpoint's source hashes.
