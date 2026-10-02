# Text padding and vertical alignment (source checkpoint)

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

This is a recoverable source checkpoint. Native all-target compilation passes;
new regression tests and final runtime validation remain pending.
