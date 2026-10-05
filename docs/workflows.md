# 工作流调度

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

`gpui-flow-runtime::Workflow` 对启动时的 `Graph` 快照执行 DAG，不修改编辑器中的文档。它与单节点 `Runtime` 分离：后者仍适合宿主手动启动独立任务。

## Runner 数据契约

每一种参与执行的 node.kind 都必须注册 `Arc<dyn AgentRunner>`。缺失注册、无效连接、环或并发上限为零会在启动之前返回错误，不会先执行半张图。

Runner 接收：

```json
{"node":{"prompt":"节点原始 data"},"inputs":{"prompt":["上游文本"],"reference":[{"source":"image.png"}]}}
```

输入端口始终对应数组，未连接时为空，多连接时按 edge ID 排序。Runner 返回按输出端口 ID 索引的 JSON 对象：

```json
{"out":{"source":"result.png"}}
```

未声明的输出端口、缺少已连接的输出、JSON 类型不符合源或目标端口时，该节点失败，下游不会得到错误数据。Any 连接到具体 JSON 类型也在运行时验证。Named 类型是宿主定义的名义契约，只验证图连接的类型名称兼容，具体媒体对象结构由对应 runner 校验。

## 生命周期

- `Workflow::new(graph, runners, concurrency)` 只验证并准备，`poll()` 才开始工作
- 同时运行的任务不超过 concurrency；一个节点须等所有直接前驱成功才可执行
- 失败节点的后继递归进入 Skipped，不影响独立分支
- `cancel()` 取消尚未启动节点并发出运行中节点的协作取消 token；不伪装成已经停止。Runner 必须主动检查 token 并停止自己的远程调用/子进程
- 所有运行中 worker 终止后才能 `retry(node_id)`。重试失败/取消/跳过节点及其全部后继，保留无关的已成功输出；重试范围之外的所有前驱必须已成功。多个失败分支汇合时，使用 retry_many 同时选中这些前驱，或 retry_failed 重试所有未成功节点，避免单支重试互相等待
- 重试不会自动产生收费请求；是否调用服务由注册 runner 决定。没有自动重试或无限循环
- `records()` 提供每节点状态、attempt、progress、message 和 outputs；`is_finished()` 包含失败/取消，`succeeded()` 才代表全部成功
- 这是进程内调度器，不是断点续跑数据库或分布式队列。进程退出不恢复执行历史，也不保证外部副作用恰好一次；runner 应自行实现幂等键和凭据管理

## Studio

工具栏 Run workflow 使用三个并发槽，按连接顺序执行启动时的完整图。文本/媒体节点提供数据，generation/agent 使用明确标为 MOCK 的离线结果，output 接收上游值。选中节点可在 Inspector 查看 workflow 状态和输出。

Cancel runs 同时取消独立任务和 workflow。存在 workflow 时 Retry selected 对所有选中节点原子执行上面的局部重试；无选中节点则重试全部失败/取消/跳过分支；没有 workflow 时仍重试独立 Runtime 任务。打开文档会丢弃旧 workflow 并发出取消；编辑画布不会改变已经启动的快照。需要更改工作流时，等其结束后重新运行。

宿主在 FlowCanvas 上使用 `register_workflow_runner` 注册此 envelope 契约的实现，使用 `register_runner` 注册直接接收 node.data 的独立任务实现，两种输入契约不混用。
