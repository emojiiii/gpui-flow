# Bounded motion-path queries

This implements the distance/point-query portion of the pinned Leafer
[motion-path plugin](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/motion-path/src/index.ts),
where `getMotionTotal` reads measured path data and `getMotionPoint` samples a
distance and transforms the result. Rust exposes explicit, read-only Path/Pen
queries instead of installing JavaScript prototype properties or layout hooks.

## API and behavior

- `Path::measure()` returns an immutable `PathMeasure` snapshot. Retain it for
  repeated queries. `total_length()` is a local-space arc length; `sample(d)`
  and `sample_fraction(t)` return an optional `PathSample` containing position,
  forward unit tangent and rotation in degrees.
- Commands M/L/Q/C/Z use their real curve geometry. Moves add no distance and
  never connect disjoint subpaths; Close adds the edge back to the subpath start.
  Finite distances/fractions clamp to the path endpoints. Empty, move-only and
  entirely zero-length paths return total zero and no sample.
- A boundary belongs to its preceding segment. At a stationary cusp the sample
  uses the incoming tangent; immediately after the cusp it uses the outgoing
  tangent. No unique mathematical direction exists exactly at that cusp, so
  this is an explicit API convention rather than claimed upstream parity.
- `PathSample::transformed(affine)` transforms the position and direction,
  renormalizes the tangent and computes rotation from that direction. Reflection,
  nonuniform scale and skew are supported. A collapsed tangent, nonfinite
  transform or overflowing sample is an error, not an invented rotation.
- `SceneEngine::measure_path(handle)` and `motion_total(handle)` measure current
  local geometry of an attached Path/Pen. `motion_point(handle, distance)` uses
  the current committed ancestor matrices to return a world position/tangent.
  Its distance remains local, so scaling does not make this a world-distance API.
- Engine queries create fresh measurements and do not write document state,
  revisions or history. Retained snapshots remain snapshots. Path edits and parent
  transform edits affect subsequent engine queries without a stale geometry cache.
  Foreign, stale, detached and wrong-kind handles reject explicitly.

## Reuse, precision and limits

The existing locked Kurbo 0.11.3 supplies curve integration, inverse arc length,
Bézier evaluation/derivatives, degree elevation and extrema subdivision. No new
geometry dependency or display-flattening approximation was added.

Quadratics are elevated exactly to cubics before integration. Regression tests
found nonfinite coincident-control and inaccurate collinear-cusp behavior through
the pinned quadratic length/inversion route. Kurbo's cubic integrator and its
monotone subdivisions avoid those cases while retaining the same curve.

The requested summed integration error budget is 0.001 local units, subject to
floating-point roundoff. Kurbo divides each input curve into at most five monotone
pieces. The input retains the [Path data budgets](path-data.md): 16384 commands,
finite +/-1000000 coordinates, and 1 MiB normalized SVG data. Length accumulation
that cannot represent a positive increment rejects instead of silently dropping
geometry. Imported arcs retain the SVG adapter's existing cubic approximation.

## Deliberate remaining scope

There is no automatic follower binding, motion tween, path reveal/growth,
normal/anchor offset, sibling path discovery, text-on-path, world-distance
parameterization or render-loop scheduling in this query slice. Other shapes
must be converted to Path explicitly. It adds no interactive/native UI tool.

## Validation

Fresh Linux/Rust 1.98.1 validation on 2026-10-02:

- 17 new regressions pass: 12 scene metrics tests and 5 engine integration tests.
  Coverage includes line/quadratic/cubic/closing distances, disjoint subpaths,
  zero-length paths, clamping/nonfinite input, endpoint degeneracy and stationary
  cusps, immutable snapshots, size/numeric budgets, full-affine tangent transforms,
  both element kinds, changed source/ancestor transforms and invalid handles.
- Four affected headless crates: 328 tests including doctests pass, zero failures,
  one existing ignored radial-raster performance benchmark. Strict Clippy
  `--all-targets -- -D warnings`, workspace rustfmt and `git diff --check` pass.
- The same isolated headless validation method as [Path data](path-data.md#validation-status)
  was used, with byte-identical source files and unchanged retained dependency
  versions/checksums. No manifests, lockfiles or dependencies were changed.

Full workspace/native UI/cross-platform checks were not rerun. No GitHub Actions,
deployment or merge was launched. Earlier recovery results were not counted as
validation of these new measurement APIs.
