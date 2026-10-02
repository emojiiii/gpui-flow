# Affine Text rendering

Status: implemented and validated on Linux. This is bounded affine glyph
rendering with an explicit same-App font bridge.

SceneView now creates a bounded CPU domain for solid plain Text whose world
transform includes rotation, reflection, shear or nonuniform scale. It requires
the same explicitly installed same-Application font bridge used by existing
composited Text. Standalone native DisplayElement still rejects transformed Text;
there is no toolkit-native rotated-text fallback or fake rectangular ink.

The pinned [Leafer render module](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/LeafRender.ts)
sets the element's world matrix before drawing; its
[Text component](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Text.ts)
uses local text layout under that render contract. This slice independently
implements that local-layout/world-pose separation using existing Flow primitives.

`display/composite_text/transform.rs` preserves the previous axis-aligned positive
uniform raster metrics exactly. For general invertible affines it reuses
`Affine::max_scale` (largest singular value) to choose an isotropic local glyph
raster density, then reuses the compositor's existing bitmap transform. No font
engine, glyph rasterizer, affine math library or dependency is added. Padding,
actual wrap, alignment, italic face, ellipsis and decoration remain in local
coordinates; intrinsic measurement and the retained fixed box do not change.
Host font rounding and bilinear image resampling do not promise browser pixel
identity, and extreme anisotropy can intentionally exhaust the source budget.

Viewport clipping projects the visible screen rectangle back into local space
conservatively. Effects still request the complete source box before clipping.
The additive `TextInputs::insert_clipped` API supplies an exact local content mask
for affine-produced pixels; it prevents pixel-rounding margins from becoming
outside-box ink. Existing `insert` clears any previous clip metadata, preserves
its old behavior and requires no TextImage struct-literal migration. The mask
runs before effects and affine painting and uses existing scratch/work admission.

Singular/nonfinite transforms, invalid clip geometry and existing glyph/font/
source-pixel/memory/work limits reject explicitly. Missing font bridges and
unsupported Text paints remain explicit errors. Source/attachment/font epochs,
transform changes and Undo reuse existing request-key invalidation. This does
not add transformed text editing, affine transform handles, arbitrary Text paints,
rich text, font axes, cross-platform native acceptance or browser parity.

Effect-specific transform restrictions are unchanged. For example, the bounded
outer-shadow implementation still requires its supported positive-similarity
transform; enabling affine glyphs does not enable arbitrary affine shadows.

## Validation

Nine new tests cover unchanged uniform raster metrics, existing maximum-scale
math, inverse visibility projection, full effect sources, singular/extreme bounds,
real rotated/reflected/nonuniform/sheared glyph pixels, local padding/decoration/
ellipsis, missing bridge and unsupported paints, stale transform jobs and exact
Undo, source budgets, local-mask admission and metadata replacement. An earlier
ancestor-clip regression now verifies supported rotated glyphs while retaining
the missing-bridge error.

Affected tests: **376 passed, zero failed or ignored** (150 UI + 226 composite).
Strict affected all-targets Clippy, example Clippy and real native linking passed.
No production source changed during the native run.

`text_affine` ran 2026-10-02 12:20:10–12:23:50 UTC and closed normally with exit 0
and empty stdout/stderr. The window showed actual 30°/90° rotation, reflection,
nonuniform scale and shear aligned with vector outlines; rounded ancestor clips,
nowrap/ellipsis/clip, italic/Unicode, decorations and opacity also worked.
Withdrawing the font bridge removed glyphs and reported TextBackendRequired;
restoring it recovered the previous artwork pixel-for-pixel. Eleven Undo actions
restored the initial identity artwork exactly. Screenshots and the close receipt
were retained with the validation artifacts.

Exact source commit `fd70a23915b33fac69aba13fd48c7fd5742d610e`, tree
`5fbc4a8785e93f36d23248aedca14a62119c0e72`; archive SHA-256
`e6852210de91212f2153cc1c55b25b1ece30fe6536b23f05cadd2df6f63afb77`;
binary SHA-256
`13104691820dd2a29ce1029368f8ae2857bc79f718deb7c6494528077b9b606a`.
This is Linux native evidence, not cross-platform typography or browser parity.
Run `cargo run --locked -p flow-studio --bin text_affine` to reproduce.
