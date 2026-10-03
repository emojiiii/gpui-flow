# Mask modes

A Group's stable z-ordered direct children delimit mask runs. At least one visible,
positive-opacity marker enables the schedule. Within an active schedule every
marker ends the previous run. With no active marker, ordinary children render.
This corrects the earlier sole-hidden-marker behavior, which hid the following
contents; callers relying on that behavior should hide the content group explicitly.

- `Alpha` uses the marker's complete rendered alpha
- `Grayscale` uses rendered luminance × alpha (encoded RGB weights .299/.587/.114)
- `Path` clips with the filled geometric contour, independent of paint alpha
- `Clipping` and `ClippingPath` additionally paint the marker before the masked run

Opaque Path runs clip the actual parent surface, preserving its blend/eraser
backdrop. A translucent Path run isolates the contents and applies marker opacity
once. Group Path masks intersect their visible positive-opacity child contours;
Box/Frame use their own contour, and Image uses its rounded layout rectangle
without requiring decoded pixels. Empty leaf paths clip to empty; a Group with
no contributing paths adds no clip. Winding rules and affine transforms apply.
Text/Custom geometric paths are explicitly unsupported; alpha/grayscale modes
continue to use existing producers. No glyph outline is replaced with a text box.

Native JSON uses snake_case. Leafer JSON exchange still explicitly rejects masks.
The compositor keeps conservative effect/allocation bounds, independently of the
logical bounds below. Arbitrary external host backdrops and container/Text Path
erasers remain outside this increment, so APP-03 remains partial.

## Layout and picking

When any direct marker is active, Group logical bounds include the stable
z-ordered prefix through the first marker. Without an active marker, ordinary
child bounds return. Reorder, visibility and opacity changes update cached
ancestor bounds; crossing the last marker's zero-opacity boundary also withdraws
stale composite placement. This replaces the earlier all-child Group bounds.
Conservative effect bounds still include the full rendered extent.

With `hitChildren=true`, content keeps its own geometric picking, including
content outside transparent mask pixels. A non-self-painting mask leaf requires
an overlapping ordinary sibling hit; Clipping variants also paint and hit their
own shape. With `hitChildren=false`, active mask branches instead select the
parent Group, using the first-marker bounds gate, real ancestor clips and world
hit tolerance. Only literal Path markers force paint-independent geometry in
this mode. Duplicate parent hits are removed and local coordinates use the parent.

Accepted Image alpha is sampled before mask-parent resolution, so transparent
Image pixels cannot create a proxy hit. Geometric Path Image markers deliberately
need no decoded pixels. Locks, visibility and attachment checks still apply.

Run `cargo test -p gpui-flow-composite mask_mode_tests` for pixel, blend, eraser,
resource and budget regression checks. The existing `compositing` example now
cycles all modes, toggles marker paint/visibility/opacity and hit-children policy,
shows current logical width and pointer target, and supports Undo.

Semantics are based on pinned Leafer [mask scheduling](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/mask/src/index.ts)
and [branch clipping](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/BranchRender.ts).
Logical bounds and picking also follow the pinned
[LeafBoundsHelper](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/helper/src/LeafBoundsHelper.ts).
Pixel regression results describe this CPU implementation, not browser/GPU bit identity.

Rendering validation includes focused pixel/admission regressions and the existing
Linux example cycling all five modes, paint/visibility/opacity changes and Undo.
Screen captures are lossy JPEG; exact CPU assertions remain in tests.

Layout/hit regressions cover ordering, activation, ancestor gates, image-alpha proxies
and stale placement. The Linux example physically confirmed width 240 → 255 → 240
on hide/Undo, ordinary child hits outside the mask and paintless Path parent hits.
