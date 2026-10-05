# 自定义节点、主题与连线

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

## 扩展点概览

一个自定义节点由三件独立的事组成：

1. **模型**：`Node.kind` 标识类型，`ports` 定义连接，`data` 存放可序列化业务数据
2. **视图**：`NodeRenderer` 根据节点和 `NodeRenderContext` 生成 GPUI 元素
3. **行为**：宿主使用编辑事务更新模型，按需要启动 `AgentRunner`

注册 renderer 不会自动注册 runner、组件面板入口、输入验证或调度逻辑；runner 可以另外用 `FlowCanvas::register_runner` 注册。内置组件列表目前固定为 `BUILTIN_KINDS`；业务应用可以程序化添加节点，并为自己的面板提供对应操作。

## 1. 定义节点数据与端口

```rust
use gpui_flow::core::{DataType, Node, Point, Port, Size};
use serde_json::json;

let node = Node::new("review-1", "Review", Point::new(400.0, 120.0))
    .with_kind("review")
    .with_size(Size::new(280.0, 180.0))
    .with_port(Port::input("draft", "Draft", DataType::String))
    .with_port(Port::output("approved", "Approved", DataType::String))
    .with_data(json!({"status": "Pending review", "text": "Draft content"}));
```

`kind` 是开放字符串，不必修改核心枚举。需要稳定迁移时，建议在应用 data 中保存自己的版本字段，并在导入后做领域校验。`DataType::Named("your-type")` 是精确字符串匹配，不是自动 JSON 验证器。

## 2. 实现 NodeRenderer

```rust
use gpui::{prelude::*, AnyElement, App, Window, div, px, rgb};
use gpui_flow::{NodeRenderContext, NodeRenderer};
use gpui_flow::core::Node;

struct ReviewRenderer;

impl NodeRenderer for ReviewRenderer {
    fn render(
        &self,
        node: &Node,
        context: NodeRenderContext,
        _window: &mut Window,
        _cx: &mut App,
    ) -> AnyElement {
        let status = node.data["status"]
            .as_str()
            .unwrap_or("Unknown")
            .to_owned();
        let color = if context.selected {
            context.theme.accent
        } else {
            context.theme.foreground
        };
        div()
            .p(px(12.0 * context.zoom))
            .text_size(px(13.0 * context.zoom))
            .text_color(rgb(color))
            .child(status)
            .into_any_element()
    }
}
```

Renderer 只提供节点主体。外层标题、拖动、选择、边框、固定位置端口和右下角缩放把手由画布提供；主体会被节点边界裁剪。画布没有自动给自定义元素施加统一缩放 transform，所以像素尺寸和字体要按 `context.zoom` 自行处理。`NodeRenderContext` 提供 `selected`、`zoom`、`theme`，不含 Runtime 或 `Context<FlowCanvas>`。

接口要求 `Send + Sync`；不要在 renderer 中执行阻塞 IO 或长计算。输入组件/复杂自定义子视图宜通过宿主创建和持有 GPUI Entity，并设计状态与事件桥接，而不是每次 render 重建后台任务。

## 3. 注册并创建画布

在已经运行 `gpui_kit::init(cx)` 的应用中，窗口回调可以构造：

```rust
use gpui_flow::{FlowCanvas, demo_graph};

let canvas = cx.new(|cx| {
    let mut graph = demo_graph();
    graph.add_node(node).expect("valid review node");
    let mut canvas = FlowCanvas::new(graph, cx).expect("valid graph");
    canvas.registry.register("review", ReviewRenderer);
    canvas
});
```

同一个 kind 再次注册会替换 renderer；未注册时使用内置 fallback。上面几段是同一个宿主流程的组合片段，`node` 和 `ReviewRenderer` 来自前两节，`cx` 来自 GPUI 回调，不是独立的命令行程序。

完整初始化形式与 Studio 一致：

```rust
use gpui::{App, AppContext};

gpui_kit::application()
    .with_assets(gpui_kit::assets::Assets)
    .run(|cx: &mut App| {
        gpui_kit::init(cx);
        // cx.open_window(...)
    });
```

宿主和组件必须使用同一个 GPUI Git 来源/提交，以免不同的 `gpui::Entity` / `Window` 类型无法互操作。当前兼容组合见根目录 `Cargo.toml` 与 `Cargo.lock`。

## 4. 通过事务修改内容

取得 `Entity<FlowCanvas>` 后，在宿主的 GPUI 更新回调中使用：

```rust
canvas.update(cx, |canvas, cx| {
    let result = canvas.editor.transaction("Approve review", |graph| {
        let node = graph.nodes.get_mut(&"review-1".into())
            .ok_or_else(|| gpui_flow::core::GraphError::MissingNode("review-1".into()))?;
        node.data = serde_json::json!({
            "status": "Approved",
            "text": "Approved content"
        });
        Ok(())
    });
    if result.is_ok() {
        cx.emit(gpui_flow::FlowEvent::DocumentChanged);
        cx.notify();
    }
    result
})?;
```

这是宿主回调片段；调用方应处理返回的 `GraphError` 并显示适当错误。建议一次用户动作一次事务，避免把纯渲染刷新写进撤销历史。直接更改节点数据不会启动任何外部服务。

### 内置内容字段约定

| kind | 主要字段 | 默认端口 |
| --- | --- | --- |
| `text` | `text` | String 输出 `out` |
| `image` | `source`、`alt`、`caption` | Named(image) 输入 `in` / 输出 `out` |
| `video` | `source`、`duration`、`caption` | Named(video) 输入 `in` / 输出 `out` |
| `audio` | `source`、`duration`、`caption` | Named(audio) 输入 `in` / 输出 `out` |
| `generation` | `prompt`、`provider`、`model` | String `prompt`、Named(image) `reference` 输入；Named(image) `out` 输出 |
| `agent` | `prompt`、`provider`、`model` | Any `in` / `out` |
| `note` | `text` | 无 |
| `output` | `text` | 不限连接的 Any `in` |

当前 Inspector 的内容输入为图片/音频/视频编辑 `source`，生成/agent 编辑 `prompt`，其他类型编辑 `text`。它不是通用表单 schema。`provider` / `model` 只是元数据，Studio 的 Run 按钮仍固定使用 MockRunner。

图片 `source` 被转为 `PathBuf`，仅代表本地图片路径。`https://...` 不会被当作下载地址。宿主须控制可读取的目录、文件类型和资源大小。视频和音频可通过 `.player(Entity<MediaView>)` 使用真实解码/播放后端；默认 FlowCanvas 不开启，宿主显式配置后才读取媒体文件。工具依赖、资源限制、同步精度和测试见 [媒体后端](media.md)。

## 5. 主题与画布选项

```rust
use gpui_flow::FlowTheme;

canvas.theme = FlowTheme {
    accent: 0x8b9dff,
    background: 0x10131c,
    ..FlowTheme::default()
};
canvas.show_grid = true;
canvas.replace_plugin(
    gpui_flow::interaction::plugins::SnappingPlugin::default().with_grid(24.),
    cx,
)?;
canvas.options = gpui_flow::FlowOptions::canvas_only();
```

`FlowOptions::canvas_only()` 隐藏 toolbar、palette、Inspector 和 status，保留 minimap 与 controls。六个 `show_*` 字段可以单独控制；这方便把画布放入宿主自己的窗口布局，但不是任意 panel 插槽系统。

`FlowTheme` 的颜色为 RGB `u32`。默认网格显示与网格吸附是两个开关；当前吸附间隔为 20 世界单位，显示点网格的基础间隔为 24 世界单位，两者尚未统一成可配置的布局系统。

## 6. 连线样式

```rust
use std::sync::Arc;
use gpui_flow::StraightEdge;

canvas.edge_renderer = Arc::new(StraightEdge);
```

默认使用 `BezierEdge`。自定义 `EdgeRenderer::paint` 接收转换后的屏幕像素端点、颜色和 `Window`，可以画正交线或其他路径。不要再转换 viewport；当前调用方已经转换为窗口坐标。

这个接口只负责绘制所有连线共用的几何风格；不接收 Edge ID、标签、data 或选择状态。边的命中测试、重连手柄、节点自定义端口锚点、每条边独立样式和自动绕障尚未提供完整 UI 扩展接口。
