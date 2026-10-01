# Bounded leaf inner shadows

`Style::inner_shadow_layers: Vec<Shadow>` stores back-to-front inner effects.
It defaults to `[]` for old documents and serializes even when empty, so removal
is a real engine property change. `PropertyKey::InnerShadowLayers` has array type,
default `[]`, and PAINT-only invalidation. Scene/engine/editor transactions reject
invalid values atomically; history, serialization and state overlays preserve it.

The independent `MAX_INNER_SHADOW_LAYERS` is 8, including transparent entries.
This permits up to eight outer and eight inner effects, but all combined work
must fit the same cumulative budgets. Eight is an input ceiling, not a promise
that every eight-layer scene fits. `Shadow::new(x,y,blur,color)` retains finite
local offsets within ±4096 and blur in [0,256]. Unsupported fields still reject.
There is no single-only inner field that needs migration to add multiple layers.

## Supported producers and exact ordering

Supported producers are the existing bounded vector leaves, Image pixels and
narrow Text glyph pixels. Visible Group, Box, Frame and Custom declarations reject
with `UnsupportedInnerShadow`, even when all colors are transparent. Dormant
invisible/zero-opacity subtrees retain the existing producer-skipping convention;
revealing an unsupported declaration rejects. Containers may contain supported
inner-shadowed leaves without having an inner effect themselves.

1. Compose the leaf's original fill and stroke, or Image/Text ink, without any
   of that leaf's own inner or outer effects, and extract actual source alpha A
2. Each own outer layer independently projects this unmodified source behind it
3. Paint the fill/image/glyph ink
4. For each inner layer, independently compute A(x) × (1 − G(A)(x − offset)),
   multiply by effect color alpha, and source-over its premultiplied color
5. Paint strokes above the inner effects, retaining existing Flow stroke semantics
6. Apply the object's opacity/blend and inherited clip once at its isolation boundary

G is the existing locked image 0.25.10 A8 Gaussian with device sigma = local
blur × effective local-to-device uniform scale / 2. Zero blur bypasses the filter. Existing image bilinear sampling preserves
fractional offsets; positive x tints the left interior edge. Inner effects use the
original alpha for both the inverse projection and clipping. A transparent hole
stays empty, rounded corners retain antialiasing, and no bounding-box fill is used.
Own effects never feed each other. A parent's outer source does include a child's
complete rendered inner result, including any alpha increase and its local opacity.
Masks consume that rendered result; ancestor clips trim the final effect and do
not create artificial inner-shadow edges.

### Translucent zero-offset behavior is intentional

For zero blur and zero offset, inner coverage is A × (1 − A). Fully opaque ink
therefore receives no inner tint, but translucent ink does. With 8-bit source
blue `[0,0,128,128]` and opaque red inner color, rounded inner coverage is 64 and
source-over yields `[64,0,96,160]`. A second opaque green inner layer still uses
the original A=128 and yields `[48,64,72,184]`. Node opacity is applied afterward.
This does not preserve the source's alpha magnitude, but never paints where
source alpha is zero. It is not equivalent to tinting an outer shadow on top.

## Bounds, text sources and cache

Inner effects add no external effect extent. Geometry, layout, hit testing,
selection and existing outer bounds are unchanged. PAINT damage includes the
original bounds and any outer-shadow-bearing ancestor that resamples the result.
Layer changes/order participate in recursive cache identity and conservative
placement equality. An incompatible accepted frame is retired, and a late old
success cannot replace a newer request.

For text sampled by its own or an ancestor's visible inner/outer effect, the host
bridge now requests the full native alignment box rather than pre-cropping it
to the viewport. This preserves offscreen glyph alpha needed at a visible effect
edge or projected into view by an ancestor shadow. Layout, alignment, wrap and
native glyph-overhang clipping to the alignment box remain unchanged. Ordinary
no-effect text keeps its viewport-cropped path. The full source is subject to the
same text dimension/pixel/byte/work limits; oversized input fails explicitly,
without falling back to an incorrect cropped alpha source.

## Admission and limitations

The existing no-pixel-allocation schedule replay includes every inner layer and
all repeated ancestor source captures before the domain output surface. Each
layer reserves the padded RGBA/A8/Gaussian scratch and charges extraction,
filtering, tint, inverse-alpha multiplication and compositing, then releases it.
The source RGBA preserves original A until tinting, so no extra unbounded alpha
cache is introduced. A later invalid layer, tight work/byte budget or excessive
nested replay rejects the whole output before any domain pixel allocation.

The same fixed limits remain: 4096 per surface dimension, 4,194,304 pixels,
128 MiB known working set, 32 million work units, and 16 isolation levels.
Only positive similarity transforms are supported for effects. Cancellation is
checked around the bounded non-interruptible library Gaussian. Backend-private
allocations and the OS remain outside a process-wide memory guarantee.

Spread, box/knockout, per-layer blends, scale-fixed effects, container inner
shadows, Custom producers and general filters remain unsupported. Text retains
its existing style/transform/backend limits. This is not a Leafer JSON importer.

## Leafer reference and intentional boundary

The official [innerShadow API](https://www.leaferjs.com/ui/reference/UI/innerShadow.html)
supports multiple effects. Pinned
[InnerShadow.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/effect/src/InnerShadow.ts)
uses a source-out inverse shadow clipped by the cached shape and iterates in
array order. Pinned
[UIRender.ts](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/render/src/UIRender.ts)
places outer effects before fill/content, inner effects before strokes, with a
special outside-stroke path. Its `__drawShape` normally draws opaque geometric
fill/stroke, retaining pixel alpha for selected image paints. Flow deliberately
uses actual paint alpha consistently with its existing outer effects, including
translucent solids/gradients and glyph ink. Leafer Group and Box/Frame also take
different content/source paths, so this slice explicitly does not support their
inner effects. These are structural references, not browser-pixel equivalence.

Run `cargo run --locked --offline -p flow-studio --bin inner_shadows` for the lab.
`inner_shadow_text_probe` exercises real-font viewport-boundary and ancestor
projection fixtures at DPR 1/1.25/2, including an oversized complete-source refusal.

## Validation

The Linux aggregate passed 867 headless tests/doctests (8 pre-existing ignored),
96 UI library tests, strict workspace/all-targets and documentation Clippy, both
format checks, documentation test targets, both native builds, and the real-font
probe's six DPR/boundary cases plus oversized-source refusal. Counts describe one
final aggregate, not repeated attempts added together. The `inner-shadow-final`
receipt records exit 0, locked/offline dependencies, one job, no debug/incremental
output and a 3 GiB disk guard. Final free space was 7,038,627,840 bytes.

The initial four-object lab used two inner blurs of 8 and 2 plus an outer blur of
6 per leaf, with a 300×110 text box and 60px text. It visibly exhausted the shared
32-million-work budget and reported `Composite text: WorkLimit`, rather than
cropping alpha or rendering a partial effect. The lab defaults were reduced to
inner blur 4/0, outer blur 2 and a 240×80 text box with 48px text. Only that example
was rebuilt/rechecked after the aggregate; no renderer budget was increased.
This is evidence of a real multi-layer size/filter cost limit, not a resolved
performance issue or production-scale certification.

Target-window smoke verification of the corrected configuration is recorded below.


Corrected target-window smoke checks visibly covered layer removal/restoration,
white stroke above inner color, independent black outer shadows, translucent
source paint versus whole-object opacity, array reversal and offset, text blur
and Undo, child clip toggling, explicit Frame rejection and Undo recovery,
repeated toggles, geometric selection bounds, clearing selection outside those
bounds on shadow-only pixels, and wheel pan/Ctrl+wheel zoom. Pan/zoom briefly
showed `Compositing...`, then complete frames returned without persistent errors.
The target window closed normally, disappeared from the inventory, and exited 0
with an empty native log. The initial over-budget run also closed normally.

The real-font probe compares full and deliberately cropped inputs, proving the
old viewport-edge false tint and missing ancestor projection at DPR 1/1.25/2;
it does not claim browser-pixel equivalence. These are smoke checks, not image
goldens or low-latency/performance certification. Close-during-worker races,
live multi-monitor/DPR changes, GPU failures, all prior examples, macOS/Windows,
CI and the eight ignored tests were not exercised. No Actions run was triggered.
