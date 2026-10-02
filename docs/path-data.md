# Bounded Path/Pen data API

This slice adds data-level editing, not interactive point/control-handle editing
or full Leafer editor parity. The pinned upstream [EditPoint display class](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/display/EditPoint.ts)
and [InnerEditor lifecycle hooks](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/InnerEditor.ts)
do not establish a general Path control-point editing implementation. No such
parity is claimed here.

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

## Usage

```rust,ignore
use gpui_flow_scene::display::Path;

let data = Path::from_svg("M0 0 Q20 40 40 0 Z")?;
let normalized_svg = data.to_svg()?;
editor.set_path_data(handle, data)?;
// The element keeps its Path/Pen kind; editor.undo() restores the prior path.
```

Use `PropertyKey::PathData` with a serialized `Path` for property batches or
owned edit sessions. An SVG string is deliberately not a second JSON property
representation: import it explicitly first. Scene documents keep their existing
Path/Pen JSON schema. The additional Rust enum variant requires updating any
external exhaustive `PropertyKey` matches.

## Validation status

Fresh recovery validation, 2026-10-02, official Rust 1.98.1 on Linux:

- 23 new regressions pass: SVG normalization/roundtrip, output/coordinate/command
  budgets, both kinds, retained metadata/identity, damage and ancestor bounds,
  changed and equal-bounds hit geometry, no-ops, malformed data, invalid targets,
  atomic mixed batches, stale/foreign/detached handles, layout rejection, JSON,
  single Undo/Redo, grouped edit cancellation and owned-session history.
- Four affected headless crates (`core`, `scene`, `engine`, `editor`): 311 tests
  including doctests pass, zero failures; one existing ignored radial-raster performance benchmark was
  not run. Strict Clippy `--all-targets -- -D warnings` passes for those crates.
- Rust formatting and `git diff --check` pass.

The lost checkout was reconstructed from verified upstream
`4e1d39b192cb3bc865df02d5ac742986f3c50bcd`; its 361-file tree matched exactly.
The implementation was rewritten, then saved as explicitly untested recovery
checkpoint `10cb2ac5fe9ff7801dc4379db262907a6710de9c` before fresh validation.
Historical tests from the lost host are not reused as evidence here.

Validation used a temporary headless workspace containing byte-identical copies
of those four crates and the repository workspace dependency definitions. Cargo
pruned the copied lockfile for that member subset; every retained external
package version and checksum was checked against the unchanged repository
`Cargo.lock`. Repository manifests and lockfiles were not altered. Both final
`cargo test` and `cargo clippy` runs used `--locked` against that copied lockfile.
This avoids downloading/building the unrelated native GPUI dependency graph.

Full workspace/native UI and cross-platform checks were not rerun for this
headless API slice. No interactive control-point tool or native UI was added.
No GitHub Actions, deployment, or merge was launched.
