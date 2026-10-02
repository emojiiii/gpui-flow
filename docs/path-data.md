# Bounded Path/Pen data API

This slice adds data-level editing, not interactive point/control-handle editing
or full Leafer editor parity. The pinned upstream general point editor was a
stub; this API does not claim to reproduce functionality absent from that source.

- `Path::to_svg()` exports absolute M/L/Q/C/Z data with shortest f32 formatting
  and normalized negative zero. Use `Path::from_svg()` to import SVG `d` first.
  Source spelling, relative/shorthand commands and exact arcs are not retained.
- `PropertyKey::PathData` (`path.data`) accepts a serialized `Path` object with a
  `commands` array. It applies only to built-in Path and Pen, retaining the kind,
  transform, style, identity, parent and other element properties.
- `SceneEngine::set_path_data(handle, path)` commits atomically through the
  existing scene/layout validation. Changes invalidate geometry, paint and hit
  data and report old/new path data; no-ops do not advance the revision.
- `SceneEditor::set_path_data(handle, path)` participates in normal history and
  generic edit transactions, including Undo/Redo and cancellation.
- The API enforces the existing SVG adapter bounds: 16384 commands, finite
  coordinates in +/-1000000 and at most 1 MiB exported data. Empty paths clear
  geometry. Invalid input and non-Path/Pen targets are rejected atomically.
- Raw `Shape`/scene mutation remains the existing trusted escape hatch; this
  bounded API does not retroactively change all native path validation limits.

## Validation status

2026-10-02 recovery checkpoint: source reconstructed from verified upstream
`4e1d39b192cb3bc865df02d5ac742986f3c50bcd`, then implementation rewritten.
Changed Rust files pass rustfmt and `git diff --check`. Compilation and fresh
regressions are pending. Historical tests from the lost host are not validation
of this implementation. No Actions, deployment, or merge was requested.
