# Finite Leafer JSON exchange (DAT-03)

`Scene::from_leafer_json`, `Scene::to_leafer_json` and
`Scene::replace_from_leafer_json` implement an explicitly limited, strict data
exchange contract. It is independent of `Scene::from_json` / `to_json`: the
existing native Scene JSON version remains **1**, with no migration or schema
change. This is **partial Leafer JSON support**, not complete Leafer compatibility
or a promise of identical browser/GPUI pixels, text metrics or interaction rules.

## Entry points and publication

```rust
use gpui_flow_scene::Scene;
let scene = Scene::from_leafer_json(r##"{
  "tag":"Group", "id":"root", "children":[
    {"tag":"Rect", "id":"r", "width":40, "height":30,
     "x":10, "scaleX":-1, "fill":"#33669980"}
  ]
}"##)?;
let leafer = scene.to_leafer_json()?;
let mut destination = Scene::new();
destination.replace_from_leafer_json(&leafer)?;
assert_eq!(destination, scene);
# Ok::<(), gpui_flow_scene::leafer_json::LeaferJsonError>(())
```

Input is one node object or an array of root node objects; output is always a
root array. It never invents a synthetic Group/Leafer root. Children and roots
retain insertion order, parent links and `zIndex`; export does not reorder the
structural tree into paint order. IDs must be unique and nonblank. Missing IDs
are deterministically assigned `leafer-N` in preorder. A collision with an
explicit ID is an error, not an implicit rename.

Parsing, field admission, construction and authoritative Scene validation happen
on a private candidate. A late failure discards the entire candidate. Replacement
publishes once, only after success; failed export leaves the source unchanged.
Errors carry a path and reason. No JavaScript executes, and importing `url` only
stores the reference: no image fetch, decoding, canvas allocation or plugin
registration takes place.

## Supported fields

Unknown fields, unsupported values and fields on the wrong tag are errors, even
when their value is null, empty, false or resembles a default. JSON duplicate
keys are errors at every depth, including user metadata.

Common fields:

- `tag`, `id`, `name`, whitespace-separated `className`, object/null `data`
- Boolean `visible` and `locked`, integer `zIndex`, `opacity` in `[0,1]`
- `x`, `y`, `scaleX`, `scaleY`, `rotation`, `skewX`, `skewY`
- `fill` / `stroke`: null or CSS hex solids `#rgb`, `#rgba`, `#rrggbb`,
  `#rrggbbaa`; `windingRule`: `nonzero` or `evenodd`
- When a non-null stroke exists: scalar nonnegative `strokeWidth`,
  `strokeAlign` (`inside`, `center`, `outside`), `strokeCap` (`none`, `round`,
  `square`), `strokeJoin` (`miter`, `round`, `bevel`), `miterLimit`

Shape-specific fields:

| Tag | Geometry and declared restrictions |
| --- | --- |
| Rect | Positive `width`, `height`; scalar or exactly four clockwise `cornerRadius` values |
| Ellipse | Positive `width`, `height`, `startAngle`, `endAngle`, ratio `innerRadius`, Boolean `closed`; existing one-revolution arc/ring validation applies |
| Line | Exactly two explicit flat/object `points`, or `width` from local `(0,0)` to `(width,0)`; `closed` must be false |
| Polygon | Explicit flat/object `points` and Boolean `closed`; no regular-polygon generator, curve smoothing or path override |
| Star | Equal positive `width` and `height`, integer `corners` from 2 to 4096, ratio `innerRadius`, `startAngle`; centered on half-size |
| Path | Explicit SVG string or finite numeric SVG opcode array `path`, using the bounded `Path::from_svg` / `to_svg`; omitted path/implicit box geometry and Canvas-only numeric opcodes are unsupported |
| Text | Explicit positive `width` and `height`, string `text`, `fontFamily`, `fontSize`, numeric hundred-step `fontWeight` from 100 to 900, `italic`, `lineHeight`, `textAlign`, `verticalAlign`, `textWrap`, `textOverflow`, `textCase`, `textDecoration`, `paraSpacing`, `letterSpacing`, `wordSpacing`, `paraIndent`, `autoSizeAlign`, `writingMode` (`x`, plus the [Flow axis extension](text-layout-properties.md#upright-cluster-writing-modes)), scalar/four-value `padding` |
| Image | Explicit positive `width` / `height`, nonempty `url`, corners; URL implies stretch/`ImageFit::Fill`, with no separate `fill` |
| Canvas | Positive `width` / `height`, corners, `pixelRatio`, `smooth`, `safeResize`; configuration only, no pixels/commands/context/URL snapshot |
| Group / Pen | Ordered `children`; no intrinsic dimensions; Pen retains a distinct structural kind |
| Box / Frame | Positive `width` / `height`, corners, `overflow` (`show` / `hide`), ordered `children` |

The pinned Leafer `Pen` is a branch containing styled Paths. It maps to an
explicit `Group::pen()` / `GroupKind::Pen` structural kind with canonical tag
`pen_group`; export writes `Pen` again and preserves every ordered child and
its paint. Native Scene v1 persists the kind in Group geometry; ordinary Group
still serializes empty geometry unchanged. The factory exposes `pen_group`.
Use `Group::new()` instead of a Rust `Group {}` literal for ordinary groups.

Flow's existing `display::Pen` remains the independent leaf path builder and
continues to reject direct Leafer export; it is never renamed into a branch.
Child Paths accept the pinned numeric M/L/H/V/C/S/Q/T/A/Z opcodes (including
relative variants), so ordinary Pen path-builder output can be imported. Export
normalizes paths to SVG strings; array spelling and analytical arc commands are
not retained. Malformed/truncated arrays, invalid flags and unknown/Canvas-only
opcodes fail atomically under the same budgets.

Pen builder/runtime fields (`pathElement`, `pathStyle`, `__path`) are not document
inputs and are rejected, as are direct `path` and size fields on the structural
Pen. Custom runtime registrations still have no finite upstream mapping.

### Defaults and retained-state fidelity

Unspecified standard dimensions are 100; Text/Image dimensions must be explicit.
Absent fills are empty, except Frame white and Text black. Stroke defaults follow
the pinned source: width 1, cap `none`, join `miter`, limit 10; alignment is center
for Path/Line, outside for Text, inside otherwise. Existing Flow validation still
rejects invalid combinations, including advanced strokes on Text, Image or Group.
Export writes the actual retained paints and stroke attributes explicitly rather
than relying on different Flow constructor defaults.

Text supports `textAlign` left/center/right, `verticalAlign` top/middle/bottom,
`textWrap` normal/none, `textCase` none/upper/lower, and `textDecoration`
none/under/delete/under-delete. Flow has no unclipped `textOverflow: show`, so
input must explicitly select hide or ellipsis. Text auto sizing, default `show`,
`break` wrapping, title/small caps, arbitrary overflow strings, font-weight names,
vertical/reverse writing modes and decoration objects are rejected. The implemented
nonnegative letter/word spacing, paragraph indentation and auto-size alignment
fields use the [Text layout contract](text-layout-properties.md). Numeric line height is in
pixels and becomes Flow's font-size multiplier; percent-unit objects preserve a
multiplier directly. Export uses `{ "type":"percent", "value": ... }`.

Image fit modes other than Fill, natural-size metadata, separate fills and
nonempty `source_crop` cannot be exported. The source crop is a Flow extension
with no verified Leafer Image field; it is never renamed or discarded.

Container clips and own paints are exchanged as retained geometry/style with
Flow's `BeforeChildren` paint order. Leafer's conditional rendering/isolation
policy is not reproduced by the adapter. Flow's explicit `StrokeAfterChildren`
state is rejected on export because no mapped upstream field represents it.
Canvas runtime pixels remain outside either document representation.

Original input spelling/default omission is not preserved: export writes explicit
retained values, not computed caches. Runtime engine handles, resource tickets,
layout snapshots and subscriptions never belong to the Scene document. Unknown
computed fields are rejected even if null/false; similarly named keys inside
explicit object `data` remain inert metadata. Nonzero computed Text layout origin
is rejected rather than silently converted into an unrelated input field.

Layers, gradients, effects, masks, blend-mode overrides, custom hit policies,
scene-level metadata, live editor/layout/animation/state/resource objects,
callbacks and computed caches are rejected. Object `data` is inert bounded JSON
metadata, not a way to enable unsupported features. Dormant extra model fields
(such as a nonzero scalar radius overridden by a radius array) cannot silently
vanish: export checks the emitted document by importing it and comparing every
object, geometry, paint and tree field.

## Transforms and numerical equality

The source contract is `MatrixHelper.setLayout`, not Flow's `Affine::skew`:
Leafer uses `skew * PI / 180` as a linear shear coefficient, **not its tangent**.
For rotation radians `r`, shear coefficients `sx`/`sy`, and scales `X`/`Y`:

```text
a = (cos(r) - sy*sin(r))*X    b = (sin(r) + sy*cos(r))*X
c = (-sin(r) + sx*cos(r))*Y   d = (cos(r) + sx*sin(r))*Y
tx = x                      ty = y
```

Negative scales/reflections are supported. Nested transforms use the existing
`parent * local` world-transform API and its overflow validation. Export
reconstructs real x/y/scale/rotation/skew fields; there is no invented `matrix`
JSON field. Well-conditioned nonzero diagonals use direct shear decomposition;
other invertible matrices use signed QR-style decomposition. Axis-aligned zero
scales work. Other singular or out-of-range decompositions can be rejected.
The adapter does not retain source angle spelling or preferred decomposition.

Geometry, paints, metadata, identity and ordered tree state must round-trip
exactly in retained representation. Transform coefficients may differ by at most
`4*f32::EPSILON*max(1,abs(source_coefficient))`; trigonometric decomposition cannot
promise bit-identical matrices. An export outside this bound fails. This is a
local-coefficient bound, not an error bound for arbitrarily amplified world-space
points. SVG arcs/shorthand normalize through the existing path adapter, so input
string spelling and exact analytical arc source are not retained.

## Budgets

- 1 MiB UTF-8 JSON input/output; individual decoded strings/keys at most 64 KiB
- 512 nodes; 24 display-tree levels; 64 JSON nesting levels; 65,536 JSON values
- 16,384 cumulative explicit points, path commands and generated star vertices
- Geometry/transform numeric attributes within finite +/-1,000,000, plus their
  existing positivity, enum, opacity, arc, stroke, canvas and Scene constraints
- No truncation, partial imports, nonfinite casts or replacement of invalid
  numbers with null; malformed JSON and duplicate keys fail

These are exchange limits, independent of the native Scene v1 parser's bounds.
Metadata numbers remain JSON numbers and are not narrowed to f32.
Export checks both visited and queued metadata values before expanding arrays or
objects, and rejects unsupported custom geometry and paints before invoking
Scene validation. Programmatically constructed scenes are subject to these
admission limits too.

## Pinned source evidence

Read-only reference inspected at core
[`1d703303ff0759ce4973d54c87eeb8192dddc570`](https://github.com/leaferjs/leafer/tree/1d703303ff0759ce4973d54c87eeb8192dddc570)
and UI
[`1e11641d3ea6b0211c83447268a90b8d58feec25`](https://github.com/leaferjs/leafer-ui/tree/1e11641d3ea6b0211c83447268a90b8d58feec25):

- [UI declarations and UI.one](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/UI.ts): input attributes and defaults
- [Group.set / toJSON](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Group.ts): `tag`, ordered child JSON
- [MatrixHelper.setLayout](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/math/src/MatrixHelper.ts): signed scale, rotation and linear skew composition
- [Numeric path opcodes](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/path/src/PathCommandMap.ts): explicit command IDs and fixed argument lengths
- [Pen and its inherited Group](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Pen.ts): `Pen extends Group`, `setStyle` appends separately styled Paths; Group `toJSON` serializes ordered children
- [Primitive source directory](https://github.com/leaferjs/leafer-ui/tree/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src): Rect/Ellipse/Line/Polygon/Star/Path/Pen/Text/Image/Canvas/Box/Frame tags and attributes
- [ImageData](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/data/src/ImageData.ts): URL creates a stretch paint and removes separate fill from JSON
- [UIData.setPath](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/data/src/UIData.ts) and [PathConvert.parse](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/path/src/PathConvert.ts): an explicit empty string becomes empty command data; omitting `path` instead leaves UI's box fallback, which this subset rejects
- [Text type declarations](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interface/src/type/IType.ts): exact enum spellings and hundred-step weights
- [BoxData](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/data/src/BoxData.ts): conditional clip-after-fill boundary

## Validation

Focused tests are in `crates/flow-scene/tests/leafer_json.rs`: all supported tags,
upstream defaults, solids/alpha, transforms/reflections/nested world geometry,
real export fields, strict unknown/duplicate/type handling, text and crop limits,
all budgets, late atomic replacement failure, export-state preservation and
unchanged native Scene JSON v1.

```sh
cargo test --locked --offline -p gpui-flow-scene --test leafer_json
cargo clippy --locked --offline -p gpui-flow-scene --all-targets -- -D warnings
```

These are data-model tests; no native-window or full upstream browser equivalence
is claimed by this milestone.

The completed finite contract covers thirteen Leafer tags, including distinct Pen
branches, fixed SVG numeric opcodes, strict input/computed-field admission and
unchanged native Scene v1. Thirty exchange tests and thirteen factory tests pass;
the final unchanged-source aggregate passes 1887 default tests/doctests (11
ignored), 304 UI tests, strict all-target checks and formatting. This pure data
module makes no browser/GPUI pixel-equivalence claim.
