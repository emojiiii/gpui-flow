# Owned background blur

`Style::background_blur` and `PropertyKey::BackgroundBlur` add a finite scalar
in `[0,256]`, defaulting to zero for older documents. Zero, including negative
zero, preserves the previous pixel, isolation and budget path. Invalid values
reject atomically; Scene JSON, engine changes and Undo/Redo retain the scalar.
The [background_blur example](../examples/studio/src/bin/background_blur.rs)
demonstrates the supported composition and explicit error cases.

## Pixels and ownership

Each operation samples the immutable, ordered paint prefix before its own paint.
`NativeOwnedCanvas` extends its declared background and procedural grid over the
admitted working rectangle with the same anchor. Raster export extends its
explicit requested background. A Normal/opacity isolation owns a separate
transparent surface; children never borrow outer pixels. Text, Image and Canvas
prefix contributions come from the same immutable request snapshot.

The coverage mask is the antialiased filled contour, independent of paint
presence, for Rect, Ellipse, Polygon, Star, Path, Pen, Box and Frame. Line has
empty filled coverage. EvenOdd holes and implicit Path closure are retained.
Own shadows, foreground blur and grayscale affect the separately completed
foreground F. For prefix B, its Gaussian G, contour coverage m and node opacity
times inherited clip q, composition is:

`B1 = (1-m)B + mG; R = F + (1-F.a)B1; output = (1-q)B + qR`

This uses premultiplied encoded-sRGB byte units, f64 interpolation and one final
rounding, maintaining RGB ≤ alpha. It does not source-over a captured backdrop
onto itself. Zero q, or zero coverage with empty foreground, preserves exact B.
The sampling halo does not enlarge geometry, hit targets or analytical write
bounds.

## Limits and unsupported contexts

Active physical sigma must be `[0.001,64]` under a positive similarity transform,
including DPR, with the existing `ceil(4*sigma)+2` sampling guard. The working
rectangle unions the output and every active sample, including earlier offscreen
effects. Distant samples can therefore exceed the budget despite a small output.
Admission charges surfaces, masks, initialization, copies, crop, foreground and
filter work before pixel allocation. Existing limits remain: 4096 per dimension,
4,194,304 pixels, 128 MiB known working set, 32 million work units and depth 16.
Planning and row loops check cancellation; the bounded Gaussian call checks at
its boundary. Failure withdraws the whole result rather than publishing a prefix.

Typed refusal covers Group/Text/Image/Canvas/Custom targets, artistic target
blends, active mask runs, eraser-forced owners or ancestor Pixel replay,
synthetic root eraser markers, ancestor foreground-effect replay, nested active
background effects, and newly isolated PassThrough children requiring an external
prefix. Earlier completed mask/eraser subtrees can contribute ordinary prefix
pixels. A fresh internal Normal owner is distinct from an outer eraser owner.
Path erasers retain geometry-only precedence.

Ordinary Domains mode and root Canvas draws cannot acquire an external backdrop.
LayeredApp retains its OwnedCanvas refusal. SVG, strict Leafer interchange and
direct native rendering reject these effects according to their visibility
contracts. The pinned Leafer BackgroundBlur implementation is empty; this is a
Flow capability, with no browser-pixel-equivalence claim.

## Validation and invalidation

An active background domain requires complete source compatibility. Paint,
viewport/DPR, resource, Canvas generation, font and animation changes participate
in frame identity and upload authority. Stale completions cannot replace current
frames; supported failures withdraw and recover through the existing controller.

Focused regressions cover independent rational pixel composition, Gaussian/crop
and effect order, geometry holes, ownership/refusals, exact budgets, cancellation,
serialization/history and UI resource invalidation. The four-scenario Linux
example check exercised mixed vector/Text/Image/Canvas prefixes, sequential blur,
pan/zoom, transparent owners, unsupported backdrop, resource replacement and
whole-frame failure/recovery, then exited normally. Desktop JPEGs establish
visible behavior; exact pixel and budget claims come from CPU tests.

Together with [shadow modes](shadow-modes.md), [foreground blur](foreground-blur.md)
and [grayscale](grayscale.md), this supplies the fixed APP-04 effect set within
the documented owned-surface boundaries.
