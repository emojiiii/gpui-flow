# gpui-flow documentation

[简体中文](README.zh-CN.md) · [Project README](../README.md)

Choose the gpui-flow SDK to build a canvas application or Flow Creative to work on a local canvas project.

## Get started

### Build with the gpui-flow SDK

- [SDK quickstart](library.md) builds a typed graph and introduces `FlowCanvas` and `SceneView` for GPUI hosts.
- [Runnable SDK examples](library.md#run-examples-without-a-desktop-session) cover graph editing, background tasks, and workflows without a desktop session.

### Use Flow Creative

- [Desktop setup](getting-started.md) covers installation and launch on Windows, macOS, and Ubuntu.
- [Workbench guide](flow-creative.md) covers arranging media, connecting nodes, and exporting a frame.
- [Full workbench reference (Chinese)](../crates/flow-creative/README.md) covers formats, save behavior, and host extension contracts.

## Library and host integration

| If you want to… | Read |
| --- | --- |
| Embed a node editor or understand its data model | [Graph architecture and persistence (Chinese)](architecture.md) · [Custom nodes, themes, and edges (Chinese)](custom-nodes.md) |
| Build a retained scene editor | [Scene objects and editing (Chinese)](display-scenes.md) · [Engine and event routing (Chinese)](engine.md) · [GPUI components (Chinese)](components.md) |
| Add custom input and editing behavior | [Interaction plugins (Chinese)](plugins.md) · [Custom scene tools](custom-tools.md) |
| Schedule tasks and pass values between nodes | [Runtime and runners (Chinese)](runtime.md) · [Workflow scheduling (Chinese)](workflows.md) |
| Play local audio or video | [Media backend and host setup (Chinese)](media.md) |

## Scene reference

- **Layout and animation** — [Flow layout](flow-layout.md), [reactive layout](reactive-layout.md), [animation (Chinese)](animation.md), and [timelines](animation-timeline.md)
- **Rendering and export** — [Appearance](appearance.md), [compositing](compositing.md), [Canvas components](canvas-component.md), [SVG export](scene-svg-export.md), and [raster export](scene-raster-export.md)
- **Shape examples** — [Ellipse and arc gallery](images/ellipse-arcs.svg) and [rectangle corner gallery](images/rect-corners.svg)
- **Text and resources** — [Text boxes](text-box.md), [resource lifecycle](resource-lifecycle.md), and [scene accessibility](scene-accessibility.md)
- **Leafer compatibility** — [Supported subset](leafer-compatibility-profile.md), [JSON exchange](leafer-json.md), and [feature ledger (Chinese)](leafer-task-ledger.md)

## Maintenance

- [Local validation and test commands (Chinese)](testing.md) · [CI workflow](../.github/workflows/ci.yml)
- [Preview and publish this documentation](publishing.md)
- [Known limitations and verification scope (Chinese)](limitations.md)
- [Studio retirement and retained regression coverage (Chinese)](studio-retirement.md). Older Studio references describe the earlier application. Current desktop commands are in [Getting started](getting-started.md).
