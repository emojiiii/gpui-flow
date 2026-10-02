# Text padding and vertical alignment

The Text component gains typed top/middle/bottom vertical alignment and four-side
padding in [top,right,bottom,left] order. Zero padding/top remain the exact old
serialization/default behavior. New fields are handled in the Text component's
own file and factory, ordinary validated Shape transactions, resource tickets,
editor Undo, shared native shaping/wrapping, auto-size measurement and CPU glyph
rasterization. Existing outer-box geometry/hit behavior is preserved.

The semantic references are the pinned
[Text properties](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Text.ts),
[TextConvert padding](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextConvert.ts)
and [TextLayout alignment](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextLayout.ts).
No upstream code is copied and no new shaping/layout dependency is added.

The inner box drives wrap width, horizontal/vertical alignment and glyph clipping
on both rendering paths. Measured Natural width and all measured heights add
padding back once. Under clipped overflow, content taller than the available box
stays top-aligned, following the pinned clipped branch. Flow retains its existing
always-clipped explicit text-box contract; upstream's separate overflow-show,
auto-size alignment offsets and writing modes are not introduced.

Each padding side must be finite, nonnegative and at most 1000000 local units,
and must leave positive finite inner dimensions. Overconstrained padding rejects
at the atomic mutation boundary, rather than using upstream's 0.01-width fallback.
Two/three-value shorthand, relative units, rich text, ellipsis, decoration, italic,
IME, letter/word spacing and arbitrary text transformations remain out of scope.
Existing Text/RasterRequest Rust struct literals need the newly added fields;
repo callers are migrated. Scene JSON without them remains compatible.

## Validation

Fresh Linux/Rust 1.98.1 validation on 2026-10-02:

- 13 new regressions cover legacy/default JSON, four-side geometry and invalid
  padding, factory/atomic rollback, unchanged-outer-bounds resource invalidation,
  Undo/Redo/cancellation, measured Natural/AutoHeight and reactive convergence,
  native pan/zoom plan, resource wrap requests, shared clipped-overflow offset,
  and actual same-backend CPU glyph pixels for all vertical alignments.
- Affected UI/text library tests: 121 passed. Scene/engine/editor/layout library
  and integration tests: 546 passed and one existing ignored manual radial
  raster benchmark. Total 667, zero failures; this is the affected subset, not
  every workspace crate or doctest.
- Native all-target check, six-package strict all-target Clippy, workspace
  rustfmt, diff checks and the `text_box` Studio example link all pass.

The side-by-side native/CPU example was observed on the Linux cloud desktop from
08:12:28 to 08:14:24 UTC. Both columns matched for initial padded top alignment,
middle and bottom, padding removal and rewrap, clipped overflowing content,
restored padding during overflow, and repeated Undo back to the initial state.
No source/test inputs changed during the run. The source snapshot tree was
`db446f6e70fe349e228534311fa380c3aab51b03`; executable SHA-256 was
`d3eb6be9162eea4b31908527afe284a9824a6e5258e56ba1416cb3b0291c6ed3`.
Normal close returned exit 0 with an empty runtime error log. Window screenshots
were inspected after each meaningful change. This is bounded single-desktop
visual/interaction evidence, not a pixel-identical cross-platform claim.

No GitHub Actions, deployment or merge was launched.
