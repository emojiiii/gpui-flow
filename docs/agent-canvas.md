# Agent 操作流程画布

宿主选择一个 `Editor` 或 `FlowCanvas` 实例，把当前快照交给 agent。Agent 根据快照中的节点、端口、连线和选区，用稳定 ID 生成一批 JSON 操作。宿主解析并提交操作，返回提交后的版本和变更 ID。

请求格式见 [JSON Schema](../crates/flow-core/schema/agent-edit-v1.schema.json)，也可通过 `gpui_flow_core::AGENT_EDIT_SCHEMA` 读取。宿主可用它校验工具输入，并根据接入平台提供工具描述。

请求中的坐标、尺寸、端口和连线端点只接受 Schema 声明的字段。业务 `data` 使用 JSON 值。

`gpui_flow::builtin_agent_node_templates()` 返回标准工作流节点的种类、默认标题、尺寸、带类型的端口和默认数据。将模板与快照一起交给 agent，创建节点时就可以选用现有端口，并填入新的 ID 和位置。自定义画布可用 `AgentNodeTemplate` 提供自己的节点模板，由宿主校验业务字段。

## 读取与提交

1. 在无窗口程序用 `Editor::agent_snapshot()`，在 GPUI 宿主用 `FlowCanvas::agent_snapshot()`。把 `schema_version`、`revision`、`graph` 和 `selection` 序列化给调用方。
2. 将 agent 返回的 `AgentEditRequest` 提交给同一实例。无窗口入口是 `Editor::apply_agent_edit(request)`，GPUI 入口是 `FlowCanvas::apply_agent_edit(request, cx)`。
3. 成功时读取回执里的 `revision` 和 `changes`。版本是在当前进程内使用的字符串，按原样传回。其他画布的版本或已被更新的版本会被拒绝，重连时重新读取快照。

下例使用 Studio 自带的 `text` 和 `output` 节点，一次创建两个节点并连接它们。提交时将 `expected_revision` 换成当前快照里的 `revision`。

```json
{
  "schema_version": 1,
  "expected_revision": "42",
  "operations": [
    {
      "op": "add_node",
      "id": "prompt",
      "title": "Prompt",
      "kind": "text",
      "position": { "x": 40, "y": 80 },
      "ports": [
        { "id": "out", "label": "Text", "direction": "output", "data_type": { "kind": "string" } }
      ],
      "data": { "text": "Summarize this passage." }
    },
    {
      "op": "add_node",
      "id": "summary",
      "title": "Summary",
      "kind": "output",
      "position": { "x": 360, "y": 80 },
      "ports": [
        { "id": "in", "label": "Input", "direction": "input", "data_type": { "kind": "any" }, "max_connections": 1 }
      ]
    },
    {
      "op": "connect",
      "id": "prompt-to-summary",
      "source": { "node": "prompt", "port": "out" },
      "target": { "node": "summary", "port": "in" }
    }
  ]
}
```

## 操作与回执

| `op` | 行为 |
| --- | --- |
| `add_node` | 以指定 ID 加入节点，位置使用画布世界坐标。可指定节点种类、尺寸、端口和数据。 |
| `update_node` | 更新标题、种类、位置、尺寸或端口。省略或传 `null` 的字段保留原值，`ports: []` 清空端口。 |
| `set_node_data` | 替换节点的业务数据。`data: null` 清空数据。 |
| `remove_node` | 删除节点，同时清理关联的连线和分组成员关系。 |
| `connect` | 以指定 ID 将输出端口连到输入端口。图模型校验端口类型、方向、容量和环策略。 |
| `disconnect` | 根据连线 ID 删除连线。 |

操作按数组顺序在候选图上执行，整批通过校验后一次提交、一次撤销。回执对节点、连线和分组分别返回 `inserted`、`updated`、`removed` 的 ID。数组记录净变化，空操作列表允许原样返回版本。锁定节点的更新和删除会被拒绝。GPUI 宿主的只读策略、正在进行的手势和文档校验也会影响提交。

宿主使用 `FlowCanvas::set_document_validator` 校验整批提交后的业务文档。对有对应画布命令的操作，现有插件过滤器也会审查。插件可通过 `CanvasPlugin::authorize_agent_operation` 针对所有 Agent 操作增加领域规则。规则读取的是执行到当前操作时的候选图。同一批提交中，已有节点的 ID 删除后不能重新创建。

宿主使用 `FlowCanvas::set_agent_assets` 登记可供 Agent 使用的素材 ID 和本地路径。内置图片、音频和视频节点的 `data.source`，以及视频的 `data.poster`，在 Agent 快照和编辑请求中使用这些 ID。快照会省略未登记的内置素材路径。修改节点类型时，在同一批里清理新类型不再使用的素材字段。宿主决定自定义节点业务数据的分享范围，并使用文档校验器实施相应规则。

失败时保留原图和撤销记录。调用方可按 `code` 处理错误，用 `message` 展示原因。逐项错误可包含从零起算的 `operation_index`，图校验错误可包含附有 `path` 和 `message` 的 `issues`。版本过期错误包含 `current_revision`。重新读取快照后，根据新图决定是否提交。

运行 `cargo run --locked -p flow-studio --bin dual_canvas -- --verify` 可验证两块原生画布之间的版本隔离，以及批量提交使用画布现有的撤销机制。
