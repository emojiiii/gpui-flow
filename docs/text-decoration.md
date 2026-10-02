# Text underline and strikethrough

Status: recoverable source checkpoint. Focused and affected checks pass; native
side-by-side acceptance is pending. This is plain solid Text decoration, not rich
text or a replacement text layout engine.

## Contract and shared geometry

Text.decoration supports `none` (default, omitted from JSON), `underline`,
`strikethrough` and `underline_strikethrough`. Shape/factory updates use ordinary
scene validation, resource tickets, history and Undo. Intrinsic measurement and
explicit Text/padding bounds do not change because a decoration was added.

The optional flow-text `gpui_decoration` module builds bounded rectangles from
the actual shaped visual row widths, alignment and native ascent/descent. It uses
font-size/11 thickness, baseline + 0.15*font-size underline and baseline minus
0.35*font-size strikethrough, following the pinned Leafer
[TextDecoration intent](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextDecoration.ts).
The baseline itself comes from GPUI rather than Leafer's approximate baseline.
Positions and thickness snap to physical pixels through DPR, with at least one
pixel thickness. This crisp-device rule is an explicit Flow difference.

Native Text paints those rectangles under the same real inner/ancestor content
mask. The CPU glyph bridge paints the same rectangles and charges their clipped
pixel writes and plan memory to its existing budgets. Both paths use the Text
fill color. Wrapped rows, ellipsis markers, italic faces and horizontal/vertical
alignment reuse the existing shaped rows; no separate line breaker is added.
Empty rows produce no line; a whitespace-only row has a real advance width and
can be decorated.

The source cap for opt-in decorated Text is 64KiB UTF-8. Shaping/decoration reject
more than 8192 glyphs or visual rows, invalid geometry and excess memory/work.
Rectangle allocation is admitted before creation; failed rasterization returns
no partial bitmap. Font-resource preparation validates the same geometry, while
font measurement stays decoration-independent. TextRasterRequest gains a
`decoration` field; existing Rust literals should choose `TextDecoration::None`.
Repository literals were migrated; no new dependency was added.

## Opacity and capability boundary

Opaque solid decorated Text keeps the native fast path. Nonopaque object opacity
or fill alpha automatically selects a bounded CPU Text domain, so glyph/line
overlap is composed before object opacity is applied once and color glyphs use
the same alpha rules. This path requires the existing explicitly supplied
same-Application font bridge; missing capability is an explicit error. Direct
standalone DisplayElement rejects the required domain instead of silently
changing alpha semantics. Inherited nonopaque containers already create domains.

This slice does not add custom decoration colors/offsets, wavy/dashed lines,
rich runs, font feature/axis controls, underline skip-ink or outside-box paint.
Layered/gradient Text, arbitrary transforms and other existing unsupported styles
remain outside the contract. It does not rewrite upstream code or promise
browser/cross-platform pixel equality.

## Validation checkpoint

Twelve new regressions cover enum/default/budget/schema/resource rules, unchanged
intrinsics, row widths/offsets/DPR/alignment/ellipsis, real line pixels, empty and
whitespace rows, plan/work/cancellation limits, alpha domain selection, actual
once-only object opacity after glyph/line overlap, stale completion and Undo.
Affected library and targeted model tests pass; final native evidence follows.
