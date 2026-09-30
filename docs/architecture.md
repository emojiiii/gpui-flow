# 节点流层架构、图模型与持久化

本文描述当前节点流层；后续组件/显示场景层的设计和验证范围应单独补充，以下分层不是最终架构承诺。

## 分层职责

```text
flow-studio                         应用入口，gpui-kit 初始化
    └── gpui-flow                   FlowCanvas、渲染器、主题、内置节点
            ├── gpui-flow-core      图、几何、选择、事务、历史、文档文件读写
            └── gpui-flow-runtime   后台 runner、取消、任务事件
```

core 和 runtime 互不依赖，也都不依赖 GPUI。核心模型可以在服务端、命令行、单元测试和其他 UI 中复用。runtime 不认识 Graph 的边，只按字符串节点 ID 运行独立任务。gpui-flow 负责把输入事件转为 core 操作，把 runtime 结果渲染成状态。

## 图模型

`Graph` 使用 `BTreeMap` 保存节点、边、组，得到确定的 ID 顺序和可比较的序列化输出；这不是视觉 z-index 或执行顺序。

- `NodeId`、`PortId`、`EdgeId`、`GroupId`：强类型字符串 ID
- `Node`：`kind`、`title`、世界坐标 `position`、`size`、端口、任意 JSON `data` 和 `locked`
- `Port`：方向、`DataType`、连接容量和应用数据
- `PortRef`：节点 ID 与端口 ID 的组合
- `Edge`：source、target、label、应用数据
- `Group`：显式矩形、成员节点集合、标题、数据和锁定标记
- `GraphOptions`：环和自连接策略

`Node::new` 创建 `kind="default"`、224 × 144 的节点。内置节点工厂另外使用 252 × 190。端口 ID 在节点内唯一即可，不要求跨节点唯一。

### 类型和连接约束

`DataType` 支持 `Any`、`Bool`、`Number`、`String`、`Object`、`Named(String)`。除 `Any` 外按精确类型匹配；`Any` 位于任意一端都可匹配另一端。`Named("image")` 只是应用约定，不附带媒体解码或 JSON schema 校验。

默认输入端口最多 1 条连接，默认输出不限数量。`with_capacity(None)` 表示不限；`Some(0)` 禁止连接。`Graph::connect` / `add_edge` 检查：

1. source/target 节点和端口存在
2. 输出端口连接到输入端口
3. 类型兼容、容量未满、不重复同一对端口
4. 符合自连接与环策略

默认 `allow_cycles = true`、`allow_self_connections = false`。构建 DAG 应显式设置 `graph.options.allow_cycles = false`；允许环的画布不等于 runner 能执行循环。

### 校验边界

Graph 字段公开，便于导入器和领域代码扩展。公开字段允许绕过守卫，因此外部直接修改后要调用 `Graph::validate()`。`validation_issues()` 返回带路径的多条错误，方便导入 UI 定位错误字段。

几何校验拒绝 NaN、无穷、非正节点尺寸和溢出的矩形边界；图校验检查 schema、ID、端口、连线、组成员引用和单组归属。任意 JSON `data` 的业务合法性由宿主负责。

## 编辑器、事务与撤销

`Editor::new(graph)` 先校验图。通过 `editor.graph()` 获取只读图，通过 `editor.transaction(label, |graph| { ... })` 修改。事务在候选副本上编辑，成功并通过校验才提交；错误不会留下部分图修改。

一次离散编辑对应一个历史条目。一次手势的多帧变化可以合并：

```rust
use gpui_flow_core::{Editor, Point};

fn move_selected(editor: &mut Editor) -> Result<(), gpui_flow_core::GraphError> {
    editor.begin_transaction("Move selection")?;
    editor.move_selection(Point::new(10.0, 0.0))?;
    editor.move_selection(Point::new(5.0, 8.0))?;
    editor.commit_transaction()?;
    Ok(())
}
```

这是 API 形状示例；生产事件处理还应在后续操作失败/取消时调用 `cancel_transaction()`，避免留下未结束事务。示例代码的 `?` 不会自动取消已经开始的手势。

- `cancel_transaction()` 恢复手势开始前的图和选择
- 活动手势中不能开始嵌套手势，也不能撤销/重做
- 历史默认最多 100 条；用 `set_history_limit` 调整
- 历史采用完整图快照，不是增量 patch，适合原型和中小图
- 撤销/重做恢复图和选择，不恢复视口或后台任务
- `locked` 在常用移动/缩放/删除等编辑操作中检查；不是安全权限控制，直接事务可由宿主修改字段

### 复制、粘贴和分组

`copy_selection()` 产生可序列化的 `Clipboard` 片段。复制被选节点、选中组的成员，以及全部端点都位于片段内的连线；外部连线不进入片段。`paste` 分配新 ID、重映射连线与组成员、按世界坐标偏移，并作为一次撤销操作。

分组只有一层，节点最多属于一个组。`group_selection` 会将成员从旧组移到新组；旧空组保留。组的矩形是显式几何，不会随单个成员移动自动调整，需要调用 `refresh_group_bounds`。删除组保留其成员节点，除非成员节点也被直接选中删除。

## 坐标约定

世界坐标与画布局部屏幕坐标分离：

```text
screen = world * zoom + pan
world  = (screen - pan) / zoom
```

`pan` 单位是屏幕像素，`zoom` 是每世界单位对应的屏幕像素。GPUI 指针事件使用窗口坐标，FlowCanvas 先减画布原点，再交给 Viewport 转换；不要额外重复减侧栏宽度。

`zoom_at(anchor, zoom)` 保持鼠标下世界点不动。默认缩放范围为 0.1–3.0；`fit` 也遵守该范围，极大图可能无法在最小缩放下完全塞进视口。MiniMap 使用相同变换生成缩略图几何，当前 UI 支持点击缩略图重新居中，不支持拖动导航。

## 持久化格式与边界

存在两种有意区分的入口：

| API | JSON 内容 | 适用场景 |
| --- | --- | --- |
| `Graph::to_json/from_json` | Graph 本身 | 图拓扑与节点数据交换 |
| `FlowCanvas::export_json/import_json` | Graph 本身 | Studio 快照、Ctrl/Cmd + S、宿主导入 |
| `FlowDocument::to_json/from_json` | `{ graph, viewport }` | 连同相机保存 |
| `Editor::to_json/from_json` | FlowDocument | 宿主持久化完整可视文档状态 |
| `save_document/load_document` | FlowDocument 文件，最多 32 MiB | 带校验的磁盘保存/读取 |
| `FlowCanvas::save_file/open_file` | 原生对话框 + FlowDocument 文件 | Studio / 宿主交互保存 |

Graph 的 `schema_version` 当前为 1。未知版本会被拒绝；尚未提供迁移器。请用这些带校验的 API 代替直接 `serde_json::from_str::<Graph>`。`FlowCanvas::import_json` 将导入作为可撤销事务，保留现有视口，清空选择并重置 Runtime；`Editor::load_document` 替换图和视口，清空选择与历史，并保留配置的历史上限。

```rust
use gpui_flow_core::{Editor, FlowDocument};

fn save(editor: &Editor) -> Result<String, gpui_flow_core::GraphError> {
    editor.to_json()
}

fn restore(editor: &mut Editor, json: &str) -> Result<(), gpui_flow_core::GraphError> {
    let document = FlowDocument::from_json(json)?;
    editor.load_document(document)
}
```

选择、撤销历史、运行记录、取消令牌、runner、图片字节都不序列化。Graph 中的路径只是字符串引用，不是资源打包。

文件 API 提供有界读取、保存前校验和同目录临时文件替换：

```rust
use gpui_flow_core::{Editor, load_document, save_document};

fn save_and_restore(editor: &mut Editor) -> Result<(), Box<dyn std::error::Error>> {
    save_document("canvas.flow.json", &editor.document())?;
    editor.load_document(load_document("canvas.flow.json")?)?;
    Ok(())
}
```

`PersistenceError` 区分 IO、无效文档和超过 32 MiB。读取最多 32 MiB 加一个哨兵字节，拒绝超限和非 UTF-8；保存写入临时文件、`sync_all` 后 rename，不创建父目录。rename 的替换/持久性保证受 OS 与文件系统影响，当前不承诺跨所有平台的断电恢复保证，目录本身不额外 fsync。自动保存、脏状态/覆盖提示、备份、多实例冲突、媒体复制和 schema 迁移仍由宿主实现。

## 渲染与事件

`FlowCanvas` 绘制网格、所有边、组、可见节点和概览。节点 body 由 `NodeRegistry` 中的 `NodeRenderer` 渲染，标题栏、端口、选择外框、移动和缩放由画布管理。`EdgeRenderer` 只决定绘制，不提供点击命中或图编辑行为。

画布约每 40ms 非阻塞轮询一次 Runtime，收到已应用事件后发出 `RunChanged(node_id)` 并通知重绘。事件不等于 Graph 自动发生变化。宿主直接变更公共字段后需按 GPUI 实体更新流程调用 `cx.notify()`，必要时自行 `cx.emit(...)`。

## 通用显示场景层

本页专门描述节点图。通用 DisplayObject/Scene、SceneEditor、SceneView 与逐组件UI属于独立分层，见[显示场景架构](display-scenes.md)与[组件组合](components.md)，不要把节点图 Group 当作通用场景容器。
