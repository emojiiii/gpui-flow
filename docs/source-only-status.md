# 当前批次：实现范围与本地验证边界

> 历史验证记录：旧 Studio 原生演示已于 2026-10-05 移除。本文的源码路径、命令、交互矩阵与结果仅对应当时版本，不是当前可运行入口或新工作台验收。当前入口见[入门](getting-started.md)，测试保留范围见[迁移说明](studio-retirement.md)。

日期：2026-09-30。对照基线为远端
[`bde1142b027dfbd52c9d5fb7f16a9261eff5b17e`](https://github.com/emojiiii/gpui-flow/commit/bde1142b027dfbd52c9d5fb7f16a9261eff5b17e)。
本页记录源码接入，不是运行结果或完整 Leafer 兼容声明。

## 后续合成增量

[有界矢量合成](compositing.md)已追加真实tiny-skia生产者、SceneView整域替换及RenderImage上传，且完成本地自动回归和专用原生窗口交互烟测。下方原批次数字是之前的独立验证记录。

## 原批次状态

源码批次先备份为`c87c121959c617beb4ab546a7008ba8e88105238`，随后恢复Linux本地验证。
Taffy trait导入与timing lint修复已备份为`817a68f6e4b014b6897768f1ef75ee5e8545c1dd`。
562项headless测试、7项真实媒体测试、38项UI库单测和10个可执行文档通过；
workspace/all-targets check及严格clippy通过。完整命令、后续原生链接/字体探针结果
见本地验证记录（历史记录见Git历史）。CI仍暂停，真实窗口交互、
Windows/macOS和大规模性能未验；历史通过数字不能替代当前日志。

## 已接入源码的切片

| 范围 | 真实复用与项目适配边界 | 尚待验证或未覆盖 |
| --- | --- | --- |
| 普通控件 | `ControlButton`适配已锁Kit `base::Button`，保留宿主theme、callback和canvas按下隔离 | 键盘/指针/焦点及disabled原生行为尚未重新运行 |
| 图拓扑 | core/runtime显式依赖petgraph 0.8.3，内部可达性、toposort和DFS；公开Graph/ID/Serde、NodeId启动顺序及EdgeId fan-in保持项目契约 | 拓扑/重试回归通过；没有分布式或持久恢复能力 |
| 曲线bounds | Kurbo 0.11.3的QuadBez/CubicBez extrema；保留f64计算到f32边界与有限性检查 | 12项path bounds回归通过；不是所有输入上的数值等价证明 |
| 数值布局 | Taffy 0.13.0私有行内solver负责普通grow/max重分配、间距、安全对齐和stretch；Flow保留分行、reverse、内容测量、flow-text、所有权、事务及极端数值f64 fallback | 13项Taffy兼容回归及现有自动尺寸/响应布局通过；任意输入收敛及大规模性能未证；无CSS shrink、百分比或完整富排版 |
| 高级stroke | cap/join/miter/dash/offset；Kurbo dash与Lyon 1.0.22 tessellation；受预算限制的共享mesh用于高级stroke绘制/命中，旧solid-round路径保留 | 仅居中local-space；inside/outside、scale-fixed和完整任意路径命令仍缺 |
| 动画timing | 复用既有纯插值、原子batch和PlaybackToken；整序列delay/repeat delay、有限/无限遍历、正反/交替方向、正有理数speed及AnimationClip | 嵌套timeline、自动时长、逐关键帧timing、motion path、physics仍缺；见[animation-timing.md](animation-timing.md) |
| 状态扩展 | 复用SceneStateStyles/现有router及动画采样；命名palette、固定框空Text placeholder、父/祖父最近button状态继承 | 仍为fill/opacity呈现；无geometry/custom-property状态、每态transition覆盖或声明式状态序列化；见[named-states.md](named-states.md) |
| 资源生命周期 | SceneResources用实例/attachment epoch/源generation/owner票据；SceneView复用GPUI公开image asset/cache、既有text shaping，桥接实际结果和paint回执 | 媒体桥接、共享加载硬取消、byte/pixel预算及GPU呈现fence仍缺；见[resource-lifecycle.md](resource-lifecycle.md) |

没有另造图片下载器、解码器、字体引擎或完整Flexbox引擎。也没有把仅仅存在于
lockfile的库算成某crate已调用的复用。已有自动回归覆盖不替代原生窗口交互或完整行为矩阵验收。

## 公开API与序列化注意事项

- `Stroke`新增cap/join/miter/dash字段。旧JSON缺失字段使用solid round默认值；
  Rust外部struct literal需要补字段或使用`..Stroke::default()`/构造器。
  新字段会进入新序列化输出，不能把向后读取兼容说成字节不变
- `AnimationSpec`、`AnimationSequence`字段及旧入口默认保持原单次正向时序；
  新配置放在`AnimationTiming`/`AnimationClip`。`AnimationInfo.duration`仍为一次遍历，
  `elapsed`和seek表示含delay/repeat的逻辑时间；完整长度读取`timing_sample`
- `StateStyles`/`StatePaint`现有公开字段不变；命名状态/placeholder/button为运行时绑定，
  不写Scene/Undo/JSON。Placeholder改变绘制副本中的文本，不参与canonical测量或命中
- `DisplayElement`保留原有单个公开`object`字段，旧struct literal继续有效；
  `with_image`返回独立`PreparedDisplayElement`，双方共享绘制入口而不把资源状态加进旧结构
- `SceneViewEvent`、`StateStyleError`、`UnsupportedFeature`增加变体；下游穷尽match需更新。
  新resource票据/事件/状态为运行时类型，不进入场景JSON
- 本地源码中的相关构造器、struct literal、模块导出和调用点已做静态核对；
  这不证明下游代码或本项目当前源码能够通过编译

## 仍然不能宣称完成的Leafer目标

隔离组/对象透明度、混合、蒙版、擦除、阴影/模糊仍需真实离屏合成后端；当前逐paint
透明度及多层fill/stroke不等于这些功能。任意几何状态、inside/outside/scale-fixed
描边、嵌套timeline、motion path、媒体资源桥接、完整事件族/多指/IME/无障碍和
专用内外编辑工具也仍未完成。CPU dirty/damage记录不等于GPU局部绘制。

资源`Complete`仅表示当前资源集合没有Pending，包括Failed/Cancelled；
`ViewReady`来自实际GPUI paint回执而非GPU呈现fence。默认本地资源策略不是文件系统
沙箱；任何远端映射/目录授权、凭证及解码预算仍由宿主控制。

## 验证分层与后续验收重点

1. 锁定依赖的两个workspace分别进行类型/所有target检查，确认新增公开API及下游调用
2. 运行原有回归与新拓扑、path bounds、Taffy兼容、stroke、timing、named-state、resource测试
3. 验证跨切片：动画→响应布局原子失败/Undo、状态placeholder→canonical text资源、
   advanced stroke→bounds/picking/gradient/clips、source替换→迟到结果/paint receipt
4. 各桌面平台链接后执行真实按钮输入、图片授权/失败/加载、字体、状态过渡及关闭清理验收
5. 仅在有当前精确提交日志后更新通过数字；运行失败、未运行和平台未验保持分别报告

上述自动检查的实际结果见本地验证记录；第4项真实桌面交互与跨平台验收仍待执行。
