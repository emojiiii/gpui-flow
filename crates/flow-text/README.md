# gpui-flow-text

Reusable plain-text measurement contracts, independent of the Flow scene,
layout, editor and UI crates. The default build has **no dependencies**. The
optional `gpui` feature provides the existing gpui-fast native backend; it does
not implement another font shaper or install fonts.

Use the package from this repository (it is not published to crates.io):

```toml
# Contracts only; no GPUI dependency.
gpui-flow-text = { path = "../gpui-flow/crates/flow-text" }
# Or enable the native implementation with the same pinned gpui-fast version:
# gpui-flow-text = { path = "../gpui-flow/crates/flow-text", features = ["gpui"] }
```

```rust
use gpui_flow_text::TextMeasureRequest;
let request = TextMeasureRequest {
    content: "Hello, 画布 👩‍💻".into(),
    font_family: "sans-serif".into(),
    font_size: 16.0,
    font_weight: 400,
    italic: false,
    text_case: Default::default(),
    para_spacing: 0.,
    letter_spacing: 0.,
    word_spacing: 0.,
    para_indent: 0.,
    line_height: 20.0,
    wrap_width: Some(200.0),
};
request.validate().unwrap();
```

A host can implement `TextMeasurer`, or enable `features = ["gpui"]` and construct
`GpuiTextMeasurer::new(app.text_system().clone())`. `measure` uses the actual
GPUI `WindowTextSystem::shape_text` and `WrappedLine::size`. For painting with
matching font and wrapping policy, `gpui_backend::shape_text` returns GPUI's
`WrappedLine` values directly. Use the same app text system for both.

All sizes are logical model units. Empty text has one line; a final newline
preserves an additional empty line. Family, weight, italic face, font size, absolute line
height, text case, paragraph/letter/word spacing, paragraph indent and optional wrap width determine the result. Platform fallback, CJK,
emoji and shaping remain GPUI's responsibility. Metrics use line boxes, not
ink bounds. Errors return without replacing document geometry.

The native adapter bounds its successful-result cache to 128 entries / 1 MiB.
Call `invalidate` when host font resources change. An invalidation interleaved
with shaping rejects the stale measurement. Host font registration and GPUI's
own resource caches are the host's responsibility.

Existing users can keep `gpui_flow_layout::{TextMeasureRequest, TextMetrics,
TextMeasurer}` and `gpui_flow::GpuiTextMeasurer`: these re-export the same types.
`FlowLayout`, `TextSizing`, reactive bindings and Undo integration stay in
`gpui-flow-layout`; the measurement crate does not own document transactions.

Verification:

```sh
cargo test --locked -p gpui-flow-text --no-default-features
cargo test --locked -p gpui-flow-text --features gpui
```

## Explicit native raster bridge

With the `gpui` feature, `GpuiTextRasterizer::application(platform)` captures the platform's actual raster backend and constructs Application using that same platform. Explicitly pass the returned Arc to `SceneView::set_text_rasterizer`; an arbitrary existing App cannot expose GPUI's private raster handle. Font/glyph identity, shaping, fallback and wrapping stay with the same native `TextSystem`. The producer uses grayscale A8 and straight-BGRA color glyphs with bounded job-local caching and stale-font epoch checks. See [compositing](../../docs/compositing.md#host-opt-in-same-native-backend-text) for the supported plain-solid/uniform-scale subset, limits, lifecycle and real-font/native reproduction commands.

## Fixed-box paint overflow

The optional `gpui_overflow::shape_text_in_box` shares GPUI visual rows and
grapheme-safe end-ellipsis decisions between native and CPU painting. This is a
paint policy; `TextMeasurer` still measures the original source. `TextRasterRequest`
now includes `overflow: TextOverflow::Clip` (old behavior) or `Ellipsis`. The
contracts-only default stays dependency-free; the GPUI feature reuses the locked
Unicode segmentation crate. See [limits and native evidence](../../docs/text-overflow.md).

`TextMeasureRequest::italic` selects the native italic face and participates in
measurement cache keys. Set `false` in existing Rust literals to preserve upright
behavior. Explicit-box clipping/advances stay unchanged; see the
[font-face contract and native receipt](../../docs/text-italic.md).

`TextRasterRequest::decoration` selects `None` (existing behavior), `Underline`,
`Strikethrough`, or `UnderlineStrikethrough`. The optional `gpui_decoration`
module uses the same shaped rows, actual font metrics and bounded allocation for
native/CPU geometry. Existing Rust request literals must add `decoration`;
measurement requests deliberately remain decoration-independent. See the
[decoration contract and native receipt](../../docs/text-decoration.md).

The SceneView producer now supports [bounded affine Text](../../docs/text-affine.md)
by choosing maximum-scale local raster coordinates and reusing the existing
image compositor. TextRasterRequest values must share one raster coordinate
space; RasterizedText.origin is expressed in that space after device scaling.


## Presentation case and paragraph gaps

`TextMeasureRequest` now includes `text_case: TextCase::None` and
`para_spacing: 0.`. `Upper`/`Lower` transform Unicode scalars before shaping while
preserving canonical `content`; Title and SmallCaps are not guessed. Conversion
source and expanded output are bounded to 64KiB UTF-8. A custom `TextMeasurer`
should use `request.text_case.presentation(&request.content)` for display content
and `request.block_height(visual_rows)` for line-box height including hard-newline
gaps. Soft wraps never gain paragraph spacing. See the
[contract and regression entry points](../../docs/text-case-paragraph-spacing.md).
For painting, use `gpui_overflow::shape_text_in_box` and its `line_offsets` in both
native and CPU paths; raw `shape_text` returns GPUI lines without positioning them.

## Optional static outline preparation

The independent `static-outlines` feature adds bounded CPU-only static-font
preparation and genuine quadratic/cubic glyph extraction, without enabling GPUI
or changing the dependency-free default. It accepts host-authorized bytes,
preserves license metadata, rewrites private aliases and exposes exact byte
identities. It does not register fonts, prove a GPUI FontId binding or enable
Canvas strokeText. See the [subset, budgets and remaining integration gates](../../docs/static-font-outlines.md).

Spacing and indentation use bounded native clusters; see the [fixed layout contract](../../docs/text-layout-properties.md) for supported values, budgets, upright writing axes, ellipsis/decoration and bounded first baselines.
