# gpui-flow

面向 Rust / GPUI 的分层富画布框架：底层是可组合的显示对象与保留式场景树，上层提供通用图形编辑器、节点工作流组件和可替换的 agent 执行接口。

私有早期开发版本，API 尚未承诺稳定。以 Leafer 的显示对象/容器分层和 React Flow 的节点扩展思路为参考，独立实现；不宣称完整 Leafer 功能或 API 兼容。

单个叶节点外阴影现已进入同一Scene/engine/CPU路径：矢量fill/stroke、图片透明像素和真实glyph alpha作为源，复用锁定image Gaussian；支持blur/offset/color、整体opacity/blend/clip与Undo。visual effect bounds不改变布局/命中/选择。Group/Box/Frame/Custom、inner/spread/box/multiple/scale-fixed阴影仍明确不支持；见[契约和预算](docs/compositing.md#bounded-leaf-outer-shadows)，示例`outer_shadows`。本轮786项headless（8项原有忽略）、86项UI、严格Clippy/格式与原生链接通过；真实窗口验过blur/offset/alpha/clip/Undo、拒绝恢复、选择与pan/zoom，详见[实际验证](docs/compositing.md#outer-shadow-validation-2026-10-01)。

## 当前进展（2026-10-01）

首个[有界矢量合成](docs/compositing.md)检查点已接入真实SceneView：整体组/多paint透明度、域内Normal/Multiply/Screen及alpha sibling mask，复用tiny-skia和现有stroke mesh。583项headless测试（含doctest）、47项UI库单测、全workspace/all-targets严格clippy及专用原生示例链接通过。已在Linux云端真实窗口验过透明度/混合/遮罩切换、Undo、pan/zoom及关闭重开；CI保持暂停。

静态Image现已通过现有GPUI资源加载/cache接入合成：支持真实像素alpha、Contain/Cover/Fill、组透明度、域内混合与遮罩。新增`composited_images`原生示例；验证范围与资源/内存边界见[compositing.md](docs/compositing.md)。

固定尺寸Box/Frame已接入真实CPU合成：自身fill/stroke后绘制children、整体透明度、圆角子级clip及域内混合。保持现有native顺序；尚未实现Leafer特有fill→children→stroke阶段。新增`composited_containers`示例，见[语义边界](docs/compositing.md#fixed-size-painted-containers)。

动画Image已复用GPUI解码帧接入native/CPU合成，支持实例暂停、seek和隐藏冻结。Text新增宿主显式接入的同原生字体后端桥，支持solid纯文字、换行/wrap/对齐及正向均匀缩放，真实glyph alpha可参与mask/clip/组透明度与域内混合；不是任意现有App自动开启。缺少文字桥、不支持的文字样式/变换、Custom和任意宿主GPUI背景混合仍整域明确报错，擦除、完整shadow/filter仍缺。闭合矢量Inside/Outside描边已新增共享有界Boolean mesh；scale-fixed描边、完整嵌套timeline和motion path仍缺。此次窗口烟测不代替跨平台、大规模性能或所有旧示例交互验收。详细约束与实际操作见[compositing.md](docs/compositing.md)。

最新文字检查点通过714项headless（另有8项原有忽略）、76项UI库单测、单独的8项GPUI字体单测、严格Clippy/格式和原生构建。真实字体探针覆盖CJK/组合音标/RTL/emoji/wrap，并在DPR 1/1.25/2对同后端灰度glyph逐像素验证；原生窗口验证了文字mask/clip/混合、整体透明度、桥撤销恢复、字体失效、对齐与Undo。详细范围与未验场景见[文字验证](docs/compositing.md#same-backend-text-validation-2026-10-01)。

此前资源/布局/动画/状态源码批次的媒体、字体、14个二进制及文档记录见[历史本地验证](docs/local-validation-2026-09-30.md)。API迁移见[源码批次状态](docs/source-only-status.md)：旧JSON读取默认兼容不等于Rust穷尽字面量/枚举match源码兼容。

显式`SceneCompositing::OwnedCanvas`现将SceneView自己的不透明主题底色、网格和完整presented Scene纳入同一CPU域，支持顶层对象读取这些真实背景及更早root进行Multiply/Screen；默认逐域模式保持不变。缺producer/资源/预算错误整画布拒绝，不支持任意宿主GPUI背景捕获。新增`owned_canvas`示例，见[范围和预算](docs/compositing.md#explicit-owned-canvas-backdrop)。本轮731项headless、81项UI、严格Clippy/格式与原生链接通过；真实窗口验证底色/grid/混合、整域拒绝恢复、默认模式边界、拖动及pan/zoom。硬失效重算会暂时隐藏旧画布，不宣称流畅帧率；见[实际验收](docs/compositing.md#owned-canvas-validation-2026-10-01)。

新增闭合轮廓`StrokeAlignment::Inside/Outside`，旧JSON保持Center；native/CPU、命中、bounds与stroke-box布局共用最终mesh，开放轮廓和超预算/精度输入原子拒绝。契约及原生比较示例见[描边对齐](docs/stroke-alignment.md)。本轮云端本地764项headless（8项原有忽略）、85项UI、严格Clippy/格式与原生链接通过；真实窗口验证两种后端的对齐/填充规则/dash/变换/clip/Undo/pan/zoom。未触发GitHub Actions。

## 架构层次

1. **display / scene**：Rect、Ellipse、Line、Polygon、Star、Path、Pen、Text、Image、Group、Box、Frame 独立文件；统一 identity、style、affine transform、层级、命中、事件、JSON
2. **engine / layout / editor**：唯一 Scene 所有权、带世代实例、生命周期、属性 schema、dirty 缓存，以及可选确定性行列布局和保留编辑历史的属性动画；独立 SceneEditor，原子编辑、父子坐标变换、选择、缩放/旋转/移动与手势级 undo/redo
3. **GPUI adapter**：SceneView + DisplayElement，真正的矢量三角化、任意仿射变换、嵌套圆角 Frame 裁切；渲染能力不足时明确 diagnostics
4. **components / nodes**：NodeFrame、PortHandle、MiniMap、ViewportControls、Toolbar、Palette、Inspector 可单独使用；八类节点逐文件实现
5. **flow / runtime**：类型化图、连接校验、分组、复制粘贴、文件持久化；独立 runner、进度/取消/重试、受控并发 DAG 调度与端口数据传递
6. **examples**：节点 Studio、原语 Gallery、独立组件组合示例，三者共用库组件

图片支持本地资源预览；视频/音频通过可替换 MediaBackend 使用本机 FFmpeg 解码、ffplay 音频输出；支持帧预览、暂停、定位、停止和音量，不自动安装工具。示例生成任务明确使用 MOCK，不访问生成服务、不产生 API 费用。Run workflow 按图的依赖执行冻结快照，generation/agent 仍明确使用离线 MOCK。

## 可运行示例

```bash
cargo run --locked -p flow-studio
cargo run --locked -p flow-studio --bin primitives
cargo run --locked -p flow-studio --bin stroke_alignment
cargo run --locked -p flow-studio --bin composition
cargo run --locked -p flow-studio --bin interactions
cargo run --locked -p flow-studio --bin gradients
cargo run --locked -p flow-studio --bin layout_appearance
cargo run --locked -p flow-studio --bin animations
cargo run --locked -p flow-studio --bin auto_size
cargo run --locked -p flow-studio --bin state_styles
cargo run --locked -p flow-studio --bin reactive_layout
cargo run --locked -p flow-studio --bin measured_text
cargo run --locked -p flow-studio --bin clip_chains
cargo run --locked -p flow-studio --bin text_measure_probe # 无窗口真实字体探针
python3 scripts/generate-media-fixtures.py
cargo run --locked -p flow-studio --bin media
# Linux 无 desktop portal 时，明确指定 Save/Open 路径：
cargo run --locked -p flow-studio -- --file ./canvas.flow.json
```

- **Interactions**：拖拽连线/吸附、有效和无效目标、只读及独立插件开关
- **Studio**：节点工作流与原生内容编辑、文件 Save/Open、连接/分组、mock runner
- **Primitives**：通用场景原语、嵌套变换/裁切、点击/拖动/缩放/旋转、捕获事件
- **Clip chains**：Text/Image 在嵌套矩形 Frame 内真实裁切，图片 fit 保持完整尺寸；平移/正等比缩放/空交集与命中一致
- **Measured text**：真实字体/字重/换行/行高测量，普通文字或容器宽度编辑联动自动高度，一次 Undo；支持中文/emoji 与显式字体缓存刷新
- **Auto size**：按内容测量容器、换行自动高度、嵌套收敛；输入与布局同一Undo事务
- **Animations**：真实属性插值、pause/resume/seek和实例销毁保护；完成一个Undo步骤，取消保留原历史
- **Layout + Appearance**：行列/换行/grow/尺寸约束，一次 Reflow 一次撤销；多层矢量填充和描边，真实渐变叠绘
- **Gradients**：多色标线性/径向渐变、变换和裁切
- **Media**：真实本地视频帧与音频播放控制，需本机 FFmpeg 工具
- **Composition**：独立 NodeFrame/Handle 与宿主 Toolbar/Inspector，嵌入 canvas-only 画布并注册自定义节点

## 快速开始

需要 Git、Rust **1.98.1**，以及桌面原生依赖。Linux 安装步骤和 macOS 要求见 [环境准备](docs/getting-started.md)。

```bash
git clone git@github.com:emojiiii/gpui-flow.git
cd gpui-flow
rustup toolchain install 1.98.1 --component rustfmt --component clippy
cargo +1.98.1 test --locked -p gpui-flow-core -p gpui-flow-runtime
cargo +1.98.1 run --locked -p flow-studio
```

克隆需要仓库访问权限。Studio 需要实际桌面会话和可用的 GPU/图形驱动；无显示环境仍可运行 core/runtime 测试。

## 第一次操作

1. 点击左侧组件名称添加节点，拖动节点标题栏移动，右下角缩放尺寸
2. 中键或 Space+左键拖动平移；按住 Ctrl/Cmd 滚轮缩放；`F` 适配全部内容
3. 左键拖动空白区域框选，Shift 追加选择，Shift + 点击标题栏切换多选
4. 从端口按下并拖到兼容端口，近距离自动吸附；松开提交，空白/无效处松开取消；`Esc` 回滚当前手势
5. `Run · MOCK` 运行选中的 generation/agent 节点；没有选中节点时运行全部此类节点
6. `Save file` / `Open` 使用原生文件对话框保存/读取含视口的文档；`Save snapshot` 只保存内存快照
7. `Ctrl/Cmd + S` 保存文件；`Ctrl/Cmd + Shift + S` 复制图 JSON 到系统剪贴板

完整快捷键及行为边界见 [入门教程](docs/getting-started.md)。

## 模块

| crate | 用途 | 需要 GPUI |
| --- | --- | --- |
| `gpui-flow-interaction` | 可组合输入插件、捕获生命周期、能力策略与命令验证 | 否 |
| `gpui-flow-core` | 图模型、几何、校验、编辑事务、历史、文档 | 否 |
| `gpui-flow-scene` | 保留场景树、显示原语、仿射变换、命中与事件 | 否 |
| `gpui-flow-engine` | 唯一场景所有者、实例/生命周期、属性/缓存与树事件 | 否 |
| `gpui-flow-animation` | 显式时钟、属性插值、播放控制、实例保护与编辑器安全会话 | 否 |
| `gpui-flow-text` | 独立文字测量接口与可选 gpui-fast 后端，无场景/编辑器依赖 | 默认否，`gpui` feature 可选 |
| `gpui-flow-layout` | 行列布局、content尺寸/换行/grow/约束、原子编辑及配置持久化 | 否 |
| `gpui-flow-editor` | 通用场景选择、事务、历史与图形编辑 | 否 |
| `gpui-flow-runtime` | `AgentRunner`、工作流调度、任务状态、事件和取消 | 否 |
| `gpui-flow-media` | 媒体后端接口、FFmpeg 解码与播放控制 | 否 |
| `gpui-flow` | `SceneView`、显示适配器、独立组件、`FlowCanvas` 与节点 | 是 |
| `flow-studio` | 原生桌面示例程序 | 是 |

主 workspace 的默认成员是 text/core/engine/interaction/layout/animation/runtime/scene/editor/media，`cargo test` 不会自动覆盖原生 UI。

## 文档导航

- [当前源码批次与未验证范围](docs/source-only-status.md)
- [动画delay/repeat/direction/speed契约](docs/animation-timing.md)
- [命名状态、Text placeholder与button继承](docs/named-states.md)
- [原生资源结果、生命周期与就绪回执](docs/resource-lifecycle.md)

- [原生 Text/Image 矩形裁切链](docs/native-clipping.md)
- [真实文字测量与响应尺寸](docs/text-measurement.md)

- [Leafer 源码分层、差距与验收契约](docs/leafer-source-map.md)
- [元素实例、属性变更与事件引擎](docs/engine.md)
- [属性动画、播放控制与Undo历史](docs/animation.md)
- [键盘事件、Space导航与释放清理](docs/keyboard-routing.md)
- [视觉行列布局与边界](docs/flow-layout.md)
- [多层填充、描边与外观边界](docs/appearance.md)
- [引擎基础契约实施顺序](docs/engine-contract-plan.md)

- [交互插件、只读策略与自定义行为](docs/plugins.md)
- [显示对象与场景架构](docs/display-scenes.md)
- [独立组件与组合开发](docs/components.md)
- [环境准备与交互教程](docs/getting-started.md)
- [架构、图模型与持久化](docs/architecture.md)
- [自定义节点、主题与连线](docs/custom-nodes.md)
- [实现 AgentRunner 与接入宿主](docs/runtime.md)
- [工作流依赖调度、类型校验与重试](docs/workflows.md)
- [音视频解码与播放器集成](docs/media.md)
- [渐变渲染与资源边界](docs/gradients.md)
- [测试、CI 与人工验收](docs/testing.md)
- [支持边界、安全与待实现项](docs/limitations.md)
- [可执行 core 示例](docs/examples/src/bin/core.rs) · [可执行 runner 示例](docs/examples/src/bin/runner.rs)

## 依赖与实现来源

底层使用 [longbridge/gpui-fast](https://github.com/longbridge/gpui-fast) 的 `main` 分支，实际版本由提交到仓库的 `Cargo.lock` 固定为 `10d005166a9f9e079d55ebfb700035f3860c3e71`。GPUI Kit 固定为 `0067bafcf3160cb871eff2521e98a51e526da175`。本批新增直接复用petgraph 0.8.3、Kurbo 0.11.3、Taffy 0.13.0和Lyon tessellation 1.0.22；headless crate仍不依赖GPUI，数值库不替代本项目文档/身份/事务契约。构建请使用 `--locked`，不要无意更新 lockfile；升级时应同时验证二者兼容性。

设计参考 [React Flow 的自定义节点思路](https://reactflow.dev/learn/customization/custom-nodes) 和 [Leafer UI 的画布编辑方式](https://github.com/leaferjs/leafer-ui)，本项目是独立 Rust 实现，不是这些项目的移植，也未声明 API 兼容。当前不为本项目源代码授予开源许可证；各第三方依赖保留其各自许可，分发前需独立审查。

关键帧：`AnimationController::animate_sequence`支持有界显式时长/easing序列、稀疏字段保持和跨段seek，继续使用同一编辑器播放事务，见[动画契约](docs/animation.md)

状态样式：`SceneView.state_styles` 提供selected/focus/hover/press/disabled的实色与opacity呈现层及进出平滑过渡/连续反转，不改文档或Undo；优先级、生命周期及剩余边界见[状态样式契约](docs/state-styles.md)

自动响应布局：先排好初始场景，再用`FlowLayout::install_reactive`绑定引擎；普通属性/树编辑自动在同一提交中派生尺寸与位置，保留一次Undo。安装/生命周期/动画所有权边界见[reactive-layout.md](docs/reactive-layout.md)

程序焦点：`SceneView::focus_element/clear_element_focus`与router/deferred listener入口保留selection独立，使用实例身份、Blur→Focus和有界重定向；见[焦点契约](docs/focus-routing.md)

反向布局：`RowReverse/ColumnReverse`与`wrap_reverse`按Leafer遍历/行序契约工作，保持场景树与绘制顺序，支持内容尺寸与响应布局。详见[布局契约](docs/flow-layout.md)
