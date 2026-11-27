# gpui-flow documentation

[简体中文](README.zh-CN.md) · [Project README](../README.md)

Use gpui-flow to build a canvas application, or open Flow Creative to work on a local canvas project.

## Get started

<a id="build-with-the-gpui-flow-sdk"></a>

### Build with gpui-flow

- [gpui-flow guide](library.md) builds a typed graph and introduces `FlowCanvas` and `SceneView` for GPUI hosts.
- [Runnable examples](library.md#run-examples-without-a-desktop-session) cover graph editing, background tasks, and workflows without a desktop session.

### Use Flow Creative

- [Desktop setup](getting-started.md) covers installation and launch on Windows, macOS, and Ubuntu.
- [Workbench guide](flow-creative.md) covers arranging media, connecting nodes, and exporting a frame.
- [Workbench reference](creative-reference.en.md) covers file formats, export limits, and host integration.

## Library and host integration

| If you want to… | Read |
| --- | --- |
| Embed a node editor or understand its data model | [Graph architecture and persistence](architecture.en.md) · [Custom nodes, themes, and edges](custom-nodes.en.md) |
| Build a retained scene editor | [Scene objects and editing](display-scenes.en.md) · [GPUI components](components.en.md) |
| Add custom input and editing behavior | [Plugin example](examples/src/bin/plugins.rs) · [Custom scene tools](custom-tools.md) |
| Schedule tasks and pass values between nodes | [Workflows and runners](workflows.en.md) |
| Play local audio or video | [Local media and models](creative-reference.en.md#local-media-and-models) |

## Scene reference

- **Layout and animation** — [Flow layout](flow-layout.md), [reactive layout](reactive-layout.md), and [animation timelines](animation-timeline.md)
- **Rendering and export** — [Appearance](appearance.md), [compositing](compositing.md), [Canvas components](canvas-component.md), [SVG export](scene-svg-export.md), and [raster export](scene-raster-export.md)
- **Shape examples** — [Ellipse and arc gallery](images/ellipse-arcs.svg) and [rectangle corner gallery](images/rect-corners.svg)
- **Text and resources** — [Text boxes](text-box.md), [resource lifecycle](resource-lifecycle.md), and [scene accessibility](scene-accessibility.md)
- **Leafer compatibility** — [Supported subset](leafer-compatibility-profile.md) and [JSON exchange](leafer-json.md)

## Maintenance

- [Local validation and test commands](testing.en.md) · [CI workflow](../.github/workflows/ci.yml)
- [Preview and publish this documentation](publishing.md)
- [Known limitations](limitations.en.md)
