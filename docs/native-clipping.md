# Native rectangular clip chains

Text and Image now render inside nested `Frame` containers whose
`clip_children=true` (the default). This slice supports zero-radius rectangles
under translation and positive uniform scale, including viewport pan/zoom. It
is not arbitrary path masking, rounded native clipping, or blend composition.

## Backend support

| Display path | Ancestor clipping supported |
|---|---|
| Vector primitives / vector custom fallback | Existing affine, nested rounded Frame clips, applied to tessellated triangles |
| Text | Nested rectangular Frame/Box clips, translation and positive uniform scale |
| Image | The same rectangular chain; Contain/Cover/Fill preserve their original full layout box |
| Group | Carries descendant clip state; does not paint its own shape |

`Box` participates when `clip_children=true`. A Frame with clipping disabled does
not contribute a clip. Curved, rotated, skewed, reflected or nonuniform native
clip transforms retain explicit `ClippedText`/`ClippedImage` diagnostics and are
not drawn using an approximate bounding rectangle. Native Text/Image transforms
also require finite, exact positive uniform scale with zero off-diagonal terms;
small nonzero skew/rotation is not silently rounded away. Vector clipping is
unchanged and keeps its broader capabilities.

## Shared geometry and painting

The adapter prepares one of three states from the retained clip chain:
Unclipped, Rect, or Empty. Each rectangle is transformed from its local bounds
through its recorded world transform and then the viewport exactly once. Every
ancestor contributes to the intersection. Nonzero local bounds origins are
preserved. The element's own overflow box further limits visible pixels.

Text continues to shape and align within its original full text rectangle. Its
canvas converts the shared clip to actual window coordinates using the host
origin, then `Window::with_content_mask` intersects it with the host's existing
content mask. The ancestor clip never changes line breaks or alignment width.

Image uses an outer overflow-hidden rectangle and an absolutely positioned inner
image box. That inner box retains the full original dimensions, with negative
relative offsets where clipping removes its left/top pixels. GPUI ObjectFit still
receives that full box: moving a clip cannot shrink or recenter the image's
Contain/Cover/Fill layout. The wrapper also composes with outer host clipping.
It does not fetch remote image URLs or change the existing host asset policy.

An empty/zero-area intersection creates no native drawing element. The Scene and
cached SceneEngine hit paths also reject Text/Image whose box has no positive
visible intersection with ancestor clips. This includes edge-only contact and
remains true with hit tolerance. Boundaries of a nonempty intersection keep the
existing inclusive `Bounds::contains` convention. Bounds hit policy remains box
selection, not image alpha picking or per-glyph picking. Rounded/affine model
clips still perform exact per-clip point containment after the broad phase.

Clip plans and viewport changes are presentation computations; they do not change
Text/Image geometry, font metrics, resource identity or edit history. Ordinary
`clip_children` edits update engine caches and remain ordinary document edits.

## Verification

- Nested transformed Frame fixtures compare the native rectangular plan against
  both Scene and Engine hit results over a grid, at three pan/zoom settings
- Disjoint, touching, own-box-empty and nonempty inclusive-edge cases are covered
- Unsupported rounded/skew/rotation/reflection/nonuniform clips are rejected,
  including unsupported entries after an already empty intersection
- Image regression calls GPUI's actual `ObjectFit::get_bounds` for all three fit
  modes and multiple zooms, proving full-size/negative-offset wrapper equivalence
- Text regression exercises host-origin translation and native ContentMask
  intersection without altering the original text alignment rectangle
- The existing affine/rounded vector regression remains part of the UI suite

```bash
cargo run --locked -p flow-studio --bin clip_chains
cargo run --locked -p flow-studio --bin measured_text
cargo run --locked -p flow-studio --bin text_measure_probe
```

`clip_chains` uses actual nested clipping Frames with text and a local checker
image. Its controls move the inner frame through visible/empty intersections,
toggle outer clipping, cycle image fit and change zoom. The measured-text example
and no-window probe now use a real clipping Frame too. Local model/native tests
and links are not manual screenshot/pixel acceptance; that visual pass remains
unperformed for this slice. Three-platform Actions validation is deferred at the
user's request.

Official reference: leafer-ui commit
`1e11641d3ea6b0211c83447268a90b8d58feec25`,
[`Frame.ts`](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Frame.ts)
sets hidden overflow;
[`UIRender.ts`](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/render/src/UIRender.ts)
saves the canvas, clips content, draws descendants, and restores it. This Rust
slice supplies the corresponding rectangular native adapter contract and retains
explicit diagnostics for the broader upstream clip shapes.

## Image geometry-domain extension

The later [Image corner slice](image-corners.md) adds exact own corners and
automatically routes Images with complex transforms/ancestor clips to the
existing bounded CPU renderer. Direct native Image clipping retains the
rectangular-chain limits described above, while SceneView now renders those
additional Image cases without an axis-aligned approximation. Text's limits
remain unchanged; this extension does not enable Image transform editor tools.

The later [Text ancestor clipping slice](text-ancestor-clips.md) selects a CPU
glyph domain automatically for nonrectangular/affine ancestor masks, when the
host supplies the existing font bridge. Direct standalone DisplayElement
clipping diagnostics and native-only exclusions remain unchanged.
