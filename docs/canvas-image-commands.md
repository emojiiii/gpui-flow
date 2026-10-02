# Canvas decoded-image command batches

Status: implementation and 11 new CPU regressions pass; affected UI checks,
native linking and real-window acceptance are being completed. Not full 2D-context
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
