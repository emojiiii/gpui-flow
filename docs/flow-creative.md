# Flow Creative

[Documentation](README.md) · [简体中文操作指南](../crates/flow-creative/README.md)

Flow Creative is a desktop workbench for arranging media, text, and drawings on an infinite canvas. Use typed ports to pass inputs between nodes and export assets or finished frames. Install the app from source and create your first project with the [getting started guide](getting-started.md).

## Create and reopen a project

1. Start Flow Creative, enter a project name, and select **新建项目** (New project). The workbench saves the project locally before opening its blank canvas.
2. Drop an image onto the canvas and add a text caption. Click the project name at the top to rename it.
3. Choose **返回项目** (Back to projects). Reopen the project from the home screen to resume with its objects, connections, assets, and viewport. Use the home search to find projects by name.

Use a regular local project for the steps below. The `--demo` and `--file` launch modes open a standalone canvas. A local project keeps `project.json` and imported files in its project folder. Choose a different project root with `--projects-dir`, as shown in [getting started](getting-started.md#choose-where-projects-are-saved). For save and asset storage behavior, see [local persistence (中文)](../crates/flow-creative/README.md#本地持久化).

## Navigate and edit the canvas

| Task | Control |
| --- | --- |
| Select or move an object | Press **V**, then click or drag. Use **Shift** to select multiple objects. |
| Pan and zoom | Press **H** for the hand tool or hold **Space** and drag. Use the bottom zoom controls or **适应画布** (Fit canvas) to bring objects into view. |
| Add text | Press **T**, then click or drag to place a text box. Enter text and press **Esc** to finish. |
| Draw a frame or freehand stroke | Press **F** and drag a frame, or press **P** and draw with the pencil. Use the shapes menu for a rectangle, ellipse, or line. |
| Reorder, hide, or lock objects | Open **图层** (Layers) on the left. Search by object or frame name, or choose the locate action to center the selected object. |
| Undo or redo an edit | Use **撤销** (Undo) and **重做** (Redo) at the top of the canvas. |

Canvas shortcuts work while the canvas has focus. Select an object to reveal its floating controls. Static PNG and JPEG images support crop, rotation, flip, and color adjustment. Double-click a text object to edit it in place. The upper toolbar lets you change size, weight, alignment, font, and color. See [canvas controls (中文)](../crates/flow-creative/README.md#操作).

## Bring in local media

Drop media files from your file manager to place them at the pointer. You can also right-click an empty area and choose **导入素材到此处…** (Import assets here). If the file picker is unavailable, enter an absolute path in the local-path field. Open **本项目素材** (Project assets) on the left to reuse previously imported files.

| Media | What you can do on the canvas |
| --- | --- |
| PNG, JPEG, GIF, or WebP image | Place the image at its original aspect ratio or adjust its display size. Use a static PNG or JPEG to crop or adjust the source pixels while retaining the original asset. [Image editing (中文)](../crates/flow-creative/README.md#图片裁剪旋转与翻转) |
| MP4 or WebM video, WAV, MP3, FLAC, OGG, or M4A audio | Play, pause, and seek from the selected object's controls. Install the [FFmpeg tools](getting-started.md#optional-media-tools) first. [Audio and video (中文)](../crates/flow-creative/README.md#本地音视频对象) |
| Supported static GLB | Inspect the embedded mesh in a local preview. Select **旋转模型** (Orbit model) to drag the viewpoint. [GLB support and limits (中文)](../crates/flow-creative/README.md#本地-3d-预览) |

## Connect nodes and process an image

Creative connects outputs to inputs of the same type. An **Image** output can supply an **Image** input on a processing or generation node. Use the destination node's action to run its task.

1. In a local project, import and select a static PNG or JPEG.
2. In the floating toolbar, choose **创建本地处理节点** (Create local processing node). The new image processing node is already connected to the source.
3. Select the processing node. Choose PNG or JPEG and a longest-edge preset such as **1024 px**, then click **运行处理** (Run processing).
4. After processing completes, use the output with another Image input or export it from the download button. The result appears as a new project asset and the original stays available. Run the node again if the input or settings change.

For a manual connection, create a node from **本地媒体处理** (Local media processing) and drag the source output to the matching input. Image processing runs locally with the workbench. Audio and video processing use FFmpeg and FFprobe. See [local media processing (中文)](../crates/flow-creative/README.md#本地媒体处理节点).

Image, video, audio, and model generation nodes save prompts and settings in the project. An image wired to the image generator supplies its visual reference. Running a generation node requires a service supplied by the embedding application. See [generation requests (中文)](../crates/flow-creative/README.md#四类生成节点与宿主接入).

## Export a frame or an asset

To export a small layout, use **F** to draw a frame around your visible, unlocked, ungrouped image and text. If you move an existing object into a frame later, select the frame and use **收纳框内对象** (Collect objects inside frame). Select the frame title and choose **导出画框…** (Export frame) from its context menu.

| Export | Supported selection |
| --- | --- |
| Frame PNG | One frame containing visible static PNG/JPEG images, text, shapes, or strokes. The export uses the frame bounds and clips its contents to those bounds. |
| Frame SVG | One frame whose visible members are shapes or freehand strokes. |
| Standalone SVG | One or more selected shapes or freehand strokes on a transparent background. |
| Original asset | A file from **本项目素材** or a node with a valid media output. Media retains its original format. |

Save to a new file outside the project folder. To combine images and text, use frame PNG. Select media assets individually to export audio, video, or GLB. See [export limits and formats (中文)](../crates/flow-creative/README.md#素材导出).

## Embed and extend the workbench

- **Creative projects.** Use `CreativeCanvas` with the `desktop` feature, plus `CreativeRegistry` for node definitions and `ProjectStore` for local documents and assets. Follow the [desktop entry point](../crates/flow-creative/src/main.rs) for GPUI initialization and text export setup.
- **Generation.** Subscribe to `CreativeEvent::GenerationRequested`, then enable generation with `set_generation_available`. Resolve the request's asset keys from the active project, execute the provider task, and report updates with `apply_generation_update`. See [generation integration (中文)](../crates/flow-creative/README.md#四类生成节点与宿主接入).
- **Custom node execution.** Register a `CustomNodeExecutor` with `register_custom_executor`. Return the declared types and use existing project asset keys for media outputs. Work with the [custom node contract (中文)](../crates/flow-creative/README.md#自定义节点输入与输出).
- **General purpose canvas.** Use `FlowCanvas` for a node graph or `SceneView` for editable scenes. Start from the [library and host integration index](README.md#library-and-host-integration).
