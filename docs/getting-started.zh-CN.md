# 环境准备与入门

[文档目录](README.zh-CN.md) · [English](getting-started.md)

## 准备构建环境

在仓库根目录选择 CI 使用的 Rust 工具链。

```sh
rustup toolchain install 1.98.1 --component rustfmt --component clippy
rustup override set 1.98.1
```

构建和测试时给 Cargo 加上 `--locked`，使用锁文件中的依赖版本和 Git 提交。首次构建可能需要联网下载依赖。

### Ubuntu 24.04

安装原生构建依赖。

```sh
sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config clang libclang-dev \
  libasound2-dev libfontconfig-dev libfreetype-dev libssl-dev \
  libvulkan1 mesa-vulkan-drivers libwayland-dev \
  libx11-xcb-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libzstd-dev
```

在具备字体和可用图形后端的桌面会话中运行工作台。

### Windows

使用 Git、MSVC Rust 工具链，以及安装了**使用 C++ 的桌面开发**工作负载和 Windows SDK 的 Visual Studio 2022 Build Tools。

### macOS

安装 Xcode Command Line Tools，并检查所选 SDK 是否提供 Clang。

```sh
xcode-select --install
xcrun --find clang
```

出现 Metal 或 SDK 构建错误时，用 `xcode-select -p` 检查所选工具链。部分 SDK 配置需要完整的 Xcode。

### 可选的媒体工具

将 `ffmpeg`、`ffprobe` 和 `ffplay` 加入 `PATH`，即可使用本地音视频功能。有声播放还需要系统音频设备。详见[媒体后端](media.md)。

## 启动 Flow Creative

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative
```

启动后进入项目首页。

1. 点击**新建项目**，打开空白画布。
2. 从文件管理器将本地 PNG 或 JPEG 拖入画布，也可右键空白处选择**导入素材到此处…**。
3. 在画布获得焦点时按 **T**，点击放置文字并输入内容。选中图片或文字即可查看相应编辑工具。
4. 返回项目首页并重新打开该项目，即可看到保存的对象、导入素材和视口。

若想快速体验已有节点的画布，可直接打开内置示例。

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --demo
```

生成节点会保存提示词和参数，并提供供宿主生成服务使用的输入。独立工作台默认未配置生成服务。详见 [Flow Creative 操作指南](../crates/flow-creative/README.md)。

## 选择项目存放位置

| 平台 | 默认项目目录 |
| --- | --- |
| Linux | `$XDG_DATA_HOME/gpui-flow/projects`，或 `~/.local/share/gpui-flow/projects` |
| macOS | `~/Library/Application Support/Flow Creative/Projects` |
| Windows | `%LOCALAPPDATA%/Flow Creative/Projects` |

使用 `--projects-dir` 在指定目录打开项目首页。

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --projects-dir ./projects
```

使用 `--file` 打开包含图与视口的独立 `FlowDocument`。

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --file ./board.flow.json
```

项目会一起保存文档与导入素材。项目格式和保存行为见[本地持久化](../crates/flow-creative/README.md#本地持久化)。

## 在无桌面环境运行示例

[文档示例](examples/src/bin/)使用独立的 Cargo workspace。在仓库根目录运行以下命令。

```sh
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin core
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin runner
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin workflow
```

`core` 演示类型化连线、事务、撤销重做和文档序列化往返。`runner` 演示独立任务的进度、完成与重试。`workflow` 在两个相连的节点之间传值，输出 `HELLO CANVAS`。示例均不调用外部服务。

只运行图模型和运行时测试时，执行以下命令。

```sh
cargo test --locked -p gpui-flow-core -p gpui-flow-runtime
```

## 在宿主中嵌入画布

- 通过 `gpui-flow-creative` 的 `CreativeCanvas`、`CreativeRegistry` 和 `ProjectStore` 复用创作对象与本地项目。使用 `CreativeCanvas` 时，启用 `desktop` feature。
- 使用 `gpui-flow` 的 `FlowCanvas` 和自定义节点、渲染器搭建节点图，或使用 `SceneView` 呈现保留式显示场景。
- 在 GPUI 之外编辑图或运行任务时，可直接使用 core 和 runtime crate。

宿主需使用与 workspace 锁定版本兼容的 GPUI 依赖。资源初始化、`gpui_kit::init(cx)` 和共享文字导出后端可参考[桌面应用入口](../crates/flow-creative/src/main.rs)。更多内容见[库与宿主集成](README.zh-CN.md#库与宿主集成)和[本地验证](testing.md)。
