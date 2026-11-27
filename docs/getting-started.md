# Getting started

[Documentation](README.md) · [简体中文](getting-started.zh-CN.md)

## Set up the build

From the repository root, select the Rust toolchain used by CI:

```sh
rustup toolchain install 1.98.1 --component rustfmt --component clippy
rustup override set 1.98.1
```

Use `--locked` with Cargo build and test commands to use the dependency versions and Git revisions in the lockfile. The first build may need network access to fetch dependencies.

### Ubuntu 24.04

Install the native build dependencies:

```sh
sudo apt-get update
sudo apt-get install -y \
  build-essential pkg-config clang libclang-dev \
  libasound2-dev libfontconfig-dev libfreetype-dev libssl-dev \
  libvulkan1 mesa-vulkan-drivers libwayland-dev \
  libx11-xcb-dev libxcb1-dev libxkbcommon-dev libxkbcommon-x11-dev \
  libzstd-dev
```

Run the workbench in a desktop session with fonts and a working graphics backend.

### Windows

Use Git, the MSVC Rust toolchain, and Visual Studio 2022 Build Tools with the **Desktop development with C++** workload and Windows SDK.

### macOS

Install the Xcode Command Line Tools and check that the selected SDK exposes Clang:

```sh
xcode-select --install
xcrun --find clang
```

For Metal or SDK build errors, check the selected toolchain with `xcode-select -p`. Some SDK configurations require the full Xcode installation.

### Optional media tools

Put `ffmpeg`, `ffprobe`, and `ffplay` on `PATH` to use local audio and video features. Playback with sound also needs a system audio device. See the [media backend guide (中文)](media.md).

## Open Flow Creative

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative
```

The workbench opens to the project home. Its current controls are labeled in Chinese.

1. Select **新建项目** (New project) to open a blank canvas.
2. Drop a local PNG or JPEG onto the canvas from your file manager. You can also right-click the canvas and choose **导入素材到此处…** (Import assets here).
3. Press **T** with the canvas focused, click to place a text object, and enter a caption. Select the image or text to see its editing controls.
4. Return to the project home and reopen the project to see your objects, imported assets, and viewport restored.

To explore a populated board with sample nodes, open the built-in demo directly:

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --demo
```

Continue with the [Flow Creative guide](flow-creative.md) to organize a canvas, connect nodes for local image processing, or export a frame.

## Choose where projects are saved

| Platform | Default project directory |
| --- | --- |
| Linux | `$XDG_DATA_HOME/gpui-flow/projects`, or `~/.local/share/gpui-flow/projects` |
| macOS | `~/Library/Application Support/Flow Creative/Projects` |
| Windows | `%LOCALAPPDATA%/Flow Creative/Projects` |

Use `--projects-dir` to open the project home at a different location:

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --projects-dir ./projects
```

Use `--file` to open an existing standalone `FlowDocument` (graph and viewport):

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --file ./board.flow.json
```

Projects store their document and imported assets together. See [project storage](flow-creative.md#create-and-reopen-a-project) and [local persistence (中文)](../crates/flow-creative/README.md#本地持久化).

## Run examples without a desktop session

The [documentation examples](examples/src/bin/) use a standalone Cargo workspace. Run these from the repository root:

```sh
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin core
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin runner
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin workflow
```

`core` checks typed connections, transactions, undo/redo, and a document round trip. `runner` shows progress, completion, and retry for an independent task. `workflow` passes a value through two connected nodes and prints `HELLO CANVAS`. The examples make no external service calls.

To run just the graph model and runtime tests:

```sh
cargo test --locked -p gpui-flow-core -p gpui-flow-runtime
```

## Embed a canvas in your app

- Use `CreativeCanvas`, `CreativeRegistry`, and `ProjectStore` from `gpui-flow-creative` for creative objects and local projects. Enable its `desktop` feature for `CreativeCanvas`.
- Use `FlowCanvas` from `gpui-flow` for a node graph with your own node definitions and renderers, or `SceneView` for a retained display scene.
- Use the core and runtime crates directly for graph editing and task execution outside GPUI.

Use a GPUI dependency compatible with the pinned workspace revision. For asset setup, `gpui_kit::init(cx)`, and the shared text export backend, follow the [desktop entry point](../crates/flow-creative/src/main.rs). Continue with the [architecture and API guides](README.md#library-and-host-integration) or [local validation commands (中文)](testing.md).
