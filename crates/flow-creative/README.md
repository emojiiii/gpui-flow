# Flow Creative

一个独立、可复用的原生创作画布 crate，复用 `FlowCanvas`、`Graph`、端口校验与
`Editor`，不另造画布引擎。交互参考 Lovart 的
[对象](https://www.lovart.ai/docs/edit-your-design/canvas-objects)与
[画布导航](https://www.lovart.ai/docs/edit-your-design/customize-your-canvas)；
类型端口和数据映射是本项目自己的扩展，不声称复刻 Lovart 的内部模型。

## 运行

```sh
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative
# 无桌面文件选择器时，保存/打开使用明确路径；不会自动读写：
cargo run --locked -p gpui-flow-creative --features desktop --bin flow-creative -- --file ./board.flow.json
```

沿用仓库的 Rust/GPUI/native prerequisites。默认特性不依赖 GPUI；模型可以独立运行：

```sh
cargo test --locked -p gpui-flow-creative --lib
```

- 左侧添加图片、文字、创作方案、自定义色板卡片；点击图层或画布选择
- 拖动卡片，右下角调整尺寸；Shift 多选；Space/中键平移，Ctrl/Cmd+滚轮缩放
- 从输出端口拖向同类型输入；例如图片→图像可连接，图片→文字会拒绝
- 选卡片后在右侧编辑内容，应用才写文档；放弃修改不写历史
- Frame 使用真实 Graph Group：显式边界、成员、移动、解组与持久化；不提供嵌套裁切
- 图形编辑/连接/删除使用原有事务与 Undo/Redo；删除卡片同步删除相关连线
- 保存沿用 Flow 的原子文件写入。打开成功会替换文档并清空历史，读取期间有修改时会保留当前画布并提示冲突
- 「数据 / JSON」可导出文档，或加载 records 模板再导入。非法导入整体失败，原画布保留
- 数据导入的图内容可一步撤销；视口属于导航状态，不进入图 Undo。文件不存选择/历史
- 连线表示结构/类型关系，不自动运行模型或重生成图像
- 初始方案和两张 SVG 图片是本地示例，不会调用生成模型、上传素材或产生 API 费用

## 明确的数据契约

持久化直接采用已有 `FlowDocument`：Graph schema v1 保存稳定节点/端口/边/Frame ID，
位置、尺寸、自定义 JSON data、锁定状态以及 viewport。图片只保存 asset key；
`CreativeAssets::register` 由宿主显式授权本地文件。数据不会自动转换为 URL/文件读取。

records v1 是另一条显式映射入口，`CanvasRecords` 包含 cards、links、frames、metadata。
每张卡片必须指定注册的 kind 和稳定 ID；端口从 `NodeDefinition` 生成，不从任意字段推断。
`RequirePositions` 要求坐标；`Grid { columns }` 仅为缺坐标卡片确定性排位，保留显式坐标。
它不是图语义推理或自动美化布局，不避让宿主已有显式位置。

`CreativeRegistry` 校验标准/自定义 payload 和完整端口定义；拒绝 `DataType::Any`、
未注册 kind、错误方向/类型/引用/容量。`CreativeGuard` 在实际命令路径校验 Add、Data、
Paste、Import 和 Load，预览/最终连线仍共用原 Graph 规则。宿主文档验证钩子在取消现有手势前校验文件/JSON导入，保存也验证实际捕获的快照。旧 core 的 Any 通配行为未变。
默认容量为 128 卡片、512 连线，单卡 payload 上限为 64 KiB。CreativeCanvas 在提交图和视口变更前
按实际保存的 JSON 格式检查 32 MiB 文档上限。独立组合 `CreativeGuard` 和 `Editor` 的宿主可调用
`Editor::set_document_byte_limit(MAX_JSON_BYTES)` 启用同一上限。

这是可信宿主扩展 API，不是安全沙箱：直接改公开 Editor 或安装恶意命令 middleware
不受本层保护。注册回调与 renderer 不写入 JSON，接收文档的宿主必须注册相同类型契约。

## 自定义节点

先用 `NodeDefinition::new` 注册 kind、标题、默认尺寸、精确端口、默认数据和验证函数，
再用已有 `gpui_flow::NodeRegistry::register` 绑定自己的 `NodeRenderer`。
`CreativeCanvas::new` 接收两份 registry 与已校验的 `FlowDocument`。
本例 Palette 正是通过这两个公开入口注册的自定义 payload/Named("palette") 端口/renderer。
未知自定义字段不会被偷偷转换成画布行为；自定义输入编辑器可由宿主替换右侧 shell。

Leafer 的显式兼容选项见 [compatibility profile](../../docs/leafer-compatibility-profile.md)。
它作用于 retained SceneView；本 crate 是复用 Graph 的卡片编辑器，不把两套对象模型混为一谈。

## 已验证

Creative 的 11 个模型/命令/表单回归、宿主文档校验回归及桌面 strict/link 已通过。
Linux 原生窗口实际检查了同型连接/异型拒绝、拖动缩放、内容编辑与 Undo/Redo、
Frame 建组搬运、保存再打开、非法 JSON 原子拒绝、records 导入及一步撤销、只读编辑保护。
这些检查不等于跨平台验证；Frame 仍为视觉分组，连线仍不执行 AI 工作流。
