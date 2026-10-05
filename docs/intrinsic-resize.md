# Built-in intrinsic resize

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

`SceneResizePlugin::default().with_builtin_resize(true)` enables component-defined
geometry edits for named single-selection handles. The default plugin and shared
multi-selection handles retain their existing transform behavior. Configure Text
independently with `with_text_resize_mode(TextResizeMode::Box)` or `FontSize`.

| Component | Owned edit |
| --- | --- |
| Rect, Ellipse, Image, Canvas | Nominal width/height |
| Box, Frame | Own width/height; children keep local geometry and reactive Flow may reflow them |
| Line, Polygon | Local endpoints/vertices |
| Path, Pen | Absolute path endpoints and Bézier controls, without flattening or changing commands |
| Star | Uniform center/radius geometry, preserving count, angle and inner/outer ratio |
| Group | Direct-child transforms in Group space, because Group has no intrinsic size |
| Text | Existing explicit [Box](text-box-resize.md) and [FontSize](text-font-resize.md) policies |
| Custom | Separate registered component protocol; never guessed by this dispatcher |

Eight signed handles use frozen local pointer coordinates and keep the opposite
nominal corner/edge midpoint stationary through rotated, skewed or reflected
ancestors. Shape edits preserve the target's affine linear terms; Group retains
its complete matrix and scales its contents. Shift locks the ratio. Star always
uses a uniform factor; Group contents containing native-only Text retain the
existing uniform presentation restriction. Partial Ellipse handles use the full
nominal box. Leaf sizing boxes exclude stroke/effect extents; Group uses its existing aggregate content bounds. The plugin's
`resize_bounds` hook is shared by native overlay, picking, hover and pointer-down.

Geometry-only patches preserve current paint, corner radii, Image source/fit/crop,
Canvas DPR/smoothing/safe-resize and other configuration. Image sizing therefore
uses its normal fit policy. Canvas sizing uses the existing resource lifecycle:
safe resize keeps the physical intersection and reset resize clears it; Undo
restores configuration, not pixels. Neither this mode nor Group content scaling
adds document history for runtime resources.

Every move goes through `SetBuiltinResize`, access checks, live-instance/capture
guards and a validated transaction. Middleware cannot downgrade/retarget it or
change Group child membership. Reactive layout runs before commit. If layout owns
and replaces a requested size, position or world anchor, the complete gesture is
rejected and rolled back; ownership is never silently disabled. A free axis can
still resize when the derived target remains the requested one. Fixed Box/Frame
resize and resulting child reflow form one Undo. Text uses the same ownership
rule: measured Box overrides are rejected; FontSize succeeds only when derived
measurements agree with the requested anchored geometry.
Agreement is exact: even a fractional font/line-height multiplication that
rounds to a different `f32` box is rejected atomically. This mode does not
introduce a tolerance or override the measurer to force agreement.

All pointer moves are one gesture and one Undo; Escape, focus loss, plugin
replacement and read-only changes use the existing cancellation lifecycle.
External edits invalidate the capture without overwriting the newer document.
No pointer motion produces no history entry.

Bounds and coordinate magnitudes are capped at 1,000,000 local units. Polygon
points are capped at 65,536; paths at 21,845 commands (at most three points per
command); Groups at 4,096 direct children. Canvas retains its tighter pixel
budget. Positive-area geometry and invertible transforms are required; an empty
Group or axis-degenerate coordinate shape has no intrinsic box handles. The
existing Line endpoint tool remains the way to edit horizontal/vertical Lines.
Locked or unsupported subtrees are rejected under existing transform rules.

Run the compact native sample:

历史命令（当前分支不可运行；仅供对应历史版本复现）：

```text
cargo run --locked -p flow-studio --bin intrinsic_resize -- --trace
```

It shows all twelve non-Text built-ins, mode switching, read-only and Undo/Redo.
The sample disables the higher-priority Line endpoint plugin to expose the
diagonal Line's outer box.
Drag Rect/Image/Canvas sides, a partial Ellipse handle, Path controls through the
outer box, and the Group/Box children to compare their policies. The status bar
reports intrinsic bounds and matrix separately. Text samples remain
`text_box_resize` and `text_font_resize`.

Focused regressions: editor `intrinsic_resize` checks all eight anchors, affine
poses, each built-in policy, resource/config preservation and budgets;
interaction `intrinsic_resize` checks routed gestures, nominal overlay geometry,
single Undo, cancellation, stale edits, middleware and actual reactive Flow
content/parent/Text ownership. Existing Text and transform regressions guard the
default modes. Native validation is reported separately; compilation alone is
not a claim of visual acceptance.

Validation: 18 focused policy/ownership regressions pass. The combined final unchanged-source run passed 1880 default tests/doctests (11 ignored), 304 UI tests, strict all-target checks and formatting. A Linux window exercised Rect and nominal Ellipse sizing, Box child preservation, Group content scaling, Undo/Redo and readonly. It exited normally; other shape/cancellation cases remain covered by the focused model tests.
