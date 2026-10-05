# 旧 Studio 迁移与回归保留

2026-10-05 起，当前原生应用入口统一为 `gpui-flow-creative` 的 `flow-creative`。
原 `examples/studio` 包、原生演示与专属媒体生成脚本已经移除。此前源码可从
删除前的 Git 历史恢复；不在当前分支保留另一份演示副本。

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative
```

核心库能力、库内回归、字体测试资产和 `docs/examples` 独立 headless 教程保留。
旧 Studio 的文件模式参数、按钮矩阵、窗口布局与原生探针均不应当作当前工作台
使用说明。当前操作、项目保存与扩展方式见[Flow Creative](../crates/flow-creative/README.md)。

## 嵌入测试审查

删除前检查了 9 个含测试的 bin，共 21 项测试。旧按钮巡游、回执编码、固定演示
布局和镜像比较器随对应演示退役；它们没有被换名为另一套应用。
底层契约的独立回归继续位于以下位置：

| 旧 bin | 保留的核心覆盖或迁移 |
| --- | --- |
| `container_inner_shadows` | 三类域共用一次 `PixelBudget` 的场景测试已抽离到 `crates/flow-composite/tests/multi_domain_shadow_budget.rs`；不依赖 GPUI 窗口。库内 `container_inner_shadow_tests.rs` 保留 alpha、clip、绘制顺序与预算拒绝矩阵 |
| `canvas_patterns` | `crates/flow-composite/src/canvas_pattern_command_tests.rs` 保留 DPR、捕获后替换/释放原来源、变换、路径状态、原子拒绝与预算边界 |
| `canvas_stroke_text` | `crates/flow-composite/src/canvas_stroke_text_tests.rs`、`crates/flow-text/src/outline_registration/shaped_outlines_tests.rs` 保留真实字体桥与描边契约；旧回执 checksum/PNG helper 自测随探针移除 |
| `custom_geometry_trace` | `crates/flow-engine/tests/custom_geometry_acceptance.rs` 与 `crates/flow-ui/tests/custom_geometry_acceptance.rs` 保留动态轮廓、精确命中、变换与实例隔离 |
| `drag_viewport_constraints` | `crates/flow-interaction/tests/drag_viewport_constraints.rs` 保留世界/父级边界、原子视口、取消、惯性与输入时钟 |
| `focus_keyboard_trace` | `crates/flow-interaction/tests/focus_keyboard_acceptance.rs`、`keyboard_router.rs`、`picked_wheel.rs` 与 `pointer_gestures.rs` 保留焦点、取消、显式目标、失效 handle 与长按时钟；删除的是旧窗口 mirror 和 trace 序列化测试 |
| `outline_font_binding` | `crates/flow-text/src/outline_registration/tests.rs` 与 `static_outlines/tests.rs` 保留精确字体绑定、来源、错误回滚与字体轮廓；两份原始字体测试资产仍在 `flow-text/tests/fixtures` |
| `scene_export` | `crates/flow-composite/src/scene_export_tests.rs` 与 `scene_svg_tests.rs` 保留导出、内嵌资源和独立生产者边界；旧窗口 receipt/fingerprint/difference helper 随演示移除 |
| `stroke_alignment` | `crates/flow-scene` 与 `crates/flow-composite` 的 stroke 测试保留真实描边几何、像素、填充规则、变换和拒绝范围；删除的是演示按钮可逆巡游 |

原 `composite-animation.gif` 同时被 UI 播放回归引用，已移入
`crates/flow-ui/tests/fixtures/`，保留三帧、延迟、透明区与 disposal 测试输入。
演示专用 checker/SVG 不再保留；核心测试不依赖它们。

## 如何阅读历史验证

现有核心契约仍适用于对应库能力。文档中涉及旧 Studio 的名称、源码路径、
交互步骤、执行命令和结果会明确标为历史；原生验收记录只证明记录时的版本和
范围，不能作为新工作台、当前提交或其他平台已经通过的证据。

历史命令块保留为文本，只有检出当时版本才可能复现。当前命令以[入门](getting-started.md)
和[本地验证](testing.md)为准。本次移除不重新宣称旧验证数量或原生验收通过。
