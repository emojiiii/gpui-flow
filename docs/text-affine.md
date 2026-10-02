# Affine Text rendering

Status: recoverable source checkpoint. Nine new focused pixel/transform tests and
strict affected Clippy pass; broader checks, native linking and window acceptance
are pending.

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
