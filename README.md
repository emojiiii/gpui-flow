# gpui-flow

私有 Rust / GPUI 节点画布框架，用于构建 agent + canvas 应用。

正在开发第一版：模型和交互内核、可扩展 GPUI 节点渲染、工作流执行事件与离线演示。仓库中不会包含 API 密钥，演示不会发起付费生成请求。

## 模块

- `gpui-flow-core`：与 UI 无关的图模型、连接校验、编辑历史和视口
- `gpui-flow-runtime`：可取消 agent runner、状态和执行事件
- `gpui-flow`：GPUI 画布组件及内置节点
- `flow-studio`：原生桌面演示

## 设计参考

[React Flow](https://reactflow.dev/learn/customization/custom-nodes) 的自定义节点模式和 [Leafer UI](https://github.com/leaferjs/leafer-ui) 的可编辑画布思想；这是独立 Rust 实现，不是这些项目的移植。底层使用 [gpui-fast](https://github.com/longbridge/gpui-fast) 与兼容的 GPUI Kit 提交。

当前为开发中脚手架，运行方式、测试结果及支持边界会随实现一起补齐。
