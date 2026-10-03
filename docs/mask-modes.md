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
The compositor keeps conservative allocation bounds. Complete Leafer mask layout
and hit rules, arbitrary external host backdrops and container/Text Path erasers
remain outside this increment, so APP-03 remains partial.

Run `cargo test -p gpui-flow-composite mask_mode_tests` for pixel, blend, eraser,
resource and budget regression checks. The existing `compositing` example now
cycles all modes, toggles marker paint/visibility/opacity, and supports Undo.

Semantics are based on pinned Leafer [mask scheduling](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/mask/src/index.ts)
and [branch clipping](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/BranchRender.ts).
Pixel regression results describe this CPU implementation, not browser/GPU bit identity.

Validation: 13 focused pixel/admission tests, the shared 1,923-default/304-UI aggregate and strict checks passed. The existing Linux example exercised all five modes, paint/visibility/opacity changes and Undo, then exited normally; screen captures are lossy JPEG, with exact CPU assertions kept in tests.
