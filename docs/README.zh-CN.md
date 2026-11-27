# gpui-flow 文档

[English](README.md) · [项目说明](../README.zh-CN.md)

使用 gpui-flow SDK 构建自己的画布应用，或在 Flow Creative 中编辑本地画布项目。

## 入门与工作台

### 使用 gpui-flow SDK 开发

- [SDK 入门](library.zh-CN.md)介绍如何构建类型化节点图，以及在 GPUI 宿主中使用 `FlowCanvas` 和 `SceneView`。
- [可运行的 SDK 示例](library.zh-CN.md#在无桌面环境运行示例)涵盖图编辑、后台任务和工作流，无需桌面会话。

### 使用 Flow Creative

- [桌面安装与启动](getting-started.zh-CN.md)适用于 Windows、macOS 和 Ubuntu。
- [工作台指南](flow-creative.zh-CN.md)介绍素材排布、节点连线和画框导出。
- [完整工作台参考](../crates/flow-creative/README.md)涵盖素材格式、保存行为和宿主扩展契约。

## 库与宿主集成

| 目标 | 相关文档 |
| --- | --- |
| 在应用中嵌入节点编辑器 | [图架构与持久化](architecture.md)、[自定义节点、主题与连线](custom-nodes.md) |
| 构建保留式场景编辑器 | [显示场景](display-scenes.md)、[引擎与事件路由](engine.md)、[GPUI 组件](components.md) |
| 扩展交互与编辑行为 | [交互插件](plugins.md)、[自定义场景工具](custom-tools.md) |
| 运行任务并在节点之间传递数据 | [运行时](runtime.md)、[工作流](workflows.md) |
| 播放本地音视频 | [媒体后端与宿主配置](media.md) |

## 场景参考

- **布局与动画** · [流式布局](flow-layout.md)、[响应布局](reactive-layout.md)、[动画](animation.md)、[时间线](animation-timeline.md)
- **外观与导出** · [外观](appearance.md)、[合成](compositing.md)、[Canvas 组件](canvas-component.md)、[SVG 导出](scene-svg-export.md)、[位图导出](scene-raster-export.md)
- **形状示例** · [椭圆与弧](images/ellipse-arcs.svg)、[矩形圆角](images/rect-corners.svg)
- **文字与资源** · [文本框](text-box.md)、[资源生命周期](resource-lifecycle.md)、[场景无障碍](scene-accessibility.md)
- **Leafer 兼容性** · [支持范围](leafer-compatibility-profile.md)、[JSON 交换](leafer-json.md)、[功能列表](leafer-task-ledger.md)

## 开发与维护

- [本地验证与测试命令](testing.md) · [CI 工作流](../.github/workflows/ci.yml)
- [文档预览与发布（英文）](publishing.md)
- [已知限制与验证范围](limitations.md)
- [旧 Studio 的迁移说明与保留的回归测试](studio-retirement.md)
