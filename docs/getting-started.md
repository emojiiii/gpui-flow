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

使用 MSVC Rust 工具链、Git 和 Visual Studio 2022 Build Tools（“使用 C++ 的桌面开发”工作负载与 Windows SDK）。CI 使用 `windows-latest` 自带的 MSVC/SDK 环境，执行 workspace check/clippy、UI 单测和工作台链接；查看对应提交的 Actions 结果，不将配置存在当作通过证据。Windows 上尚未进行真实窗口、输入法与文件对话框人工验收。

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

## 2. 启动 Flow Creative

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative
```

首页用于创建和打开本地项目。新项目从空画布开始；导入图片后，媒体直接呈现在
画布中，选中对象会显示编辑工具。文字、生成节点、端口连线和本地 GLB 的当前
操作与限制见 [Flow Creative](../crates/flow-creative/README.md)。

默认生成服务未连接。要执行真实生成，宿主需要提供服务、凭证和任务处理；仅创建
生成节点或连线不会调用外部模型。

### 项目位置与恢复

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --projects-dir ./projects
```

项目保存文档、视口和导入资产。默认目录按平台选择；完整路径和保存失败、冲突、
关闭窗口的行为见[项目持久化](../crates/flow-creative/README.md#本地持久化)。
不要把项目目录与只包含图数据的 JSON 文件混用。

需要读取现有 FlowDocument 时，可显式使用独立文件模式：

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --file ./board.flow.json
```

此入口读取 FlowDocument；普通启动仍进入项目首页。原来的 Studio 与各原生
演示二进制已删除，迁移范围见[说明](studio-retirement.md)。

## 3. 在宿主中复用

- 需要创作对象与本地项目时，使用 `gpui-flow-creative` 的 `CreativeCanvas`、
  `CreativeRegistry` 和 `ProjectStore`
- 需要通用节点图时，使用 `gpui-flow` 的 `FlowCanvas` 和自定义渲染器
- 只需要模型或服务端处理时，可直接使用不依赖 GPUI 的核心 crate

宿主需使用同源的 GPUI 依赖，设置 `gpui_kit::assets::Assets` 并执行
`gpui_kit::init(cx)`。当前原生应用入口可参考
[`crates/flow-creative/src/main.rs`](../crates/flow-creative/src/main.rs)，
模型契约见[架构](architecture.md)、[节点扩展](custom-nodes.md)和
[运行时](runtime.md)。
