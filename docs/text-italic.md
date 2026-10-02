# Text italic face and weight factory coverage

Status: recoverable source checkpoint. Focused real-font/model checks passed;
final affected checks and native side-by-side acceptance are pending.

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

## Validation checkpoint

Nine new tests cover legacy JSON and fixed boxes, factory/default/range rejection,
stale resource tickets, reactive intrinsic measurement/Undo, cache isolation,
actual distinct italic font IDs and raster pixels, italic ellipsis width and late
composite-job rejection. Final counts and native receipt follow.
