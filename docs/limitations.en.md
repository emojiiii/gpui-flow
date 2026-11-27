# Known limitations

[简体中文](limitations.md) · [gpui-flow guide](library.md)

| Area | Current boundary |
| --- | --- |
| Graph editor | Connections use typed ports. Graph groups are one level deep. Reconnecting an existing edge by dragging and editing arbitrary node fields require host UI. |
| Scene editor | `SceneView` supports a retained object tree and editor commands. Text, images, custom objects, and effects have specific transform and rendering limits. See [display scenes](display-scenes.en.md) for the relevant APIs. |
| Large documents | Graph history stores snapshots. Test memory and rendering time with representative documents before selecting a history limit or a target scene size. |
| Workflow execution | The runtime executes tasks locally. Workflows use a validated DAG with bounded concurrency. Durable queues, conditional branches, loops, and distributed workers are outside the workflow model. |
| Collaboration | Graph and scene documents are local. Real-time collaborative editing and conflict resolution require host infrastructure. |
| Media and export | The workbench handles supported local media files. See [Flow Creative reference](creative-reference.en.md) for codecs, 3D preview, export types, and file limits. |

## Host responsibilities

- Keep credentials out of graph payloads, exported documents, clipboard data, and logs. The runtime calls the runners and providers supplied by the host.
- Validate imported data and limit which local paths an untrusted document can reference. Editing and locking policies control editor commands within the host.
- Check input devices, IME, accessibility, and rendering on the operating systems you intend to support. See [testing](testing.en.md) for the repository's verification commands.

See the [Leafer compatibility profile](leafer-compatibility-profile.md) for the opt-in behavior and [JSON exchange](leafer-json.md) for the supported document subset.
