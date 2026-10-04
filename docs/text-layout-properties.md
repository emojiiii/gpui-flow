# Fixed plain-text layout properties

This module adds `letter_spacing`, `word_spacing`, `para_indent`, and
`auto_size_align` to the existing `text_case` and `para_spacing` contract.
`writing_mode` additionally supports a bounded Flow axis extension described below.
The pinned Leafer build only declares these modes; it has no non-horizontal
runtime to copy. This is not a claim of Leafer vertical-text parity.
Rich runs and WYSIWYG editing remain outside the original fixed-property task.

## Supported behavior

The three new numeric properties use absolute local logical units, default to
zero, and accept finite values in `0..=1_000_000`. Negative overlap, strings,
percent/unit objects, and implicit coercion are rejected. Font-size resizing
preserves these absolute values, as it already preserves paragraph spacing.
Leafer JSON uses `letterSpacing`, `wordSpacing`, `paraIndent`, `autoSizeAlign`, and
`writingMode`; Flow's own document and registry use snake_case.

GPUI first shapes the original presentation text with the selected font and
fallbacks. Letter spacing adds advance after each native shaping cluster,
including the last; it does not split ligatures, combining glyph clusters, or
emoji clusters. Word spacing adds advance after each ASCII U+0020 space cluster.
This is a deliberate bounded native-cluster/ASCII-space contract, not Leafer's
per-scalar shaping or its language-dependent word classification. Spaced or
indented layouts require monotonic native cluster indices and positions; unsupported
reverse-order native shaping fails rather than painting reordered text.

Paragraph indentation reserves space before only the first visual row of every
hard-newline paragraph, including empty/trailing paragraphs. That reserved space
participates in width measurement, alignment, wrapping and ellipsis. Soft wraps
start without indent. Spacing uses the pinned GPUI greedy wrap opportunities over
adjusted native advances. The shared shaped result drives native painting, CPU
pixels, Canvas glyph outlines, decorations, resource measurement, and export.
Case conversion happens first and canonical source content remains unchanged.
Paragraph gaps remain hard-newline-only. Existing `none`/`upper`/`lower` case
support is unchanged; `title` and `small-caps` remain explicit rejections.

`auto_size_align` defaults to false in Flow to preserve existing documents;
Leafer JSON defaults it to true, matching upstream. It affects axes owned by
measured layout: both axes for `TextSizing::Natural`, only y for `AutoHeight`.
Fixed-size text is unaffected. Measured layout writes the size and the serializable
`layout_origin` geometry atomically; it leaves the object's transform unchanged.
With admitted inner width W (Natural retains Flow's minimum width of 1),
measured height H and padding T/R/B/L:

- Start/Top and fixed axes use origin 0
- Automatic Center/Middle use `-W/2-L` and `-H/2-T`
- Automatic End/Bottom use negative outer width/height
- Disabling auto alignment resets measured origins to zero

Thus asymmetric padding expands around aligned content. Bounds, flattening, hit
geometry, clipping, painting, export pixels, and resize anchors all use that local
origin. Repeated reflow is idempotent; undo/redo restores the complete geometry.
`layout_origin` is computed layout output, finite and bounded to ±1,000,000.
Leafer JSON's currently supported explicit-size exporter rejects a nonzero origin
instead of silently discarding it; Flow document serialization retains it.

## Upright cluster writing modes

`x` retains the existing horizontal implementation. Flow's `x-reverse` places
native clusters right to left; `y` places them top to bottom; `y-reverse` places
them bottom to top. Every glyph stays upright. All glyphs and offsets in one
native shaping cluster stay together, including ligatures and combining marks.
Case conversion precedes shaping. Canonical source content is never reordered.
The nondefault modes require monotonic source clusters and horizontal native
positions; unsupported native ordering returns a typed `Unsupported` result.
This is axis layout, not bidi/RTL shaping or OpenType vertical GSUB/metrics.

Y modes use one font-size inline cell per native cluster, centered horizontally
in a line-height-wide column. Columns advance left to right. Wrapping uses the
content **height** for Y and width for X-reverse, breaking only between native
clusters. A cluster wider/taller than the inline limit remains whole and clips.
Letter/ASCII-space spacing follows the inline direction; paragraph indentation
applies only to each hard-newline paragraph's first row/column. Paragraph spacing
separates rows/columns only at hard newlines. Empty and trailing paragraphs retain
one row/column. `align` acts along the inline axis with Start/End following its
direction; `vertical_align` acts along the cross axis (left/middle/right in Y).
Padding and clipping remain in physical x/y coordinates.

Fixed boxes and `Natural` measured sizing are supported. Physical measurement,
auto-size alignment, local bounds, undo/redo, resource readiness and painting use
the same axis placement. Vertical `AutoHeight` supports `wrap=false`: it preserves
physical width and derives height, with auto alignment changing only the y origin.
For wrapped vertical text, `AutoWidth` instead preserves physical height, wraps
within its top/bottom padding and measures width; auto alignment changes only x.
Horizontal `AutoWidth` requires `wrap=false`. Neither mode may measure its own
wrap axis, and vertical first-baseline alignment remains unsupported. Third-party
measurers must opt into `measure_writing_mode`; its default rejects new modes. See
[measurement ownership](text-measurement.md) for padding and parent constraints.

All four modes support Clip, end Ellipsis, underline and strikethrough (including
both decorations together). Axis ellipsis removes only complete native source
clusters and appends the same native `...` marker used by horizontal ellipsis.
Retained source glyph IDs, fonts and intra-cluster offsets are unchanged; the marker
is shaped separately, so no new kerning or ligature crosses its boundary. Every
inline-overflowing visible row/column is shortened; cross-axis overflow forces a
marker on the last fully visible row/column (at least the first is retained).
A marker that cannot fit with paragraph indentation is kept whole and physically
clipped. Original cross-axis block extent and paragraph offsets survive truncation,
so removing hidden rows/columns never recenters the visible block.

Decorations follow the inline axis and exclude paragraph indentation, including
at the opposite end in reverse modes. X-reverse uses the displayed row's native
baseline offsets. Y modes place underline at the right edge of the centered
nominal font-size cell and strikethrough at the column center; this is Flow's
explicit upright-axis convention, not OpenType vertical decoration metrics.
Decoration thickness and endpoints share horizontal text's device-pixel snapping,
color, clipping and raster work limits. Empty rows/columns have no decoration;
visible ellipsis markers are decorated. Measurement remains based on source layout.

Nondefault modes use the same App's actual glyph raster backend through the bounded
CPU compositor for native presentation and raster/SVG export. A missing bridge
is an explicit failure. Direct native `DisplayElement` reports a required
compositing domain. No glyph rectangle proxy, font replacement, glyph mirroring
or rotated-image approximation is used. Leafer JSON accepts/emits the exact four
spellings using these documented Flow semantics; a pinned Leafer reader will
ignore its writingMode setting rather than reproduce Flow's axis presentation.

## Limits and checks

Nondefault spacing/indent admits at most 64KiB source UTF-8, 8192 shaped glyphs and
8192 source paragraphs. Owned copies of native layouts and wrap boundaries are
admitted before allocation and retained storage is charged beside paragraph
indices, bitmap and decoration storage, including simultaneously live ellipsis
source/probe/output copies. Existing source expansion, raster, outline, work,
font-epoch and cancellation gates remain in force. Default horizontal unstyled Clip text
keeps the existing native path. Axis plans additionally admit at most 8192 rows
and charge owned glyph/row/cluster storage before allocation, including simultaneously
live source, native marker and ellipsis output plans. Font epochs, cancellation,
pixel-work and working-set limits remain shared with horizontal text.

Focused regressions cover model/import/registry rejection, reactive reflow and
history, asymmetric auto alignment, nonzero-origin resize anchors, native cluster
advances, cache isolation, first-row wrapping/ellipsis, exact CPU glyph translation,
owned storage admission, glyph limits and cancellation. Axis formatting checks cover retained ligature and
combining clusters, marker placement in both directions, paragraph/wrap offsets,
narrow-marker clipping, snapped decoration geometry and native raster ink. These are local Linux font
checks, not cross-platform glyph or pixel guarantees. The compact native example below supplements these shared-layout checks with
visible property changes and history controls.

## Pinned primary references

All Leafer UI references use commit
`1e11641d3ea6b0211c83447268a90b8d58feec25`:

- [ITextStyleInputData](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interface/src/ICommonAttr.ts) fixes the seven-property scope
- [Text defaults and local bounds](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Text.ts) establish zero spacing/indent, writingMode x, autoSizeAlign true, and automatic local bounds
- [TextRows](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextRows.ts) and [CharLayout](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/CharLayout.ts) distinguish letter advances, word gaps and paragraph-start indentation
- [TextLayout](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextLayout.ts) and [TextConvert](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextConvert.ts) establish alignment and asymmetric auto-size padding
- [IWritingMode](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interface/src/type/IType.ts#L198) aliases [core axis types](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/display/ILeaf.ts#L29); pinned UI runtime search finds declarations/default only, with no writingMode consumer
- [GPUI LineLayout](https://github.com/longbridge/gpui-fast/blob/10d005166a9f9e079d55ebfb700035f3860c3e71/crates/gpui/src/text_system/line_layout.rs) and [LineWrapper](https://github.com/longbridge/gpui-fast/blob/10d005166a9f9e079d55ebfb700035f3860c3e71/crates/gpui/src/text_system/line_wrapper.rs) supply the native shaping-cluster and greedy wrap behavior used by the locked dependency

The compact native gate is `cargo run -p flow-studio --bin text_layout_properties`.
Click **Apply / reset layout**: paragraph first rows indent, spacing opens, and the
selected auto-sized label centers around the fixed blue crosshair. Click **Undo**
and **Redo** to restore both typography and anchored bounds. Click **Reject -1**:
the status reports rejection, with the current scene and revision unchanged.
The fixture uses the real native measurer and shared renderer, with no pixel oracle.

The Linux example completed Apply, Undo, Redo and atomic negative-value rejection, then exited normally. Public Rust struct literals for Text and TextMeasureRequest must initialize the new fields; existing constructors and older Scene JSON preserve their defaults.

**Next writing mode** cycles X → Y → Y-reverse → X-reverse → X for both
the clipped paragraph and Natural label. The same window installs the native
glyph bridge, so oriented text is real ink; Undo/Redo includes mode and bounds.

The axis-mode Linux smoke run cycled all four modes, reflowed the Natural label,
restored mode/bounds with Undo/Redo and rejected negative spacing atomically,
then exited normally. Vertical labels can overlap the example’s fixed annotations;
this does not establish typography for every script or every property combination.

**Ellipsis + lines** toggles end ellipsis with underline and strikethrough for
both samples, and temporarily disables paragraph wrapping to expose inline
overflow in every mode. Toggle again to restore Clip, no decoration and paragraph
wrapping. Undo/Redo includes these formatting changes. The Natural sample uses the shorter two-line “Center / label” text; the
fixed-origin annotation sits below its vertical bounds.

Axis ellipsis uses the same count-based line-box endpoints as intrinsic
measurement. A box exactly equal to a fractional measured block does not acquire
an ellipsis. For multi-row/column blocks, a one-ULP smaller cross-axis extent
still omits the last complete row/column; the existing minimum-one-row rule stays
in force for very small boxes.
