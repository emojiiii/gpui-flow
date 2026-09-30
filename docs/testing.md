# 测试、CI 与人工验收

## 原则：区分检查层级

- `cargo check` 验证类型与编译路径，**不验证最终链接或窗口运行**
- `cargo build -p flow-studio` 验证 native 链接，**不验证鼠标、键盘、输入法或画面**
- core/runtime 测试验证模型与任务语义，**不验证 GPUI 交互**
- 人工桌面验收需要真实窗口、图形后端和输入设备

CI 文件存在不等于 CI 已执行或通过。以目标提交的 Actions 日志为准，不要把过往成功结果当作当前分支状态。

## 本地自动检查

使用环境准备中的 Rust 和 native 依赖，在仓库根目录执行：

```bash
cargo fmt --all -- --check
cargo test --locked -p gpui-flow-core -p gpui-flow-runtime -p gpui-flow-scene -p gpui-flow-editor
cargo clippy --locked -p gpui-flow-core -p gpui-flow-runtime -p gpui-flow-scene -p gpui-flow-editor --all-targets -- -D warnings
cargo check --locked --workspace --all-targets
cargo clippy --locked --workspace --all-targets -- -D warnings
cargo build --locked -p flow-studio
```

完整构建会拉取、编译 GPUI 平台依赖，需要更多磁盘与内存。资源有限时可先执行前 3 条，随后在目标桌面补 native 检查；不要把前 3 条通过称为全量通过。

文档示例是独立的小 workspace，自己的 lockfile 也提交到仓库：

```bash
cargo fmt --manifest-path docs/examples/Cargo.toml --package gpui-flow-doc-examples -- --check
cargo clippy --locked --manifest-path docs/examples/Cargo.toml --all-targets -- -D warnings
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin core
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin runner
```

示例中有断言：core 验证连接拒绝、拖动事务合并、撤销/重做、片段复制、分组和含视口文档 round-trip；runner 验证离线字符串转换、输出和终态重试。它们不只是打印 API 名称。Markdown 中的宿主/自定义 renderer 片段是组合示例，当前没有自动提取成 GPUI 编译测试。

## 现有测试关注点

### core

见 [`crates/flow-core/tests/core.rs`](../crates/flow-core/tests/core.rs)：

- ID 序列化、确定性 JSON 与 Graph/FlowDocument round-trip
- 方向、类型、Any/Named、容量、重复、缺失、自连接和可选 DAG 校验
- NaN/无穷/非正几何、版本不兼容、ID 不一致和无效导入
- 事务失败/校验失败/panic 的原子性
- 一次手势合并多次移动、取消还原、redo 保留/失效和历史上限
- 选中锁定节点时的原子回滚、节点缩放和删除引用清理
- 分组重归属、组/成员共同移动不重复、删组保留节点
- 剪贴板 ID/边/组重映射，非法片段与粘贴溢出回滚
- 视口坐标往返、锚点缩放、fit、minimap 和逆方向框选

[`persistence.rs`](../crates/flow-core/src/persistence.rs) 另外验证文件 round-trip/覆盖、无效保存不改变已有文件、无效加载和 32 MiB 上限。

### runtime

见 [`crates/flow-runtime/src/lib.rs`](../crates/flow-runtime/src/lib.rs) 的 tests 模块：成功、失败、合作式取消、运行中拒绝重试和终态后 attempt 递增。新增供应商适配器至少还应验证自己的超时、认证错误脱敏、远端取消、重复事件、重试幂等性和资源清理。

不要在 CI 使用真实付费供应商来证明这些逻辑；使用可控制的 fake/mock，并给等待断言设置超时，避免永久挂起。

## GitHub Actions

[`.github/workflows/ci.yml`](../.github/workflows/ci.yml) 包含：

1. Ubuntu 24.04：主 workspace 与文档示例格式检查、core/runtime/scene/editor 测试及 clippy、执行文档示例
2. Ubuntu 24.04 / macOS 15 / windows-latest（MSVC）：安装平台依赖，完整 workspace check/clippy，UI组件单测，链接全部三个示例

CI 使用 Rust 1.98.1 和 `--locked`，只授予 `contents: read`，不需要供应商密钥，不发布 crate、部署应用或上传用户图。并发工作流会取消同一分支的旧 run。native job 不是图形 E2E 测试。

如 GPUI 依赖更新，请同时核对：

- `Cargo.toml` 的来源与版本约束
- `Cargo.lock` 中 gpui-fast/gpui-kit 的真实 SHA
- 上游工具链与 Linux native deps
- Linux 和 macOS 完整编译与真实窗口验收
- 文档中的 API、字段、行为和限制

不要通过删除 `--locked`、忽略 native job 或隐藏警告让 CI 变绿。

## 人工桌面验收清单

以下是应该逐项执行的验收步骤，**不是已经通过的测试结果**。记录系统版本、GPU/图形后端、目标提交、截图和复现步骤。

### 画布与几何

- [ ] 启动显示初始图、侧栏、Inspector、状态栏，文字无明显裁剪
- [ ] 平移、鼠标锚点缩放、100% 重置、Fit，节点与端口/边保持对齐
- [ ] 窗口缩放、非 100% DPI 后重复操作，画布原点不漂移
- [ ] 多选移动、反方向框选、缩放节点，单次 undo 恢复整个手势
- [ ] 移动/缩放期间 Esc 撤销手势；鼠标释放在画布外不留下拖动状态
- [ ] 选择、快捷键和 Inspector 输入焦点切换不会误删节点或触发全局操作

### 内容、连线和历史

- [ ] Inspector 分别编辑 text、prompt、source，节点和图 JSON 更新
- [ ] 输入后撤销/重做、同节点快照恢复，输入框与 Graph 保持一致
- [ ] 测试合法连线、同向端口、类型不匹配、占满输入、重复连线、Esc 取消
- [ ] 多节点复制粘贴后 ID 唯一、内部边保持、外部边不复制，撤销可恢复
- [ ] Group selection 与删除组符合 core 语义，不意外删成员
- [ ] 添加后撤销，再编辑会清掉 redo，连续操作没有部分修改

### 持久化与媒体

- [ ] Save file/Open 对话框成功、取消、不可写目录、无效/超大文件；有效文件恢复 viewport
- [ ] 打开文件清空历史；保存期间继续编辑时明确保存的是点击时快照
- [ ] 内存 Save/Load snapshot 只影响图；Ctrl/Cmd + S 输出可由 Graph::from_json 解析
- [ ] 关闭重开不误以为内存快照是自动保存
- [ ] 宿主 FlowDocument 保存/恢复含 viewport；非法版本与缺失引用被拒绝
- [ ] 有效本地图片路径、空路径、不存在路径和不支持的文件类型不破坏界面
- [ ] 音视频呈现清楚的占位/元数据行为，没有暗示播放或真实生成

### 任务与中断

- [ ] 自定义 kind 注册 fake runner 后 Run 确实调用该实现；未注册 kind 不运行
- [ ] FlowCanvas::new 不注册 runner，demo 的两个 kind 使用 Mock
- [ ] Mock 成功、取消、终态重试；运行中反复点击 Run 不重复启动同节点任务
- [ ] 取消后直到 runner 退出再进入终态；重复取消不误显示完成
- [ ] 任务运行中删除节点/导入新图/关闭窗口，记录宿主需要的生命周期策略
- [ ] 使用故意失败或 panic 的 fake runner，界面保持可用且显示安全错误
- [ ] 真供应商只在另行授权的集成环境测试，核对费用和远端取消状态

## 后续测试建设建议

优先补充 GPUI 事件驱动交互测试与截图基准、runtime 取消/旧事件/线程退出的确定性测试、属性/模糊测试覆盖大量图变更序列，以及大图性能基准。新增测试应描述可证明的行为，不以“可渲染”代替全部功能完成。


## 2026-09-30 已执行的本地验收

独立于上面的待测清单，以下有实际执行结果：

- 纯 Rust：core 39、scene 31、editor 10、runtime 7 项测试通过；另 2 项 doctest 通过
- GPUI library：5 项组件/裁切测试通过；全部 workspace/all-targets check 与 Clippy `-D warnings` 通过
- 三个示例均完成 Linux 链接并在 dot 云桌面真实打开
- Gallery：实际显示实色原语、PNG、路径、嵌套圆角裁切、继承透明度；拖动、Undo、旋转及子元素→Frame 捕获事件通过
- Composition：独立 NodeFrame 选中回调、Handle 请求计数、宿主状态重置、canvas-only 嵌入与 Fit 通过
- Studio：Fit、选择、MOCK 到 100% 终态通过；指定路径模式 Save 后添加第 8 节点，再 Open 恢复 7 节点；磁盘 JSON 含 viewport；损坏 JSON 导入报错且保持原图
- 该 Linux 桌面缺少 `org.freedesktop.portal.Desktop`，默认原生文件选择器显示错误；因此使用并验证了 `--file PATH`。未宣称默认原生对话框成功

未执行的内容仍包括 Windows 原生运行、macOS 实际窗口操作、真实供应商/媒体解码、输入法矩阵、大规模性能、完整所有交互路径。GitHub CI 的最终提交结果以 Actions 页面为准。
