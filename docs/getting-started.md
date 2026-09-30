# 环境准备与入门教程

## 1. 工具链与依赖

本仓库的 CI 使用 Rust 1.98.1，与锁定的 gpui-fast 工具链对齐。这是当前验证目标，不是承诺的最低 Rust 版本。

```bash
rustup toolchain install 1.98.1 --component rustfmt --component clippy
rustup override set 1.98.1
```

`Cargo.lock` 锁定 Git 依赖提交；`main` 并不表示每次构建都取上游最新代码。所有验证命令使用 `--locked`，格式检查除外。

### Ubuntu 24.04

以下依赖基于锁定版本的 GPUI Linux 构建依赖及上游安装脚本，覆盖字体、Wayland/X11、音频、加密、压缩和绑定生成：

```bash
sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config clang libclang-dev \
  libasound2-dev libfontconfig-dev libfreetype-dev libssl-dev \
  libvulkan1 mesa-vulkan-drivers libwayland-dev \
  libx11-xcb-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libzstd-dev
```

运行原生窗口还需要桌面会话、字体与可用图形后端。CI 的 `cargo build` 不会打开窗口，也不能代替视觉验证。其他 Linux 发行版请按发行版包名安装对应开发库。

### Windows

使用 MSVC Rust 工具链、Git 和 Visual Studio 2022 Build Tools（“使用 C++ 的桌面开发”工作负载与 Windows SDK）。CI 使用 `windows-latest` 自带的 MSVC/SDK 环境，执行 workspace check/clippy、UI 单测和全部示例链接；查看对应提交的 Actions 结果，不将配置存在当作通过证据。Windows 上尚未进行真实窗口、输入法与文件对话框人工验收。

### macOS

安装 Xcode Command Line Tools，并保证系统 SDK 和 Metal 工具可用：

```bash
xcode-select --install
xcrun --find clang
```

CI 的 macOS runner 提供 Xcode。某些 SDK/系统版本需要完整 Xcode 才能提供构建所需工具；遇到 Metal/SDK 错误时，先检查 `xcode-select -p` 和所选 Xcode，而不是修改 Cargo 依赖绕过错误。

### 无桌面服务器

可以验证 core/runtime 和运行文档示例，不需要 GPUI 原生库：

```bash
cargo test --locked -p gpui-flow-core -p gpui-flow-runtime
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin core
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin runner
```

首次解析 workspace 依赖仍可能需要网络下载 Git/registry 元数据。离线运行的意思是示例不会请求生成服务，不代表初次编译无需下载 Rust 依赖。

## 2. 启动 Studio

```bash
cargo run --locked -p flow-studio
```

初始图包含文本、图片、生成、输出、视频、音频和便签。左侧是节点组件面板，中间是画布，右侧展示选中节点的几何/端口属性，并提供内容输入和运行操作。

### 导航和选择

| 操作 | 行为 |
| --- | --- |
| 左键拖动空白背景 / 中键拖动 | 平移视口 |
| 滚轮 / 触控板滚动 | 平移视口 |
| Ctrl/Cmd + 滚轮 | 以鼠标位置为锚点缩放 |
| `+` / `−` 控件 | 以画布中心为锚点缩放 |
| 点击缩放百分比 | 回到 100% |
| `F` / Fit | 适配全部图内容 |
| 点击节点标题栏 | 选中并可拖动节点 |
| Shift + 点击标题栏 | 切换多选 |
| Shift + 拖动空白背景 | 按相交范围框选节点 |
| Ctrl/Cmd + A | 全选节点 |
| `G` | 切换点网格 |

节点标题栏是默认选择/移动的命中区域。内置节点主体不是文本编辑器；选中节点后可在 Inspector 的 EDIT CONTENT 编辑内容。当前框选完成后会替换节点选择，而不是累加旧选择。

### 编辑和连线

- 拖动标题栏移动选中节点；拖动右下角把手调整节点尺寸
- Studio 的缩放把手最小尺寸为 160 × 100 世界单位
- 连线使用“点击第一个端口，再点击第二个端口”，不是拖放端口
- 连线必须为输出到输入，类型相容，且未超出端口容量；不合法连接显示错误
- `Esc` 取消未完成连线或编辑手势；已开始的移动/缩放事务会还原
- `Delete` / `Backspace` 删除当前选择；删除节点同时删除相连边
- `Group selection` 将选中节点合并为单层视觉分组；分组核心 API 比当前 UI 操作更完整
- Ctrl/Cmd + C/V 复制、粘贴当前选择，粘贴时分配新 ID，并保留片段内部连线
- Ctrl/Cmd + Z 撤销；Ctrl/Cmd + Shift + Z 或 Ctrl/Cmd + Y 重做

一次拖动/缩放对应一个撤销步骤。复制粘贴使用当前画布实例中的片段缓存，不从系统剪贴板读取 JSON，也不能直接与其他应用互通。

### 离线任务

`Run · MOCK` 对选中的 `generation` / `agent` 节点启动独立任务；如果没有选中任何节点，则运行图中全部这两类节点。选择一个普通节点再运行不会隐式运行其他节点。进度和状态展示在节点里。

`Cancel runs` 发出取消请求。取消是合作式的，要等 runner 检查令牌并返回后，界面才会进入取消终态。`Retry selected` 重跑先前任务的同一份输入，不会读取编辑后的节点数据；要使用新输入，应重新调用 `start`。运行中重复启动/重试会被拒绝。

`Run workflow` 是独立入口：按依赖执行完整图快照，以三个并发槽传递端口值；失败会跳过后继。Inspector 显示输出，结果不自动覆盖 Graph data。存在 workflow 时 Retry selected 使用局部重试语义，详见[工作流教程](workflows.md)。

### 编辑节点内容与图片预览

选中节点后，右侧 EDIT CONTENT 输入根据类型编辑一个字段：文本/便签/输出等编辑 `text`，生成/agent 编辑 `prompt`，图片/视频/音频编辑 `source`。变更通过事务写入节点数据。图片 source 可填本地图片文件路径；Studio 中视频/音频 source 可填本地文件路径，自动探测后显示播放器；播放需手动点击，依赖本机 FFmpeg/ffplay。宿主自己的 FlowCanvas 默认不启用媒体，需 set_media_backend。标题、几何和端口列表仍为只读展示。

## 3. 保存、恢复与导入

当前 Studio 区分文件文档、内存快照和剪贴板导出：

- `Save file`：弹出原生保存对话框，默认名称 `canvas.flow.json`，保存 Graph 与 viewport
- `Open`：选择一个 FlowDocument JSON 文件，校验后加载图和视口，清空选择、撤销历史和运行记录
- `Save snapshot`：保存当前 Graph 的 JSON 字符串到内存，不保存 viewport
- `Load snapshot`：导入内存快照，是一次可撤销图编辑；关闭窗口后快照消失
- Ctrl/Cmd + S：把 Graph JSON 复制到系统剪贴板，**不会打开保存文件对话框**
- 这些格式均不保存编辑历史、选择、运行记录、凭证或媒体文件本体

文件读写通过 `core::save_document(path, &document)` / `load_document(path)` 实现，限制 32 MiB。写入先校验文档，写同目录临时文件并 sync，最后 rename 到目标；父目录必须已存在。没有自动保存、未保存变更提醒、备份或多实例冲突保护。保存捕获点击时的文档快照，选择文件期间的后续编辑不会自动补入此次保存。

文件对话框取消后不加载/保存。打开成功会替换当前图，不能用原来的 undo 历史恢复，先保存要保留的修改。Linux 的原生对话框依赖实际桌面/portal 环境，单纯 native 编译通过不能证明文件选择器可用。

### 没有桌面 portal 时的明确文件路径模式

```bash
cargo run --locked -p flow-studio -- --file ./canvas.flow.json
```

此模式下 Save file/Open 使用指定路径，不弹对话框；只在用户点击按钮时读写。文件存在时 Save 会原子替换它，Open 会替换当前文档。父目录需存在。没有提供 `--file` 时仍使用系统选择器。

宿主也可设置 `canvas.document_path = Some(path)`，或直接调用异步 `save_to_path(path, cx)` / `open_path(path, cx)`。`is_file_busy()` 表示当前有文件任务；同一 view 同时只允许一个文件任务，避免重复点击产生竞态覆盖。底层没有跨进程锁。

宿主可使用 `FlowCanvas::save_file(cx)` / `open_file(cx)` 触发这些异步对话框。`export_json` / `import_json` 则处理 Graph-only JSON。需要自行存储含视口 JSON 时用 `canvas.editor.to_json()` 与 `Editor::from_json()` / `load_document()`。两种 JSON 顶层结构不同，不应混用，详见[架构](architecture.md#持久化格式与边界)。

## 4. 嵌入现有 GPUI 应用

在同一个 workspace 中引用 `gpui-flow`，并与其使用相同的 GPUI 来源。完整入口可参考 [`examples/studio/src/main.rs`](../examples/studio/src/main.rs)。基本构造方式：

```rust
use gpui_flow::{FlowCanvas, demo_graph};

// 位于宿主的 cx.open_window(..., |_, cx| { ... }) 回调中：
let canvas = cx.new(|cx| {
    FlowCanvas::new(demo_graph(), cx).expect("valid graph")
});
```

启动宿主时需与 Studio 一样设置 `gpui_kit::assets::Assets`，并在构造画布前执行 `gpui_kit::init(cx)`，为内容输入等组件初始化状态。

`FlowCanvas` 是 GPUI Entity 的 view，不是一个运行服务器的全局单例。一个实例拥有自己的编辑器、选择、剪贴片段、内存快照和任务 Runtime。宿主可通过公共 `editor`、`registry`、`theme`、`edge_renderer`、`runtime`、`options` 字段组合应用能力。

`FlowEvent` 提供 `DocumentChanged`、`SelectionChanged`、`ConnectionRejected(String)` 和 `RunChanged(String)`。目前事件不是覆盖全部行为的完整审计流：例如视口变化、部分快捷键选择、宿主直接编辑 `editor` 时需要宿主自行通知、保存或发事件。

### 只嵌入画布

```rust
canvas.options = gpui_flow::FlowOptions::canvas_only();
```

它隐藏 toolbar、palette、Inspector 和 status，仍保留 minimap 与缩放控件。可分别设置 `show_minimap = false`、`show_controls = false` 隐藏剩余辅助 UI；`FlowOptions` 默认全部显示。`FlowCanvas::new` 不注册 runner，应用通过 `register_runner` 接入执行能力；`FlowCanvas::demo` 才注册两个离线 mock，见[运行教程](runtime.md)。
