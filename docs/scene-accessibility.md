# Opt-in scene accessibility and semantic navigation

EVT04 remains **partial**. This slice implements real AccessKit scene nodes and
bounded keyboard/AT activation. Native IME candidates and Windows/macOS are not
claimed verified. Build, model tests and real platform adapter checks are
separate evidence; a node-builder test is not an AT-SPI/UIA/AX acceptance test.

## Host API and native example

```rust
view.set_accessibility(Some(SceneAccessibility::new("Diagram")), cx);
// Restore the default, non-semantic canvas:
view.set_accessibility(None, cx);
```

No serialized scene fields, dependency changes or replacement input framework
are required. `accessibility_issues()` reports withheld projections and unsupported
visibility. `cargo run --locked -p flow-studio --bin scene_accessibility -- --trace`
places the canvas between two host buttons. The first toggles read-only; the last
dumps `Window::debug_a11y_tree_json()`, the actual last GPUI tree when available.
That dump does not issue a platform AT action. `--trace` separately records
routed semantic requests and Text-session lifecycle.

The root has `GraphicsDocument`, the host label and **one** native Tab stop using
the existing canvas FocusHandle, with tab-stop state set on that actual handle.
It stays in the host tab scope rather than creating a nested tab group, preserving
Before → Canvas → After order among same-index siblings. In displayed paint order, proxy nodes have
Group, GraphicsObject or Label roles, a name from object name / Text content /
ID, actual selection state, and disabled/read-only state. The current logical
target reports active-descendant focus only while the canvas owns native focus.
An accent outline around its conservative logical bounds is painted in the same
presentation guard as the content. No authored styles or document history change.

Proxies have no mouse handlers, tracked FocusHandle or Tab index. They use GPUI's
ordinary real-node role/action path; synthetic children are not used. A small
Element wrapper delegates the Div lifecycle and adds AccessKit disabled/read-only
flags, which the public Div builder does not expose. It creates no extra hitbox.

## Input and defaults

- A private scene key context binds Tab/Shift-Tab/Enter actions before Kit root
  traversal can consume them. The context is absent while an inner editor owns
  input; callbacks also check exact canvas focus. Matched actions normalize the
  originating key into the scene route exactly once, with raw KeyDown as fallback
- Tab/Shift-Tab first run the existing scene KeyDown tree/default/queued-action
  path. If permitted, they advance through enabled displayed targets. A boundary
  calls the host window's focus-next/previous; the scene does not wrap locally
- Enter on editable Text opens the existing content editor, opting into that
  tool when needed. Enter on another target requests focus only. AT Focus does
  the same focus operation; editable Text additionally exposes AT Click as
  OpenText. Neither action changes selection
- Ctrl/Alt/Cmd combinations, Shift-Enter, Space-pan, crop editing and inner-tool /
  Textarea ownership are left to their existing owners. The scene does not
  intercept IME character delivery or add a text input implementation
- Locks and disabled Navigation capability disallow semantic focus. Read-only
  permits focus but disallows OpenText. Inherited visibility/opacity/locks,
  invertible editing ancestry and Edit capability are checked live
- Done/Cancel restores the canvas and its original semantic Text target when
  still valid. If an edited document has not published yet, entry waits for its
  displayed frame. Leaving the panel for another host control retains that focus

Both AT actions and keyboard defaults dispatch `EventKind::SemanticAction` with
`EventInput::Semantic { action, source }`; SceneView emits `SceneViewEvent::Semantic`.
Native canvas focus entry/return uses the honest NativeFocus source because GPUI
does not expose the originating device for a generic focus notification.
Capture/target/bubble listeners may prevent default. There are **no** invented
pointer coordinates, pressure, pointer identity or pointer timestamps, including
after prior pointer input. Listener queues still drain on cancellation.

The semantic request and its subsequent Focus notifications share the existing
256-action budget through a one-use continuation. A keyboard KeyDown is its own
existing bounded operation before the semantic request. After listener commands,
and again after Focus listeners, the view rechecks identity, policy, presentation
and geometry before opening Text. Delete/recreate, remount, document load, lock,
read-only and stale queued requests cannot retarget a replacement.

## Displayed frame and bounded support

Opting in forces the existing coherent presentation path even without state
styles or scale-fixed strokes. Projection starts from its **displayed** StateFrame;
it never combines a pending CPU image with a newer canonical semantic tree.
Only native units whose RenderObject is natively renderable and members named by
accepted DomainSource IDs qualify. Rejected units and unaccepted uploads do not.
The same PresentationElement encloses native/CPU content, semantic proxies and
the focus outline. Viewport, clipping bounds, DPR, window, resource provenance,
canonical revision and displayed key are checked again at prepaint/paint and
when an action arrives. A retained old state-style sample keeps its old semantics
until atomic publication of the replacement.

Hidden, effective-zero-opacity, offscreen and broad-phase fully clipped targets
are absent. Supported ordinary clips are axis-aligned rectangular clips on
axis-aligned objects. A rotated object's partially viewport-clipped geometry,
rounded/rotated clips, Custom content, or a domain containing any mask/eraser
has explicitly unsupported semantic visibility: the affected targets are omitted
and diagnosed. This intentionally conservative support does not add glyph/pixel
picking. Bounds describe logical geometry, never precise visible alpha; they do
not account for holes, transparent image pixels or ordinary sibling occlusion.
They must not be used as a pixel-hit or pixel-visibility promise.

Nodes/bindings are capped at 2,048, total label bytes at 64 KiB; admitted domain
membership is also bounded before projection. Overflow withholds the **whole**
projection and reports BudgetExceeded. Labels are not silently truncated and
partial trees are not published. The root remains a labelled host Tab stop.

A binding identifies ElementHandle + attachment epoch + document epoch. Its
monotonic node ID survives repaint and an offscreen/onscreen roundtrip while
that live binding fits the budget. Reconciliation removes obsolete identities;
disable/re-enable revokes old callbacks using a fresh allocation-identity token.
ID exhaustion fails closed. Callback closures retain only WeakEntity handles.
Managed LayeredApp scenes are explicitly unsupported by this opt-in adapter.

GPUI constructs accessibility nodes during prepaint. A later paint/upload failure
can briefly leave an already built node in the current platform tree. Once the
presentation is invalidated, its live action gate rejects requests immediately;
the next tree update removes it. The public API does not provide an atomic
rollback of a previously submitted accessibility tree. Undetected graphics-driver
faults are not a semantic visibility guarantee.

## Regression and platform boundaries

Focused model/controller tests cover real upload admission/publication/failure,
retained state-style bounds and active-target candidates, view invalidation,
identity replacement/remount/reset and offscreen reuse, budgets, rejected/native
unsupported content, conservative clipping, locked/read-only policy, modifier /
inner-editor yielding, bilateral Tab boundaries, cancelable semantic metadata,
queued load/lock, and the shared action budget. Metadata/projection fixtures are
not substitutes for native adapter testing.

Native acceptance should separately inspect the real platform tree, request AT
Focus and Text activation, traverse both external controls in both directions,
observe the focus outline, close/reopen Text with Done/Cancel, and release the
window. Real IME candidate interruption and Windows/macOS remain unverified;
EVT04 is not closed by this module alone.

Local validation (2026-10-04): 27 routing and 333 UI regressions passed, plus
focused checks after the native fixes, strict checks, formatting and example
linking. Linux keyboard checks confirmed Before → canvas targets → After and
the reverse order, locked-node skipping, Text activation and Done/Cancel return.
Native checks caught the explicitly tracked FocusHandle's Tab-stop configuration
and a post-focus redraw issue; both were corrected. The final check showed the
focus outline after external entry and Cancel without moving the pointer.
The available platform reported `a11y active=false`, and the UI tool returned an
X11 window shell rather than an accessibility tree. Real AT tree/Focus/Click
requests were therefore **not verified**; neither were native IME candidates.
