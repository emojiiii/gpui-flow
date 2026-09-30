# Measured scene text

This is a bounded plain-text slice of Leafer's text layout contract. It uses the
same native GPUI shaping function for measurement and scene painting. It does
not estimate width from character counts and does not provide rich-text editing,
IME, baselines, letter/paragraph spacing, ellipsis, percentage sizing, or complete
Leafer Text compatibility.

## Inputs and dimension ownership

`Text` stores family, font size, weight (1..=1000, default 400 for old JSON),
relative line height, alignment, wrap policy and a positive explicit size.
`FlowLayout.texts` optionally maps live Text IDs to:

- `TextSizing::Natural`: measure both axes without wrapping; requires `wrap=false`
- `TextSizing::AutoHeight`: preserve the explicit or parent-allocated width;
  measure height using that width when `wrap=true`, otherwise without wrapping

Measured height cannot also be allocated by parent grow/basis/stretch/min/max.
Natural width has the same ownership restriction. AutoHeight width may be
allocated, for example by a column container's Stretch. Text is measured before
and after parent allocation, then existing bounded measure/arrange passes settle
text and content-sized ancestors. Conflicting ownership, invalid metrics and
non-convergence fail on a private candidate.

Each logical line box is `font_size * line_height`. Empty text retains one line;
a final newline retains another empty line. Native empty-text width is zero;
Natural scene geometry uses a minimum width of 1 to preserve the scene's positive
bounds contract. AutoHeight retains its supplied width even for empty text.
Width is the shaper's layout advance, not glyph ink or stroke bounds. Individual
glyphs can overhang or be taller than a configured line box; painting clips to the
explicit text rectangle. Start/Center/End use GPUI's line alignment within that
same rectangle.

## Host adapter and reactive edits

The model/layout crate has no GPUI dependency. `TextMeasurer: Send + Sync` accepts
an owned `TextMeasureRequest` and returns validated `TextMetrics`. Implementations
must be deterministic for the current font environment; callbacks may run more
than once. Metrics must use the requested absolute line height and report
`height = line_height * lines`. They must not commit edits or produce external
side effects from `measure`.

The native crate exports `GpuiTextMeasurer`. Bind it to the **same**
`App::text_system()` used by rendering:

```rust,ignore
let backend = Arc::new(GpuiTextMeasurer::new(cx.text_system().clone()));
config.texts.insert("body".into(), TextSizing::AutoHeight);
let measured = config.with_text_measurer(backend);
measured.apply_to_editor(&mut view.editor)?;
measured.install_reactive(&mut view.editor.engine)?;
// A normal editor.transaction / shape property edit now measures and arranges
// text and dependent containers before the single atomic commit notification.
```

The executable `measured_text` example changes content, font size, weight and
container width through ordinary editor transactions. Buttons do not manually
call reflow to simulate automatic layout. Explicit `refresh_text_metrics` clears
the backend metric cache and performs a real atomic layout transaction. A refresh
with unchanged metrics adds no Undo step; changed metrics and dependent geometry
are one step. Backend errors, panics, or unavailable fonts leave prior document
sizes, revision and history intact. A failed edit also leaves current animation
playback active; a successful edit respects the existing separate playback/edit
Undo boundaries. Undo/Redo restores exact snapshots without remeasuring them.

Binding and font service are runtime configuration. Serialize `layout()` in
FlowDocument and recreate the backend/binding in the host after loading. Destroyed
handles do not give replacement Text IDs old measurement authority; reset removes
runtime providers. Detach/reattach follows the existing live-layout binding policy.
Definitions used alongside measured text must be pure/deterministic; geometry is
normalized before measuring and revalidated before commit.

## Native shaping, painting and cache

`GpuiTextMeasurer` and the scene canvas text painter call the same
`WindowTextSystem::shape_text` entry point with explicit family/weight, normal
font style/default features, model wrap width and line height. Painting uses
`WrappedLine::paint`, avoiding inherited host truncation, line-clamp or fallback
styles. CJK, emoji, ligatures and missing-family/glyph substitution use GPUI's
platform shaper. A requested missing family may resolve through GPUI's fallback
stack; the headless probe checks actual resolved fallback and measured output.
No available usable font is an error, including for empty text.

Metrics are in model units. Viewport zoom and device pixel ratio never rewrite
text dimensions or enter the model cache. The painter scales font size, wrap
width, line height and clip rectangle together. Existing renderer limits remain:
positive uniform text transforms, its own overflow rectangle and nested rectangular Frame clips are supported; skew/rotation/nonuniform transforms and rounded/affine native clips continue to report unsupported diagnostics. See [native clipping](native-clipping.md).

Successful metric requests are cached by all inputs, at most 128 entries and
1 MiB of text/family bytes. Errors are not cached. Resource invalidation clears
entries; an in-flight earlier measurement returns a retryable error instead of committing stale geometry or repopulating that cache generation.
Measured requests are bounded to 64 KiB UTF-8 text and a 512-byte family name.
This limit applies to opt-in measurement, not existing explicitly sized text.
Serialize font-resource changes and scene edits on the host editor thread; measurement is a snapshot, not a lock spanning arbitrary host operations.
Font installation/loading and GPUI's native font-resolution caches remain host
responsibilities. This API invalidates Flow metrics and reflows; it does not claim
to repair stale native font-registration state or install missing fonts. Font
family/content/size/weight/line-height/width edits automatically use different
cache keys.

## Verification and upstream reference

```bash
cargo test --locked -p gpui-flow-layout --test text
cargo run --locked -p flow-studio --bin text_measure_probe  # no window
cargo run --locked -p flow-studio --bin measured_text       # interactive example
```

The no-window native probe uses real installed fonts for variable glyph widths,
bold, CJK, emoji, wrapped content, empty text, trailing newline, explicit missing
family fallback, zoom round-trip, invalid inputs and reactive one-Undo geometry.
Pure tests use named fixture metrics only to verify transaction and ownership
contracts; they are not production width estimators. Native painter wiring has
scale/alignment/rectangle and cache-boundary tests. This batch has not received
manual window/pixel visual acceptance. Linux results do not establish Windows or
macOS font identity or pixel equality; native font metrics can differ by platform.

Official source anchors reviewed: leafer-ui commit
`1e11641d3ea6b0211c83447268a90b8d58feec25`,
[`TextRows.ts`](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextRows.ts),
[`TextLayout.ts`](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextLayout.ts),
and [`Text.ts`](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Text.ts).
The shared concept is real font measurement feeding line boxes and automatic
bounds; this Rust API and its size ownership policy are intentionally explicit.
