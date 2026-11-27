# Flow Creative reference

[简体中文](../crates/flow-creative/README.md) · [Workbench guide](flow-creative.md)

## Local media and models

| Asset | Requirements and limits |
| --- | --- |
| Audio and video | MP4 and WebM video, or WAV, MP3, FLAC, OGG, and M4A audio. On Windows, Media Foundation can preview H.264 MP4 with optional AAC audio when FFmpeg tools are unavailable. See the [media tool requirements](getting-started.md#optional-media-tools) for other formats and platforms. Each asset is limited to 128 MiB and 30 minutes. |
| 3D models | GLB 2.0 with embedded buffers and textures. The static preview supports triangle meshes and opaque base colors. Animation, skinning, and external resources are outside the preview format. The file limit is 32 MiB, with an additional limit of 100,000 vertices and 50,000 triangles. |
| Clipboard images | Static PNG or JPEG. Each image is limited to 32 MiB, 4,096 pixels per axis, and 4,194,304 pixels in total. |

Import copies the original into the project. Playback position is local to the session. The GLB camera orientation is saved with the project.

## Export files

| Export | Limits |
| --- | --- |
| Frame PNG | Static PNG/JPEG images, text, shapes, and strokes. Exports at 1× canvas size, clipped to the frame. Each source image and the output are limited to 4,096 pixels per axis and 4,194,304 pixels in total. |
| SVG | Shapes and strokes. Up to 128 objects, 65,536 stroke points, 32,768 units per axis, and 4 MiB. A frame SVG clips the selected frame's visible contents to its bounds. |
| Original asset | Exports the existing asset in its original format. Choose the corresponding file extension. Each export is limited to 128 MiB and 60 seconds. |

Choose a new destination outside the project directory. Visible objects in unsupported formats prevent frame export. A stale or missing node output cannot be exported.

## Embed Flow Creative

Enable the `desktop` feature of `gpui-flow-creative` for `CreativeCanvas`. Use `CreativeRegistry` and `NodeDefinition` to register object definitions, `NodeRenderer` to supply the node body, and `ProjectStore` for project persistence.

For PNG text export in a host, create the application with `GpuiTextRasterizer::application`. Install the returned backend with `install_creative_text_export_backend` in that same application. The built-in workbench does this in its [desktop entry point](../crates/flow-creative/src/main.rs).

See [Embed a canvas in your app](library.md#embed-a-canvas-in-your-app) for the differences between `CreativeCanvas`, `FlowCanvas`, and `SceneView`.
