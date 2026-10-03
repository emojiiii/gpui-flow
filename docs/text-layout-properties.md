# Fixed plain-text layout properties

This module adds `letter_spacing`, `word_spacing`, `para_indent`, and
`auto_size_align` to the existing `text_case` and `para_spacing` contract.
ELM-04 remains **partial**: actual vertical/reverse `writing_mode` layout is still
required. `writing_mode: "x"` is the sole accepted value; `y`, `x-reverse`,
`y-reverse`, and unknown values fail explicitly. Declaring or round-tripping an
unused enum does not establish writing-mode support. Rich runs and WYSIWYG editing
remain outside the original fixed-property task.

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

## Limits and checks

Nondefault spacing/indent admits at most 64KiB source UTF-8, 8192 shaped glyphs and
8192 source paragraphs. Owned copies of native layouts and wrap boundaries are
admitted before allocation and retained storage is charged beside paragraph
indices, bitmap and decoration storage, including simultaneously live ellipsis
source/probe/output copies. Existing source expansion, raster, outline, work,
font-epoch and cancellation gates remain in force. Default unstyled Clip text
keeps the existing native path.

Focused regressions cover model/import/registry rejection, reactive reflow and
history, asymmetric auto alignment, nonzero-origin resize anchors, native cluster
advances, cache isolation, first-row wrapping/ellipsis, exact CPU glyph translation,
owned storage admission, glyph limits and cancellation. These are local Linux font
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
