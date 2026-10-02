# Resource readiness and native paint acceptance (LIF-02)

## Scope and evidence classes

This acceptance slice uses the existing `SceneResources` tracker, public GPUI
`ImageCache`/`RenderImage`, and the existing `SceneViewEvent::Resource` and
`SceneViewEvent::ResourceView` bridge. It introduces no production loader,
decoder, completion flag, or replacement paint receipt path.

- `examples/studio/src/bin/resource_lifecycle_trace.rs` is a controlled native
  fixture. Its host cache initially returns `None`, then a button publishes a
  real in-memory decoded `Arc<RenderImage>` or a real cache error. The cache
  subscribes `window.current_view()` and calls public `App::notify` on its pending
  consumers when the result settles. It never directly calls tracker `complete`
  or `frame_drawn`, and it never emits fabricated `SceneViewEvent` values.
- The main image uses source `A` or `B`. The host resolver maps each new
  observation to a distinct embedded backend job. Old jobs deliberately survive
  source replacement, detach/remount and destroy/recreate, so releasing an old
  job can be distinguished from releasing the new job. This is intentionally a
  per-generation host mapping: a normal shared cache may legitimately return
  the same already-decoded source to a freshly authorized new ticket. The
  forbidden operation is settling the new observation with an old ticket, not
  reusing a shared source's bytes. The extra image fixtures always fail or are
  denied, and the label uses the real native text shaper.
- The last host canvas in the scene region increments a paint-traversal witness.
  The trace requires a witness before accepting ViewReady/Complete. The actual
  resource receipt still comes exclusively from SceneView's existing final
  canvas paint callback, deferred and revalidated against the current tracker.
  Source inspection plus stale-receipt regressions establish that boundary; the
  host witness alone is not an independent GPU or presentation measurement.
- Headless adapter tests separately prove ownership release and stale-result
  rejection. They do not substitute direct tracker calls for native paint proof.

ViewReady means GPUI paint traversal/submission, **not a GPU fence, successful
texture upload, or compositor presentation**. `Complete` means zero pending
observations and deliberately includes failed and cancelled resources. The
fixture retains old backend jobs to test late delivery; it does **not** establish
hard cancellation, encoded/decoded byte budgets, remote loading, media readiness,
or cross-platform behavior. See [the resource contract](resource-lifecycle.md).

## Authored checks

`crates/flow-engine/tests/resource_lifecycle_acceptance.rs`:

1. A batch of one loaded, one failed and one cancelled image cannot complete from
   the old all-pending receipt. A fresh receipt produces ViewReady then Complete
   once, including all three terminal counts; repeat receipts/redraws are inert.
2. Resetting a document (`reset_scene`) with identical IDs changes the document epoch; an old
   ticket and old frame cannot settle or paint the replacement. Ready/ViewReady
   are permitted again only in the new epoch.

`crates/flow-ui/src/resources/lifecycle_tests.rs`:

1. An externally owned decoded-image cache and two independent Flow adapters
   have separate Arc leases. Disposing one adapter preserves the other adapter
   and the cache. Disposing both leaves the external cache's lease untouched.
   This is a decoded-ownership test, not a backend cache eviction test.
2. A decoded result owned only by Flow is released on disposal, together with
   playback and timeline entries. Disposal is idempotent.
3. Source A/B/A, detach/remount and destroy/recreate reject late decoded results;
   rejected Arcs are dropped and the new ticket stays pending until its own
   result. Old attachment cancellations cannot dispatch to a new instance.
4. Terminal failures and cancellations cannot be overwritten by a late success;
   its decoded Arc is not retained.
5. Replacing the host cache releases Flow's prepared lease without evicting an
   external owner; old completion is rejected and a fresh ticket remains pending.

Existing `crates/flow-engine/tests/resources.rs` provides the broader identity,
capacity, dispatch, cross-engine/tracker, disposal and runtime-only regressions.

## Reproduce local checks

Use the repository's configured toolchain and native libraries. In the prepared
Linux validation environment:

```sh
source /workspace/shared/gpui-flow-native-env.sh
cargo test --offline -p gpui-flow-engine --test resources --test resource_lifecycle_acceptance
cargo test --offline -p gpui-flow --lib resources::lifecycle_tests
cargo clippy --offline -p flow-studio --bin resource_lifecycle_trace -- -D warnings
cargo clippy --offline -p gpui-flow --lib --tests -- -D warnings
cargo build --offline -p flow-studio --bin resource_lifecycle_trace
```

Serialize shared-target Cargo commands; use one build job. No CI/Actions,
deployment, or external-service interaction is required.

## Physical native QA script

Launch the frozen binary with stderr captured as JSON lines. Use real mouse
clicks, wait for the scene and trace to repaint between numbered actions, and
capture the window and its accompanying trace. The top bar must remain
`violations=0`. The full ticket, handle generation, attachment epoch, backend
jobs, current resource counts and recent event trace are visible.

Suggested command after the local binary has been built and copied for review:

```sh
resource_lifecycle_trace 2> resource-lifecycle-native.jsonl
```

Execute this sequence once from a fresh process. Do not complete extra pending
jobs between steps. Job numbers in this sequence are deterministic.

| Step | Physical action | Expected observations |
| --- | --- | --- |
| 1 | Open and wait | Job 1 A pending. `p=1 l=1 f=1 c=1`; one Ready and one ViewReady after actual paint, no Complete. Label visible, no checker image |
| 2 | Click `A/B/A` | Job 2 A pending. Hero source still A, same handle/attachment, higher resource generation; old pending ticket cancelled as Superseded and dispatched=true. No intermediate B render is necessary |
| 3 | Click `Complete oldest` | Backend job 1 becomes READY. Current job 2/ticket remains pending; no hero Loaded and no Complete, no checker. This is an actual stale backend completion, not a synthetic resource event |
| 4 | Click `Complete newest` | Job 2 READY, green checker visible; hero Loaded once and Complete `p=0 l=2 f=1 c=1`. Ready/ViewReady do not repeat within this document |
| 5 | Click `Redraw` twice | No new Resource or ResourceView milestones, counts unchanged |
| 6 | Click `Source B`, then `Detach / remount` | Jobs 3 and 4 B pending. Hero handle preserved, attachment epoch and ticket generation change; old cancellation dispatched=false |
| 7 | Click `Complete oldest`, then inspect | Job 3 READY, job 4/ticket still pending, no new hero Loaded/Complete |
| 8 | Click `Complete newest` | Job 4 READY, purple checker visible; new hero Loaded and Complete once |
| 9 | Click `Source A`, then `Destroy + same ID` | Jobs 5 and 6 A pending. ID still hero but ElementHandle changes, old cancellation dispatched=false, new ticket pending |
| 10 | Click `Complete oldest`, then inspect | Job 5 READY cannot land on recreated hero; job 6/ticket still pending, no hero Loaded/Complete |
| 11 | Click `Complete newest` | Job 6 READY, green checker; current ticket Loaded and Complete once |
| 12 | Click `Source B`, then `Cancel / deny` | Job 7 remains pending in external cache; current hero denied and Cancelled(NotAuthorized). Complete `p=0 l=1 f=1 c=2`; checker absent |
| 13 | Click `Complete oldest` | Job 7 finishes late, but hero stays cancelled; no Loaded, no repeated Complete, no checker resurrection |
| 14 | Click `Source A`, then `Fail oldest` | Job 8 becomes failed; Complete `p=0 l=1 f=2 c=1`, no checker |
| 15 | Click `Drop view`, then `Mount view` | Old view disappears; cache job history remains. New engine/tracker owner, job 9 A pending, fresh Ready/ViewReady after paint, `p=1 l=1 f=1 c=1` |
| 16 | Click `Complete newest`, then `Redraw` twice | Job 9 READY, green checker; exactly one new Complete `p=0 l=2 f=1 c=1`, violations remain zero |

A retained shared cache is intentional in steps 12–16. The absence of externally
owned cache eviction must not be presented as a Flow memory leak. Conversely,
actual Flow-owned prepared-image leases are covered separately by weak-Arc
regressions. Native rendering may retain its own transient Arc until a frame is
released; this fixture does not infer ownership solely from process RSS.

## Recorded results

Local Linux checks passed on 2026-10-02:

- Engine: 16 existing resource tests and 2 new receipt acceptance tests
- UI adapter: all 37 `resources::` tests, including the 5 new lease/settlement tests
- Strict Clippy (`-D warnings`): native fixture, UI library and tests, engine resource targets
- Native fixture linked successfully; dynamic-library inspection found no missing libraries
- `git diff --check` passed

Frozen native binary SHA-256:
`e7b7891f45b88120d2c0e2aba3ea23e1b2b64d4adf672442592ae812fe3a5ea9`.
The build used the integrated worktree above `911e3f8741130b6eb8b9298fedc985839c5cd03b`,
including the then-uncommitted focus-input work. Source hashes, a frozen copy of
relevant source files and the local command logs were preserved with the binary.
This is focused resource evidence, not a claim that all workspace targets ran in
this validation batch. No Actions or deployment was run.

### Physical native result: bounded acceptance passed

The integrator performed the complete physical sequence on Linux from
**2026-10-02 16:24:36 to 16:29:31 UTC**, using the frozen binary above. The window
was closed normally and the captured process exit was **0**. All ten supplied
screenshots were visually reviewed, including actual green/purple checker
pixels, stale-source/attachment/instance placeholders, denial after late
completion, the producer failure, the dropped view and the remounted completion.

The [checked-in evidence summary](validation/2026-10-02-resource-lifecycle-summary.json)
contains the run receipt, all **62 exact JSON records**, screenshot hashes,
indexed assertions, parsed milestone counts and source hashes. Its raw JSONL
SHA-256 is
`23a92d0fa4f8e48ef24ec1253fcac0956c54eb355e0fd3723108e8f6c779affc`.
The summary is below 100 KiB; the binary and PNG files remain local validation
artifacts. The integrator verified source checkpoint
`f5ab200c6f6bfaf786742b9b0ace8a530de0d6cf`, tree
`b2a5441be7e7a13e6de9ba08bf609538c6b94052`, and zero Actions runs. The six recorded
lifecycle source/test files in the local index exactly match the frozen inputs.

Audit findings:

- All 62 records have `violations=0`, with contiguous sequence numbers
- Sixteen observed resource tickets each have exactly one terminal event
- Ready and ViewReady each occur once for engine 1 and once for engine 2;
  the six Complete events are unique per engine/document/resource epoch
- All ViewReady/Complete events have a nonzero host paint-traversal witness;
  all milestone counts independently match the current resource states
- A/B/A changes hero generation 3 to 5 while preserving handle and attachment;
  old backend job 1 produces no Loaded/Complete; only job 2 loads generation 5
- Remount preserves the handle but changes attachment 0 to 5 and generation
  6 to 7; the old cancellation is `dispatched=false`; only job 4 loads it
- Destroy/recreate preserves the ID but changes handle generation 1 to 2,
  attachment 5 to 8 and resource generation 8 to 9; old job 5 cannot populate
  the recreated image, and its cancellation is `dispatched=false`
- Cancellation completes with `p=0 l=1 f=1 c=2`; old job 7 finishing later
  neither loads the hero nor repeats Complete. Failure completes with
  `p=0 l=1 f=2 c=1`
- Drop/remount preserves the external cache history and changes the engine
  owner from 1 to 2; fresh Ready/ViewReady precede job 9's Loaded and Complete
- Repeated redraws were physically performed and no duplicate stages appear.
  The Redraw button emits no action record, so its exact click count is operator
  evidence rather than independently recoverable from the JSON

This closes **LIF-02's bounded readiness/native-paint acceptance gap** for the
validated controlled in-memory ImageCache path together with the tracker and
adapter regressions. It does not upgrade LIF-03. A decoded cache result is used
here, so file/network decoding itself was not exercised. The session drops a
settled failed view; cancellation on disposal of a still-pending view remains
unit-tested rather than part of this native session. GPU presentation fences,
hard cancellation, byte budgets and native heap ownership measurements remain
outside the claim, as described above.
