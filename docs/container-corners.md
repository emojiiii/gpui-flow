# Box/Frame per-corner clipping

This slice propagates the typed independent radii from Rect into Box/Frame
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

## Validation

Fresh Linux/Rust 1.98.1 validation on 2026-10-02:

- 12 new headless regressions: six engine, two editor and four CPU pixel tests.
  They cover Box and Frame defaults, real independent corner hits, affine and
  nonzero-origin clips, nested intersections, clipping toggles, unchanged-AABB
  descendant cache invalidation, factory schemas, invalid-update rollback,
  cancellation/Undo/Redo and equivalent internal/external-domain mask pixels.
- Four new native adapter regressions cover every vector paint layer, nested
  geometry intersections, gradient cache invalidation on corner-only edits and
  explicit Text/Image rejection/zero-array override. All 102 UI library tests
  pass, using the real GPUI dependency and native linker.
- The eight-crate headless suite passes 838 tests including doctests, with no
  failures and one existing ignored radial-raster performance benchmark. Strict
  headless all-target Clippy passes. Final-source full native workspace all-target
  `cargo check` and strict `cargo clippy -- -D warnings` also pass, including Studio.
  The real full-workspace library/integration suite also passes 1215 tests,
  zero failures and eight existing ignored tests (doctests were not part of that
  command). Workspace rustfmt and `git diff --check` pass. The copied manifest arrangement is described
  in [Ellipse validation](ellipse-arcs.md#validation).

The native adapters are exercised through prepared triangle pixels and eligibility
checks, not a live window/GPU presentation. Cross-platform and actual native-window
interaction remain unverified for this slice. No GitHub Actions, deployment or
merge was launched.
