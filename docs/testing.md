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
2. Ubuntu 24.04 / macOS 15 / windows-latest（MSVC）：安装平台依赖，完整 workspace check/clippy，UI组件单测，链接全部示例

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
- [ ] 内存 Save/Load snapshot 只影响图；Ctrl/Cmd + Shift + S 输出可由 Graph::from_json 解析；Ctrl/Cmd + S 保存文件
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

## 音视频、渐变与工作流增量验收

- 本地完整 workspace clippy 严格通过；五个原生示例编译链接通过
- Workflow 26 个回归测试覆盖依赖/传值/并发/失败传播/取消/多失败分支汇合重试
- Media 9 个测试包含真实 FFmpeg 解码、seek/pause、短音轨长视频、快速重复操作、子进程回收/超时/缺工具。音频设备为 SDL dummy，不证明扬声器可听
- Gradient 16 个语义/像素测试，另有明确 ignored 的性能测量；GPUI 缓存/窗口诊断测试纳入 UI 单测
- 云端 Linux 实際媒体窗口已验证：首帧、Play、Pause、seek 到 3 秒、Mute、Stop 归零；渐变 Gallery 的多色标、径向、透明硬边、曲线描边、旋转、嵌套圆角裁切可见，拖动/旋转成功
- Studio Run workflow 实際按依赖完成，文本/参考先成功、生成随后执行、输出终态成功，Inspector 可查看状态与输出
- Windows/macOS 增量结果仍以对应提交 Actions 为准；未做这些平台的真实音频设备/窗口验收

## 视觉布局与多层外观增量（2026-09-30）

- 最终纯 Rust aggregate：268 项通过（含 doctest），常规运行中忽略8项；另行运行7项实际 FFmpeg/SDL dummy 媒体测试全部通过，剩余1项为原有梯度性能测量
- GPUI library：16项通过，其中6项新增测试覆盖真实矢量 paint 准备顺序、透明层跳过、不同宽度渐变描边缓存、非均匀变换/嵌套裁切、even-odd空洞和逐paint透明度
- 可选 layout：22项集成测试和1项doctest，覆盖行列/换行/grow/约束、嵌套、身份/监听器保持、原子失败和一次撤销
- 全 workspace/all-targets check、Clippy `-D warnings`、格式检查通过；五个可执行文档示例通过
- 新 `layout_appearance` 示例已完成Linux原生编译与链接；当前批次未重新执行真实窗口/鼠标/键盘验收，不能将旧 Gallery 的交互记录视为新功能验收
- 三平台最新结果以本轮提交的Actions为准；本地上述测试不替代Windows/macOS运行验收

新增入口：

```bash
cargo run --locked -p flow-studio --bin layout_appearance
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin layout
cargo test --locked -p gpui-flow-layout
```

## KeyUp / Space 路由增量（2026-09-30）

- 最终纯Rust aggregate：280项通过；另行7项FFmpeg/SDL dummy媒体测试通过，唯一未执行项仍为原有梯度性能测量
- GPUI library：17项通过；新增native key名称规范化测试与12项键盘router回归
- 回归覆盖首按取消、once监听器后的repeat不绕过、Up强制释放、Space平移保持选择、捕获/目标/冒泡顺序、只读队列、失焦/Escape/PointerCancel、实例reset与detach/reattach、回调删除自身即时清理
- 全workspace/all-targets严格Clippy、文档Clippy、五个文档可执行示例通过
- 本批未新增窗口手工验收；三平台链接和自动测试仍以本轮提交的Actions为准

## 属性动画与安全播放历史增量（2026-09-30）

- 最终headless aggregate：319项通过（含doctest），另行7项FFmpeg/SDL dummy媒体测试通过；原有梯度性能测量仍明确ignored
- 动画专项：23项插值/时钟/实例/冲突/容量测试、15项编辑器历史会话测试、1项doctest通过
- 历史回归覆盖edit→animate→finish/cancel→Undo/Redo、原Redo分支、用户拖动中断、直接engine写入/删除、新文档epoch、外来/陈旧token与控制器丢弃后的基线恢复
- GPUI library 17项通过；完整workspace/all-targets检查与严格Clippy、六个可执行文档示例通过
- 新animations示例完成Linux原生链接（本地为节省共享磁盘只去除可重建debug段，不改源码或检查规则）；具备真实帧时钟与Finish/Cancel/Undo/Redo适配，尚未实际打开窗口验收
- 三平台最新自动结果仍以本轮提交Actions为准；编译/单测不能证明手工GUI操作和不同驱动已通过

## 内容尺寸与编辑驱动布局增量（2026-09-30）

- 最终headless aggregate：340项通过（含doctest）；另行7项FFmpeg/SDL dummy测试通过，原梯度性能测量仍ignored
- layout共44项：原22项集成+1项doctest，以及新增20项集成+1项有界收敛单测
- 新回归覆盖行列hug、padding/min/max、空容器、固定主轴wrap、auto主轴禁wrap、尺寸所有权拒绝、嵌套测量/父级分配、非整数幂等、custom归一化后bounds/hit/布局固定点、输入+布局单步Undo、失败原子性
- 独立审查后补活跃动画→布局的分离Undo/Redo、stale tick/cancel保护、非法布局保留播放与原Redo分支；所有新增回归通过
- 全workspace/all-targets check、严格Clippy、文档Clippy、格式检查通过；七个可执行文档示例通过
- GPUI library17项通过；新auto_size示例完成Linux原生链接，尚未实际打开窗口操作验收
- 三平台自动结果以本轮提交Actions为准；该增量实现显式编辑事务驱动reflow，没有全局observer或文字内在测量

## Paint-only状态样式增量（2026-09-30）

- 最终headless aggregate：360项通过（含doctest），另行7项FFmpeg/SDL dummy通过；原有梯度性能测量仍ignored
- 新20项状态回归覆盖字段优先级/disabled屏蔽、祖先hover/press与精确focus、prevented Down/Up、Leave/重入/拖动capture/外部取消、当前文档基线、Redo与播放token保持、卸载重挂载epoch、同ID重建、注册失败与paint上限冲突
- GPUI library18项通过，新增真实呈现准备路径回归证明状态paint溢出报错时仍画canonical对象、移除绑定后清错；不是以空白画面吞掉错误
- 完整workspace/all-targets check、严格Clippy、格式检查、文档Clippy及8个可执行文档示例通过
- state_styles原生示例完成Linux编译链接；本批未进行真实窗口/鼠标/键盘验收。Windows/macOS自动链接与测试以本轮Actions结果为准
- 本轮仅fill/opacity状态呈现，不包括自动transition、命名state、placeholder、button继承、geometry状态、状态驱动命中或大场景增量性能保证

## 同步响应布局增量（2026-09-30）

- 最终headless aggregate：381项通过（含doctest），另行7项FFmpeg/SDL dummy通过；唯一未执行的既有项仍为梯度性能测量
- 新增8项engine provider防护测试及13项reactive flow集成测试，覆盖提交前一次通知/Undo、插入删除/reorder/reparent、挂载/销毁/ID复用、runtime配置与精确Undo、paint/metadata跳过、循环/递归/panic失败原子性
- 独立审查发现同tag shape篡改风险后收紧为显式size/transform白名单；回归覆盖13类Text/Image/Frame/Polygon/Path非法字段修改、合法Text/Image尺寸修改且保留内容/资源
- 动画回归包含布局拥有的transform拒绝、custom geometry自动更新父尺寸、中间帧definition panic保持所有数据/时钟/播放token，以及失败普通编辑保留原Redo/播放基线
- 全workspace/all-targets check、严格Clippy、格式检查、文档Clippy、9个可执行文档示例及18项UI测试通过
- reactive_layout完成Linux原生链接，控件调用普通SceneEditor::transaction，由已安装provider自动重排；本批未实际打开窗口验收。三平台自动结果以精确提交的Actions为准
- 没有宣称异步全局watcher、帧合并、局部依赖调度、序列化runtime配置或完整input/computed分离

## 状态 fill/opacity 过渡增量（2026-09-30）

- `flow-interaction/tests/state_transitions.rs`：14项，覆盖进出时间/exit回退与显式零时长、RGBA/opacity/easing、连续快速反转、同目标不重启、disabled/PointerCancel、同通道canonical编辑/animation冲突、Undo/Redo、detach/reattach epoch、reset、binding替换/解绑、整帧失败原子、单调时钟、极大时长和配置变更finish
- 复用现有animation纯插值器；`TweenValue::interpolate`验证有限输入与匹配类型，精确端点，无引擎写入
- 395项headless aggregate通过（含doctest）；原有梯度性能测量仍ignored
- 19项UI单测通过，包括实际RenderObject中间色/opacity、endpoint后active=false；workspace所有target check和strict clippy通过
- 现有`state_styles`原生示例启用180ms进入/320ms退出；本地链接和headless行为验证，不声称人工GUI验收
- executable `state_styles`文档验证进出/反转且Undo不被污染；发布后的三平台结果仍以精确commit的Actions为准

## 显式关键帧序列增量（2026-09-30）

- `flow-animation/tests/sequences.rs`：13项，覆盖累计稀疏字段/精确边界双向seek、逐段easing、大步tick与分步等价、一次多目标通知、pause/resume/完成后回退、晚段非法端点原子拒绝、64段/32属性/累计时长溢出、极大时长、未来属性归属与外部覆盖、mount epoch、编辑器finish/cancel/Undo/Redo、晚段custom几何失败的文档和时钟回滚、JSON严格解析
- `flow-layout/tests/reactive.rs`：新增“首段只有paint、晚段抢占layout-owned transform”整序列注册拒绝回归
- 409项headless aggregate通过（含doctest），状态过渡14项单独复验通过；9个可执行文档通过，animation例实际验证跨段seek和cancel保留旧Redo
- animation/layout所有target严格clippy与文档严格clippy通过；共享缓存清理后workspace/all-targets原生check通过（缺失build输出自动恢复），无需全面clean
- 现有`animations`原生示例已改成真实两段、总2秒序列；本批本地为编译验证，新的原生链接/UI及媒体完整回归由精确提交CI执行。不声称人工GUI验收
