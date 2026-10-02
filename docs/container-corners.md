# Box/Frame per-corner clipping (validation in progress)

This checkpoint propagates the typed independent radii from Rect into Box/Frame
painted geometry, factory schemas, ClipRegion, retained clip caches, CPU masks
and the native vector/gradient clip paths. The existing scalar/default JSON is
preserved. New `with_corners` data uses the same pinned fallback normalization
and locked Kurbo geometry as [Rect](rect-corners.md).

`Shape::child_clip()` now carries `(bounds, scalar_radius, optional_corner_radii)`.
`ClipRegion` has the corresponding optional radii, and its `local_rect()` returns
the real clip shape. Picking uses the same per-corner geometry. Conservative
subtree/visibility bounding boxes remain rectangular broad-phase bounds.

Native Text/Image still require truly rectangular ancestor clips. Nonzero radii
must explicitly reject that native path, never turn into a rectangular bounding
box approximation. An explicit all-zero array overrides the scalar and remains
rectangular. CPU composite clips support independent corners through real masks.

The new public model/runtime fields and child_clip tuple require external Rust
struct literal/destructuring updates; repository call sites are migrated.
`rounded(radius)` clears a prior optional array, while `with_corners` overrides
the scalar. No first-class Canvas resource or other unimplemented component is
claimed by this work.

## Current checkpoint

Headless cargo check for the eight-crate subset passes. New geometry, cache,
pixel/history and native adapter regressions are being added; full affected and
native validation are not complete. This source checkpoint is intentionally
labelled WIP. No GitHub Actions, deployment or merge is launched.
