# 画布与 GPUI 组件

从 `gpui_flow::components` 导入 `FlowCanvas`、`SceneView` 和外围控件。画布是有状态的 GPUI Entity，由宿主挂到 `.child(...)`。按钮、节点外壳、端口和缩略图是由宿主传入状态与回调的 GPUI 控件。

## 对外画布 API

画布根和容器采用 LeaferJS 的父子模型。`add` 接收待挂载的数据，`find_id` 在当前画布查找，`children` 查询直属子级，`remove` 从所属画布移除对象。`SceneView` 还提供 `add_child` 和 `parent`，用 `Group`、`Box` 或 `Frame` 组织图形子树。[LeaferJS 容器](https://www.leaferjs.com/ui/en/reference/display/Group.html) · [按 ID 查找](https://www.leaferjs.com/ui/en/reference/UI/id.html)

| 操作 | 流程画布 | 图形画布 |
| --- | --- | --- |
| 挂载到 GPUI | `FlowCanvas::mount(document, cx)` | `SceneView::mount(scene, cx)` |
| 添加到根 | `add(node, cx)` | `add(object, cx)` |
| 查询 | `find_id(&id)`、`children()`、`node(&handle)` | `find_id(&id)`、`children(parent)`、`object(handle)` |
| 嵌套 | 图节点在所属画布内显示，`NodeRenderer` 提供节点内容 | `add_child(parent, object, cx)`、`parent(handle)` |
| 编辑 | `select(&handle, cx)`、`remove(&handle, cx)` | `select(handle, cx)`、`remove(handle, cx)` |

`FlowCanvas::mount` 使用 canvas-only 布局。用 `register_node_renderer` 为当前画布注册节点内容，用 `set_options` 调整外围控件。`NodeFrame` 等独立控件可用于宿主自己的布局。挂载后的流程节点和场景元素以句柄引用，由所属画布渲染和编辑。两块画布可以使用相同 ID，跨画布句柄会被拒绝。

宿主可通过 `FlowCanvas::agent_snapshot()` 和 `apply_agent_edit(request, cx)` 读取和批量编辑当前流程画布，见 [Agent 操作与 JSON Schema](agent-canvas.md)。

`SceneView::mount_with_profile(scene, LeaferCompatibilityProfile, cx)` 启用已有的有界 Leafer 鼠标、拖动、缩放和文本编辑行为。布局与容器绘制仍使用文档中显式设置的属性，适配范围见[兼容任务表](leafer-task-ledger.md)。

## 同窗双画布

[同窗示例](../examples/studio/src/bin/dual_canvas.rs)分别创建两块流程画布或两块图形画布。两侧故意使用相同 ID。可分别点击、删除和撤销，也可运行验证模式自动检查选区、撤销、父子关系和外来句柄拒绝。

```bash
cargo run --locked -p flow-studio --bin dual_canvas
cargo run --locked -p flow-studio --bin dual_canvas -- --scene
cargo run --locked -p flow-studio --bin dual_canvas -- --verify
cargo run --locked -p flow-studio --bin dual_canvas -- --scene --verify
```

## 独立控件组合

可运行的完整示例：[composition.rs](../examples/studio/src/bin/composition.rs)。左侧卡片完全独立，右侧把 `canvas_only` 画布嵌入自定义宿主界面，并注册自己的结果节点 renderer。

```bash
cargo run --locked -p flow-studio --bin composition
```

## 1. 独立节点，不创建 FlowCanvas

下面是可放进普通 GPUI 应用的完整 `Render` 实现：

```rust
use gpui::{prelude::*, *};
use gpui_flow::{FlowTheme, Handle, NodeFrame, TextNode};
use gpui_flow::core::{DataType, Point, PortDirection, Size};

struct PromptCard {
    selected: bool,
}

impl Render for PromptCard {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        NodeFrame::new("prompt-card", "Prompt")
            .size(Size::new(280., 180.))
            .selected(self.selected)
            .theme(theme)
            .body(TextNode::new("A quiet landscape, soft light").theme(theme))
            .on_header_mouse_down(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.selected = !this.selected;
                cx.notify();
            }))
            .child(
                Handle::new("text-out", PortDirection::Output, DataType::String)
                    .label("Text")
                    .anchor(Point::new(280., 80.))
                    .theme(theme)
                    .on_connect(cx.listener(|_, _, _, cx| {
                        cx.stop_propagation();
                        // 在这里通知宿主连接控制器；组件不会自行修改 Graph
                    })),
            )
    }
}
```

尺寸约定非常重要：

- `NodeFrame::size` 是逻辑尺寸；`zoom` 会缩放外壳和标题
- 不设置 `position` 时，节点参与普通 GPUI 布局
- `position` 是父元素中的屏幕坐标，设置后改为绝对定位；不要再次乘 zoom
- `body` 中的组件需要显式使用相同的 zoom，外壳不会对任意 GPUI 子树施加几何变换
- `PortHandle::anchor` 是父元素中的屏幕坐标，也就是端口中心；画布传入逻辑 anchor × zoom
- `Handle` 是 `PortHandle` 的别名；图模型端口仍叫 `gpui_flow::core::Port`
- `on_header_mouse_down` / `on_resize_mouse_down` / `on_mouse_down` 接受 GPUI 指针回调；没有 resize 回调时不显示缩放把手

`NodeFrame::child` 是叠加层插槽，适合端口、角标、自定义按钮；`body` 是内容插槽。这些插槽均接受任意 `IntoElement`。

## 2. 内置节点内容可单独使用

| 类型 | 入口 | 可选能力 |
| --- | --- | --- |
| TextNode | `new(text)` | `theme`、`zoom` |
| ImageNode | `new(local_path)` / `empty()` | `alt`、`caption`、`preview_height` |
| VideoNode | `new(source)` | `player`、`duration`、`poster`、`caption`、`on_open` |
| AudioNode | `new(source)` | `player`、`duration`、`waveform`、`caption`、`on_play` |
| GenerationNode | `new(prompt)` | `provider`、`model`、`progress`、`status`、`on_run` |
| AgentNode | `new(prompt)` | 组合 GenerationNode，提供相同执行状态与回调 |
| NoteNode | `new(text)` | 无端口便签内容 |
| OutputNode | `new(text)` | `content` 可替换/扩展结果区域 |

所有类型都支持 `from_node(&core::Node)`。每个节点模块还提供 `text_node_body`、`image_node_body` 等函数，统一签名为 `(&Node, NodeRenderContext) -> AnyElement`。`builtin_node_body` 是方便的内置分发入口；`FlowCanvas` 先查自定义 `NodeRegistry`，没有命中才调用它。

图片使用 GPUI 图片元素加载本地路径。音视频节点可通过 `.player(Entity<MediaView>)` 接入真实 FFmpeg 解码、FFplay 音频输出和播放/暂停/seek 控件；详见 [媒体后端](media.md)。未接入 player 时，视频显示元数据/本地 poster，音频只显示宿主传入的波形，不生成假波形。原有 `on_open` / `on_play` 回调保留给自定义宿主。`GenerationNode::new` 也不会伪装已经配置供应商；只有 `provider == "mock"` 才显示 MOCK 标识。`builtin_node` 的离线示例数据仍明确使用 mock。

## 3. 可替换的编辑器周边组件

- `ControlButton::new(id, label).on_click(...)`：普通操作按钮，可 disabled
- `Toolbar::new(id).leading(element).child(action)`：左右内容插槽
- `Palette::new(id).entries(...)`：数据驱动组件面板；`PaletteEntry` 包含 kind、label、color；`on_select` 返回 kind
- `Inspector::new(id).field(label, value).child(editor)`：属性面板；只读 field 和任意编辑器插槽
- `ViewportControls::new(id, zoom).on_action(...)`：发送 `ZoomOut`、`Reset`、`ZoomIn`、`Fit`，不会自行改变视口
- `MiniMap::new(id, visible_world_rect).nodes(...)`：每个 `MiniMapNode` 包含 world bounds、color、selected；`on_navigate` 返回点击对应的世界坐标

`Palette::on_select`、`ViewportControls::on_action`、`MiniMap::on_navigate` 都能直接使用 `cx.listener(...)`。在同一个父级内使用多个同类组件时给它们不同的 id；媒体/生成节点也提供 `.id(...)`，`from_node` 自动采用节点 id。

缩略图组件本身不负责定位；像其他组件一样由宿主包一层 div，放在需要的位置。FlowCanvas 的缩略图点击会以点击位置为中心平移视口。

```rust,ignore
MiniMap::new("overview", editor.viewport.visible_world_rect(extent))
    .nodes(editor.graph().nodes.values().map(|node| {
        MiniMapNode::new(node.bounds(), kind_color(&node.kind))
            .selected(editor.selection.nodes.contains(&node.id))
    }))
    .on_navigate(cx.listener(|this, world: &Point, _, cx| {
        this.editor.viewport.pan =
            Point::new(this.extent.width / 2., this.extent.height / 2.)
            - *world * this.editor.viewport.zoom;
        cx.notify();
    }))
```

## 4. 边界与验证

- body 组件只管内容，框架外壳只管外观与回调；业务数据、连接校验、撤销事务、文件保存、任务执行分别由 core、宿主和 runtime 负责
- 原来的 `badge`、`progress_bar`、`panel_title`、`builtin_node`、`kind_color`、`BUILTIN_KINDS`、`demo_graph` 根导出保留
- `components/` 和 `nodes/` 路径也公开，可精确导入单个模块
- 本文示例属于组合教程；最终交互、字体、裁剪、DPI、真实媒体和输入法仍需在目标 GPUI 桌面运行验收
- 组件单元测试覆盖构造参数、端口类型、缩略图坐标往返和节点 payload；这些测试不等同于图形 E2E
