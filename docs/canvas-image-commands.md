# Canvas decoded-image command batches

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

Status: implementation, 12 new regressions and real Linux-window acceptance pass. Not full 2D-context
or browser drawImage parity.

`CanvasCommand::DrawImage { image, source, destination }` and
`CanvasSurface::draw_commands_with_images(commands, &ImageInputs)` add immutable
host-decoded image inputs to the existing atomic vector/context batch. `image`
is a call-scoped resource key, not a URL; no fetching, decoding or frame-clock
behavior is added. The old `draw_commands` is compatible and supplies no images,
so a DrawImage through it reports missing input. Resource keys are at most 512
UTF-8 bytes. Repeated keys/aliases can share one immutable `Arc<StaticImage>`.
Only referenced buffers are retained and counted; completed commands retain no
input references. A previous snapshot of this same Canvas is safe as an input.

Source bounds are physical decoded-image pixels; `None` uses the full frame.
Destination bounds are logical Canvas units before the current affine transform.
Canvas DPR is applied once. Positive, finite rectangles with coordinates and
sizes bounded by 1,000,000 are required. Negative/zero sizes are rejected rather
than emulating browser rectangle normalization. Out-of-image source regions stay
transparent while preserving the original source-to-destination mapping; they
are not stretched into the whole destination. Smoothing follows retained Canvas
`smooth`; there is no separate persistent context smoothing setting.

The independent `commands/image.rs` adapter compiles to the existing texture
compositor using an internal Canvas leaf, without allocating another Canvas
surface. Optional source cropping uses a per-draw destination mask without copying
image pixels. Transform, opacity, frozen context clips, Save/Restore and source-over
ordering use the same command pipeline as vectors. Bilinear filtering still uses
the decoded texture's neighboring samples at a crop boundary; no browser-exact
sampling behavior is promised. Reflection uses the transform, not negative sizes.

All source images, context state, crop masks, dimensions, points, node/depth/mask,
working-set and pixel-work limits are checked before publishing. Missing/failed
images, a late invalid command, exhausted budget or unbalanced stack leave both
pixels and generation unchanged. Referenced image dimensions also obey the target
surface's reduced limits. A batch publishes at most one immutable generation.
Runtime pixel edits do not enter document JSON or Undo. Existing configuration
history and resource identity/destroy rules are unchanged.

Pinned reference: Leafer's [ICanvasContext2D/CanvasDrawImage](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts)
defines full-image, destination-size and source/destination rectangle overloads.
Flow offers one typed bounded variant with explicit immutable inputs. Text draw
commands, persistent context/current paths, image patterns, every composition mode
and full browser context compatibility remain outside this slice.


## Validation

**81 affected checks passed, zero failed/ignored**: 63 composite Canvas tests
(including 11 new image-command tests) and 18 UI Canvas tests (including one new
resource/identity test). Strict composite/UI all-targets Clippy, example Clippy,
format checks and actual native linking passed. Unchanged unrelated suites were
not rerun. Tests cover image/vector ordering and per-draw alpha, source crop at
DPR2, transparent out-of-image source mapping, reflection/frozen clips/restore,
missing/failed/late input rollback, finite rectangles and budgets, smoothing
compared pixel-for-pixel to direct image draw, shared aliases, immutable self-snapshot
input and stale/destroyed Canvas handles.

The `canvas_images` window ran 2026-10-02 14:00:18–14:03:12 UTC and closed normally
with exit 0 and empty stdout/stderr. Actual DPR1/DPR2 views showed the full frame,
a rotated cropped quadrant, transparent source margin, reflected alpha image,
curved and nested even-odd clips, a subsequent independent batch, late missing-input
rejection, safe shrink/configuration Undo, Clear and redraw. Pixels intentionally
are not restored by configuration Undo. Three artwork-region comparisons were
pixel-identical: before/after rejection, unclipped/redraw and unclipped/Clear+redraw.
Smoothing alternatives and immutable self-draw have automated pixel coverage;
the native fixture used smooth rendering. No cross-platform or browser exactness
claim is made.

Exact source checkpoint `0ab6ffbf8bdbdb837a30f6cbf193e5506cb0124d`, tree
`8fdee85586c864b0636e99f4eead209838e12801`; source archive SHA-256
`d7a0c4afd17169d7962d1f0ee2a4c6029569e954264ce6e068e273346e7e286b`;
binary SHA-256
`d4789b6d7130f7654f03991625a2e23c8e1273d305458b98e0f4ccda57b89721`.
Source was unchanged throughout native acceptance. Screenshots and the normal
close receipt are retained with validation artifacts. Run
`cargo run --locked -p flow-studio --bin canvas_images` to reproduce.
