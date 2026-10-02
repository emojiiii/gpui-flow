# Text case and paragraph spacing

This is a bounded plain-text subset. `Text.text_case` supports `none` (default),
`upper`, and `lower`; `Text.para_spacing` defaults to `0` local logical units.
Canonical `Text.content` is preserved by shaping, painting, measurement, layout,
serialization, and undo/redo. Document spellings are snake_case, not a Leafer JSON
import API. `title`, `small-caps`, and unknown enum spellings are rejected.

Case conversion happens before GPUI shaping and native wrapping. Each Unicode
scalar uses the Rust standard library's uppercase/lowercase mapping independently,
matching the pinned upstream per-character transformation. Expanding characters
(e.g. ß → SS, İ → i + combining dot) are supported. No normalization, locale-specific
casing, contextual whole-string lowercasing, or font small-caps synthesis is added.
The Unicode tables are those of the Rust toolchain; byte-for-byte cross-version
browser Unicode equivalence is not promised. The existing locked GPUI/font platform
remains responsible for shaping, fallback, glyph selection, and wrapping.

Paragraphs are delimited only by U+000A (`\n`), including empty paragraphs and a
trailing newline, preserving Flow's existing explicit line-box behavior. A paragraph
may contain many soft-wrap visual rows; those rows do not acquire paragraph gaps.
The block height is visual row count × absolute line height plus hard-newline count
× paragraph spacing. Gaps are inserted before each paragraph after the first,
with no leading or trailing gap beyond the empty paragraph represented by a final
newline. Native text, CPU text, decorations, ellipsis, and vertical alignment share
these same row offsets. Ellipsis selects the last fully visible line box (retaining
at least the first row for tiny boxes), so a paragraph gap cannot cause an extra
hidden row to appear or shift the baseline.

`para_spacing` must be finite and within 0..=1,000,000. Negative paragraph overlap
is outside this slice. Case conversion and nonzero paragraph spacing admit at most
64KiB UTF-8 source. Converted UTF-8 is also capped at 64KiB before allocation.
Existing measurement, ellipsis, decoration, raster glyph, bitmap, cancellation,
and working-set budgets remain in force. Retained paragraph/ellipsis offsets are
charged to the CPU raster working-set budget. Unstyled default Clip text retains its
existing explicitly sized native path.

## Pinned upstream evidence

All references use leafer-ui `1e11641d3ea6b0211c83447268a90b8d58feec25`:

- [ICommonAttr](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interface/src/ICommonAttr.ts) declares textCase and paraSpacing
- [IType](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interface/src/type/IType.ts) defines none, upper, lower, title, and small-caps
- [TextCase](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextCase.ts) transforms each input character; title changes the first word character only
- [TextRows](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextRows.ts) separates newline paragraph starts from soft wraps and applies casing before measuring
- [TextLayout](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextLayout.ts) adds paraSpacing at paragraph starts and in total block height
- [TextConvert](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextConvert.ts) establishes row creation, padding, layout, character layout, clipping, and decoration order
- [CharLayout](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/CharLayout.ts) consumes upstream word/paragraph grouping
- [Text](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Text.ts) defaults textCase/paraSpacing and uses a browser font variant for small-caps

Title depends on Leafer's own width-dependent word grouping and is not replaced by
Unicode titlecase. Small-caps remains unsupported. Letter/word spacing, indentation,
rich text, vertical writing, full Leafer wrapping parity, and editor typing/caret
behavior are also outside this slice.

## Local verification entry points

- Model and engine: `cargo test -p gpui-flow-scene -p gpui-flow-engine --test text_paragraph_styles`
- Reactive measurement/history: `cargo test -p gpui-flow-layout --test text_paragraph_styles`
- Native shaper/CPU regression fixtures: `cargo test -p gpui-flow --lib paragraph_style_tests`
- Native versus CPU demo: `cargo run -p flow-studio --bin text_paragraph_styles`

The demo cycles None/Upper/Lower and 0/10 paragraph spacing on two retained Text
objects, with padding, empty/trailing paragraphs, soft wrapping, end ellipsis,
vertical alignment, and Undo. Native platform checks must be reported separately
from headless model tests; building the demo alone is not visual verification.

## 2026-10-02 local regression receipt

The final style implementation passed 1,001 local tests: 808 across scene, engine,
layout, and interaction (including their doctests); 179 native UI tests; 13 GPUI
text tests; and one standalone text-contract doctest. Three benchmark
fixtures remained explicitly ignored. This includes 22 new regressions for this
slice and the FontSize-resize preservation cross-test. The eight new native
fixtures cover case-equivalent shaped widths and exact CPU pixels, empty/trailing
paragraphs, soft-wrap offsets, gap-aware ellipsis/decorations, DPR/alignment,
exact paragraph glyph displacement, retained offset-memory admission, and
cancellation/work-budget failure.

Strict Clippy passed for all targets of `gpui-flow`, `gpui-flow-text`,
`gpui-flow-scene`, `gpui-flow-layout`, `gpui-flow-engine`, `gpui-flow-interaction`,
and `flow-studio`, including every existing example's request-literal migration.
The new demo linked successfully. These are local Linux checks using installed
fonts. Interactive window/pixel acceptance is separate; these results do not
establish macOS/Windows font or pixel identity.

## Native window receipt

The exact tested source is commit `4b47d2597030de8a49fb08d9e2b62587604c37ab`,
tree `2f594c12b2ac680a633f05751522aaf01bb6f6e1`. Source archive SHA256:
`5564634eb596352861385fb32722ec016e93518b0ca8bae92d69f85501ad68e6`.
The linked binary SHA256 is
`29c94e058ee7bc2e2704c74db8ad6d0e26f2298130553362601b10e9da22b2b1`.

On Linux, the actual window ran 2026-10-02 15:51:24–16:01:05 UTC and closed
normally with exit 0 and empty stdout/stderr. Physical controls exercised
None → Upper → Lower, paragraph spacing 10 → 0, top/middle/bottom under overflow,
and five Undo operations restoring the original case and spacing. Upper expanded
ß to SS and changed wrapping; Lower displayed the scalar-based dotted-I and Greek
case behavior. Both paths retained the same soft-wrap spacing, hard-newline gaps,
clipping and end ellipsis. Because the fixture overflows, vertical alignment
clamps to the same top position; this is not evidence of a non-overflow vertical
shift (the native automated alignment fixtures cover that separately).

Seven captured states had identical 252×192 RGBA artwork regions between the
native and CPU presentations after translating by their 300-pixel separation.
The restored 600×240 artwork region also exactly matched the initial capture.
These are specific local-font/DPR screenshot comparisons, not cross-platform
pixel guarantees. The retained canonical-content and serialization guarantees
come from the model regressions, not from visually inspecting the transformed text.
