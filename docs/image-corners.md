# Image corners and geometry-driven compositing (source checkpoint)

Pinned [Leafer Image](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Image.ts)
extends Rect. Flow now adds scalar/independent four-corner Image geometry by
reusing its existing Rect/Kurbo contract, rather than substituting an image's
axis-aligned bounds. `rounded` and `with_corners` share Rect's clamp/precedence;
missing/zero scalar and absent array preserve old Image JSON and constructors.

Image geometry picking and contour export use the real rounded outer box.
Painted picking still intersects that geometry with the document's content-fit
bounds; it is not decoded-alpha hit testing. If natural_size is absent, pure
model picking cannot infer decoded intrinsic dimensions. Loading remains owned
by the existing authorized image resource adapter, with no new fetcher.

The CPU path intersects the actual fitted image rectangle with an own rounded A8
mask and inherited masks, preserving Contain's transparent letterbox rather than
stretching or repeating edge pixels into it. The same mask ledger is replayed
before allocation, including nested effects and inherited-mask scratch.

The locked GPUI image primitive applies its corner radii to the intersection of
the image and layout boxes. That is exact for Cover/Fill under the existing native
transform/ancestor-clip constraints; those use GPUI's existing corner properties
and paint_image for selected animation frames. Rounded Contain uses a CPU domain,
because applying corners to its smaller fitted rectangle would change geometry.

Scene planning also chooses a bounded CPU domain automatically for an Image with
rotation, reflection, shear, nonuniform scale, or nonrectangular/transformed
ancestor clips. A default rectangular axis-aligned image retains its native fast
path. Missing resources or exhausted budgets reject the complete domain; there
is no bounding-box fallback. Corner-only edits keep decoded resource ownership
but invalidate the complete render-source key. Animated images reuse the same
selected-frame clock and generation snapshots.

No new Text clipping, alpha hit policy, image editing/persistence, decode behavior,
advanced image paints/strokes or cross-platform guarantee is introduced.
External exhaustive Rust Image struct literals must include the new fields.

Initial native all-target compilation passes. This is a recoverable source
checkpoint; pixel, lifetime/cache, budget and final native interaction validation
are still in progress. No Actions, deployment or merge is launched.
