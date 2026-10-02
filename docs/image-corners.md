# Image corners and geometry-driven compositing

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

## Validation

Fresh Linux/Rust 1.98.1 validation on 2026-10-02:

- 16 new regressions: four scene, two engine, one editor, seven CPU and two
  native-adapter tests. They cover legacy JSON/defaults, clamping/precedence,
  invalid rollback, real geometry and fit hits, unchanged-bounds dirty/history,
  decoded resource-ticket reuse, three-mode real pixels/letterboxes, nested own
  and ancestor masks, affine placement, real shadow alpha, missing-source failure,
  exact work/peak admission, deliberate native/CPU planning and animated late-frame
  rejection after a corner-only edit.
- The admission test exposed that preflight previously activated only for shadow
  domains. Rounded image masks now also activate that same preflight; one-byte
  and one-write-under-budget cases reject before painting, not after partial work.
- Affected UI/compositor libraries: 310 tests passed. Scene/engine/editor library
  and integration tests: 354 passed, with one pre-existing ignored radial raster
  benchmark. Total 664, zero failures; this is not the entire workspace matrix.
- Native all-target check, five-package strict all-target Clippy, workspace
  rustfmt, diff checks and the `image_corners` example link pass.

The actual Linux window ran 08:45:01–08:49:21 UTC. Cover/Fill native corners and
Contain CPU letterboxes, corner swaps, opposite own/ancestor corner intersections,
rotation, live GIF frame changes, corner editing during animation and repeated
Undo back to the original static/native-fast-path state were visually observed.
Short asynchronous Compositing indicators settled to complete images; no
bounding-box substitution or persistent rejection appeared. Normal close exited
0 with an empty runtime error log. Screenshots were inspected at settled states.

Code/test inputs stayed unchanged: tree
`fd8d65925b6dbf108250488f0e2ee11d982b16f3`, executable SHA-256
`dfd1cd1a446de2444da42a31b800d7ce91d61551eb3e853bf6f26094a1c0b29d`.
No Actions, deployment or merge was launched; this bounded Linux receipt is not
cross-platform or arbitrary-scale performance proof.
