# gpui-flow

面向 Rust / GPUI 的本地创作工作台与可复用画布库。当前应用入口是
[Flow Creative](crates/flow-creative/README.md)：项目首页、无限画布、媒体对象、
类型化节点与连线，以及选中对象旁的编辑工具。界面和操作持续参考 Lovart，
当前能力与限制以实际实现为准。

私有早期开发版本，API 尚未承诺稳定。底层场景树参考 Leafer 的分层思路，
节点扩展参考 React Flow；不宣称完整 API 兼容。

## 启动工作台

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative
# 可选：指定项目保存目录
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --projects-dir ./projects
```

需要 Rust 1.98.1、对应平台的原生依赖与桌面会话。环境准备见
[入门](docs/getting-started.md)。启动进入项目首页，新建项目打开空白画布。
项目保存节点、连线、视口和导入的本地资产，退出后可重新打开。

## 当前能力

- 图片、文字、本地静态 GLB 直接放入画布；图片导入保留原比例
- 节点选择、移动、缩放、分组、复制、删除，以及事务式撤销和重做
- 类型端口连线与端口配色；Creative 使用精确类型验证
- 图片、视频、音频和模型生成节点，分别保存提示词与参数
- 本地项目和资产持久化；GLB 提供有界 CPU 静态几何预览
- 可扩展的节点定义、渲染器、records 映射与生成事件接口

默认未连接生成服务。生成节点是宿主接入入口，不会自动调用模型、收费或产生结果。
GLB 预览的格式、材质和预算范围，以及项目保存边界，见
[工作台说明](crates/flow-creative/README.md)。

## 库分层

| 模块 | 职责 |
| --- | --- |
| `flow-creative` | 当前工作台、项目模型、媒体对象与宿主扩展 |
| `flow-core` / `flow-runtime` | 图、端口、事务、历史与任务运行 |
| `flow-scene` / `flow-engine` | 显示对象、场景树、生命周期、属性与缓存 |
| `flow-layout` / `flow-animation` | 布局、时间线与属性动画 |
| `flow-editor` / `flow-interaction` | 场景编辑、选择、变换、输入与手势 |
| `flow-composite` / `flow-text` / `flow-media` | 有界 CPU 合成、文字与媒体能力 |
| `flow-ui` | GPUI 适配、画布与可复用 UI 组件 |

应用使用现有核心库。原来的 `examples/studio` 演示集合已移除；必要回归留在
库测试中，独立的 `docs/examples` headless 教程继续保留。旧演示与原生验证的
历史范围见[迁移说明](docs/studio-retirement.md)，旧命令不再是当前启动入口。

## 开发验证

```sh
cargo test --locked -p gpui-flow-creative --lib
cargo test --locked -p gpui-flow-interaction --test pan_release
cargo test --locked -p gpui-flow --lib
cargo build --locked -p gpui-flow-creative --features desktop --bin flow-creative
```

按改动选择测试；构建通过不代表原生交互或跨平台验收。完整开发规则与文档教程
检查见[本地验证](docs/testing.md)。

## 文档

- [环境准备与入门](docs/getting-started.md)
- [Flow Creative 的操作、持久化与宿主接口](crates/flow-creative/README.md)
- [架构、图模型与持久化](docs/architecture.md)
- [核心引擎](docs/engine.md)、[显示场景](docs/display-scenes.md)、[组件](docs/components.md)
- [工作流](docs/workflows.md)、[运行时](docs/runtime.md)、[自定义节点](docs/custom-nodes.md)
- [合成契约](docs/compositing.md)、[兼容性范围](docs/leafer-compatibility-profile.md)、[已知限制](docs/limitations.md)
- [本地验证](docs/testing.md)、[旧 Studio 迁移与测试保留](docs/studio-retirement.md)
