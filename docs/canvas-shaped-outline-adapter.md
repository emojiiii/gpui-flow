# Canvas same-App outline adapter

`gpui-flow/canvas-stroke-text` forwards
`gpui-flow-text/static-outline-registration`. It enables only the Canvas command
adapter; the host must still create its App and bridge using the supported
static-font registration startup factory and an explicitly supported native
backend. Default builds keep ordinary fills, vectors and export behavior, and
return an explicit feature-required error for Canvas stroke-text jobs.

`SceneView::draw_canvas_commands` captures the font epoch and the consuming
App's `cx.text_system()` owner. Its optional
`draw_canvas_commands_with_cancel` overload also accepts cancellation. Both use
the guarded font route and the resource guard after candidate upload. Failure
before either publication boundary leaves the prior pixels, generation and
current path current. Existing resource events and repaint notification are
preserved.

Each stroke job uses the ordinary isotropic text raster transform with
`full_source = true`. Its shaped producer validates every actual run, including
original and ellipsis-probe runs, without skipping transparent or offscreen
jobs. Empty text still requires a capable backend. Nonpositive or nonfinite
content boxes are explicitly rejected rather than treated as identity-free
success; normal Canvas text validation rejects them before preparation.
Unsupported decorations remain errors.

The adapter concatenates all positioned glyph contours into one Path. It undoes
only the positive isotropic raster matrix, preserving unsnapped positions,
quadratic and cubic controls, closes and counters. The existing Canvas
compositor then applies the original affine transform and DPR exactly once.
Canvas lowering owns the exact content clip, stroke color, width, cap, join and
dash behavior. Unhinted outline geometry does not promise hinted fill pixel
identity.

One private budget spans the batch. Remaining global surface/path/work quotas
and caller-reduced outline command/path allowances bound extraction. Each job's
command cap is also reduced to the per-path point limit before extraction.
Accounting retains the capacities of the output vector, copied IDs, Path
vectors and still-live glyph vectors. Request content/font strings are charged
before copying. During conversion the glyph vectors and concatenated Path are
both charged; glyph/request storage is released only after it is dropped.
Actual allocator capacity above a reservation is checked before exposing the
allocation. Parsing, validation and conversion work is never refunded.

Remaining fill rasterization shares the aggregate retained-byte and work
allowance, including its temporary request strings. `CanvasFontOutput` reports
outline preparation work separately from the unchanged fill `pixel_work` field;
the compositor checked-adds the two before rasterization. All returned Path and
output-vector storage remains charged while fill inputs are prepared. Immutable
font bytes, host shaping caches and shared shaping-helper scratch retain the
producer's separately documented scope; these budgets are not exact process-RSS
measurements.

Pure headless adapter tests cover typed error mapping, Q/C conversion,
fractional/negative positions, anisotropic and reflected raster spaces,
DPR-independent coordinates, lower shared budgets, simultaneous conversion
storage, retained output capacity, cumulative work and atomic conversion
failure. Surface tests cover explicit unavailable capability for hidden/empty
text, invalid content boxes, ordinary vectors/transparent fills and cancellation.
They do not construct a native App or establish new native font identity or
Canvas stroke visual acceptance. The shaped producer and native identity gate
have separate evidence; end-to-end native Canvas stroke acceptance remains a
later gate.
