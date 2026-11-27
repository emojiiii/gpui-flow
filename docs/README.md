# gpui-flow documentation

[简体中文](README.zh-CN.md) · [Project README](../README.md)

Use Flow Creative to build and export a canvas project, or use the Rust libraries to embed a node graph or retained display scene in your own GPUI app.

## Get started

- [Install and launch Flow Creative](getting-started.md) on Windows, macOS, or Ubuntu.
- [Use the Flow Creative workbench](flow-creative.md) to arrange media, process an image through connected nodes, and export a frame.
- [Run the headless examples](getting-started.md#run-examples-without-a-desktop-session) for graph editing, background tasks, and workflows.
- [Read the full workbench reference (中文)](../crates/flow-creative/README.md) for limits, save behavior, and extension contracts.

## Library and host integration

| If you want to… | Read |
| --- | --- |
| Embed a node editor or understand its data model | [Graph architecture and persistence (中文)](architecture.md) · [Custom nodes, themes, and edges (中文)](custom-nodes.md) |
| Build a retained scene editor | [Scene objects and editing (中文)](display-scenes.md) · [Engine and event routing (中文)](engine.md) · [GPUI components (中文)](components.md) |
| Add custom input and editing behavior | [Interaction plugins (中文)](plugins.md) · [Custom scene tools](custom-tools.md) |
| Schedule tasks and pass values between nodes | [Runtime and runners (中文)](runtime.md) · [Workflow scheduling (中文)](workflows.md) |
| Play local audio or video | [Media backend and host setup (中文)](media.md) |

## Scene reference

- **Layout and animation** — [Flow layout](flow-layout.md), [reactive layout](reactive-layout.md), [animation (中文)](animation.md), and [timelines](animation-timeline.md)
- **Rendering and export** — [Appearance](appearance.md), [compositing](compositing.md), [Canvas components](canvas-component.md), [SVG export](scene-svg-export.md), and [raster export](scene-raster-export.md)
- **Shape examples** — [Ellipse and arc gallery](images/ellipse-arcs.svg) and [rectangle corner gallery](images/rect-corners.svg)
- **Text and resources** — [Text boxes](text-box.md), [resource lifecycle](resource-lifecycle.md), and [scene accessibility](scene-accessibility.md)
- **Leafer compatibility** — [Supported subset](leafer-compatibility-profile.md), [JSON exchange](leafer-json.md), and [feature ledger (中文)](leafer-task-ledger.md)

## Maintenance

- [Local validation and test commands (中文)](testing.md) · [CI workflow](../.github/workflows/ci.yml)
- [Preview and publish this documentation](publishing.md)
- [Known limitations and verification scope (中文)](limitations.md)
- [Studio retirement and retained regression coverage (中文)](studio-retirement.md). Older Studio references describe the earlier application. Current desktop commands are in [Getting started](getting-started.md).
