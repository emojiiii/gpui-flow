# gpui-flow

面向 Rust / GPUI 的分层富画布框架：底层是可组合的显示对象与保留式场景树，上层提供通用图形编辑器、节点工作流组件和可替换的 agent 执行接口。

私有早期开发版本，API 尚未承诺稳定。以 Leafer 的显示对象/容器分层和 React Flow 的节点扩展思路为参考，独立实现；不宣称完整 Leafer 功能或 API 兼容。

## 架构层次

1. **display / scene**：Rect、Ellipse、Line、Polygon、Star、Path、Pen、Text、Image、Group、Box、Frame 独立文件；统一 identity、style、affine transform、层级、命中、事件、JSON
2. **editor**：独立 SceneEditor，原子编辑、父子坐标变换、选择、缩放/旋转/移动与手势级 undo/redo
3. **GPUI adapter**：SceneView + DisplayElement，真正的矢量三角化、任意仿射变换、嵌套圆角 Frame 裁切；渲染能力不足时明确 diagnostics
4. **components / nodes**：NodeFrame、PortHandle、MiniMap、ViewportControls、Toolbar、Palette、Inspector 可单独使用；八类节点逐文件实现
5. **flow / runtime**：类型化图、连接校验、分组、复制粘贴、文件持久化；独立 runner、进度/取消/重试、受控并发 DAG 调度与端口数据传递
6. **examples**：节点 Studio、原语 Gallery、独立组件组合示例，三者共用库组件

图片支持本地资源预览；视频/音频通过可替换 MediaBackend 使用本机 FFmpeg 解码、ffplay 音频输出；支持帧预览、暂停、定位、停止和音量，不自动安装工具。示例生成任务明确使用 MOCK，不访问生成服务、不产生 API 费用。Run workflow 按图的依赖执行冻结快照，generation/agent 仍明确使用离线 MOCK。

## 可运行示例

```bash
cargo run --locked -p flow-studio
cargo run --locked -p flow-studio --bin primitives
cargo run --locked -p flow-studio --bin composition
cargo run --locked -p flow-studio --bin interactions
cargo run --locked -p flow-studio --bin gradients
python3 scripts/generate-media-fixtures.py
cargo run --locked -p flow-studio --bin media
# Linux 无 desktop portal 时，明确指定 Save/Open 路径：
cargo run --locked -p flow-studio -- --file ./canvas.flow.json
```

- **Interactions**：拖拽连线/吸附、有效和无效目标、只读及独立插件开关
- **Studio**：节点工作流与原生内容编辑、文件 Save/Open、连接/分组、mock runner
- **Primitives**：通用场景原语、嵌套变换/裁切、点击/拖动/缩放/旋转、捕获事件
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
| `gpui-flow-editor` | 通用场景选择、事务、历史与图形编辑 | 否 |
| `gpui-flow-runtime` | `AgentRunner`、工作流调度、任务状态、事件和取消 | 否 |
| `gpui-flow-media` | 媒体后端接口、FFmpeg 解码与播放控制 | 否 |
| `gpui-flow` | `SceneView`、显示适配器、独立组件、`FlowCanvas` 与节点 | 是 |
| `flow-studio` | 原生桌面示例程序 | 是 |

主 workspace 的默认成员是 core/interaction/runtime/scene/editor/media，`cargo test` 不会自动覆盖原生 UI。

## 文档导航

- [Leafer 源码分层、差距与验收契约](docs/leafer-source-map.md)
- [元素实例、属性变更与事件引擎](docs/engine.md)
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

底层使用 [longbridge/gpui-fast](https://github.com/longbridge/gpui-fast) 的 `main` 分支，实际版本由提交到仓库的 `Cargo.lock` 固定为 `10d005166a9f9e079d55ebfb700035f3860c3e71`。GPUI Kit 固定为 `0067bafcf3160cb871eff2521e98a51e526da175`。构建请使用 `--locked`，不要无意更新 lockfile；升级时应同时验证二者兼容性。

设计参考 [React Flow 的自定义节点思路](https://reactflow.dev/learn/customization/custom-nodes) 和 [Leafer UI 的画布编辑方式](https://github.com/leaferjs/leafer-ui)，本项目是独立 Rust 实现，不是这些项目的移植，也未声明 API 兼容。当前不为本项目源代码授予开源许可证；各第三方依赖保留其各自许可，分发前需独立审查。
