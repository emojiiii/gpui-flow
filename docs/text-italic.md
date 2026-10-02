# Text italic face and weight factory coverage

Status: bounded Linux real-font, model and native side-by-side acceptance passed
on 2026-10-02. This is face selection, not complete typography parity.

## Contract

Text.italic is a boolean, false by default and omitted from old-style JSON.
It selects GPUI's native FontStyle::Italic with the component's existing family
and numeric weight. It does not skew an upright bitmap or synthesize geometry.
Font matching/fallback stays with the same host text backend; whether a family
has a distinct italic face is a platform/font resource property.

The value travels through TextMeasureRequest, intrinsic/reactive layout,
measurement-cache keys, native resource preparation, actual native text painting,
CPU glyph rasterization and shared ellipsis shaping. Font/glyph identity remains
within one Application. Changes invalidate resource tickets and composite keys;
late upright jobs cannot republish after an italic change. Ordinary scene/history
transactions apply, including intrinsic-size reflow and Undo/Redo.

Review also found that Text already supported font_weight but its closed factory
schema did not list that field. The schema now accepts font_weight with default
400 and italic with default false. Existing u16/integer and 1..1000 model limits
remain enforced; malformed or out-of-range creation is atomic.

The public flow-text TextMeasureRequest gains `italic: bool`; downstream Rust
struct literals must set false for their old behavior. Repository callers and
examples were migrated. There is no new dependency or font installation.

## Pinned alignment and differences

The pinned [Leafer Text component](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Text.ts)
uses an italic boolean in its font descriptor and treats it as a bounds-affecting
input. Flow routes the flag through its actual native measurement instead of
using Leafer's fixed fontSize*0.16 box-width expansion. Explicit Text boxes and
padding remain unchanged and retain their existing clipping contract; natural
measurement continues to use actual line advances/line boxes, not a guessed
italic overhang. This is not a promise that all glyph ink lies within advances.

Text decorations, custom font features/axes, letter/word/paragraph spacing,
vertical writing and rich text remain separate gaps. Italic does not relax
Text's existing positive-uniform world-transform restriction.

## Validation (2026-10-02)

- Nine new regressions: two scene/default/box, two factory/resource, one reactive
  intrinsic layout/history, one metric cache and three actual-font/UI cases
- 157 affected checks passed: 132 UI library, 10 optional-GPUI text library,
  14 targeted scene/engine/text-layout tests and one contracts-only doctest.
  Zero failures/ignored. Earlier focused tests overlap and are not additive;
  unchanged whole-workspace modules were not broadly rerun
- Verified distinct resolved font IDs and different actual glyph bitmap bytes,
  actual measurement against shaped line advances, bold-italic ellipsis fitting,
  stale text tickets/jobs, ordinary reflow/Undo/Redo and fixed-box geometry
- Strict affected all-targets Clippy, four migrated example/probe Clippy targets,
  formatting, locked checks and real native link passed. No new dependency,
  font package or substitute shaping engine was required
- Native cloud window ran 10:45:59–10:48:50 UTC and exited 0 with empty stderr.
  Native and CPU paths visibly agreed for upright, italic, bold italic, changed
  wrap points, ellipsis/clip modes, nowrap Unicode/CJK fallback, repeated face
  replacement and five-step Undo to the initial upright scene. Explicit boxes
  remained fixed; the font change affected actual glyphs and line layout
- Window inventory confirmed closure and screenshots remain in the native tool
  record. Other font installations, platform fallback/synthesis choices, actual
  glyph-overhang containment and cross-platform rendering were not accepted here

Exact source tree: `bf50b5de97eed0edc896b8ebf6c9a3f439a2b588`.
Deterministic archive SHA-256:
`9af90732f42fad4e832510e85737264d2e4294f4f4bc183f43a7bae8a4d43cd2`.
Linux example binary SHA-256:
`5b64359dea405a6f303b2915223eacc26fbf0e8f8c5a526867f6b4d5469abc11`.
Only documentation changed after these tests and the native run.

Run `cargo run -p flow-studio --bin text_italic` for the side-by-side native/CPU
font, weight, wrap, ellipsis, Unicode and Undo demonstration.
