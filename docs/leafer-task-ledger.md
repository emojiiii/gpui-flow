# Leafer 对齐任务总表 v1（审阅版）

固定45个去重能力单元，最新已验证远端源码 [`761b626b`](https://github.com/emojiiii/gpui-flow/commit/761b626b5c9c77de1c2fdf5fe4c3751114ffb1e7)，原生夹具工作区来源逐项单列。这是具体范围的开发台账，不是全部Leafer API兼容证明，不用任务数计算工时或百分比。旧39条A–E验收契约仅为交叉索引，不与本表相加；此前31/8快速审计不作为本表完成数。

## 2026-10-03 云环境更换与证据位置

08:51 UTC左右原云host被替换，679份已发布源码/资产已按精确tree恢复；旧raw QA原图/trace/binary目录不可访问，历史报告/哈希不等于raw已恢复。[恢复说明](cloud-workspace-recovery-2026-10-03.md)区分旧历史结果、新host31项聚焦/115Pillow复验与未完成的0844Z原生。0927Z另起完整验收并将新真实证据按partial/complete版本存入私有Library；当前计数不因恢复动作改变。

## 状态计数

- 有界契约已验证：33
- 部分实现：9
- 未开始该能力：3
- 待核实验收：0
- 开发/验收中：0

有界已验证仅表示本行支持集合有实际证据，不表示整个上游模块完全对齐。源码、自动检查、真实窗口分别记录。未开始指本项新增能力，不以已有依赖代码冒充实现。目标是继续完成表内未闭环行为；技术依赖不因此被删除，也不能以“今晚”为由虚报通过。

ID与有限验收集合由本表定义，状态随证据更新。扩大范围须留下变更记录，新增行为另增ID并明确变更总数，不拆小方法美化进度。表内标出的Flow扩展不是固定OSS源码已有实现。跨平台验收、闭源插件、浏览器DOM控件和真实Agent供应商不计本轮Linux画布对齐范围。

## 截图格式勘误

[2026-10-02格式勘误](screenshot-evidence-formats.md)：327张历史桌面截图实际为JPEG，保留原`.png`文件名、原始QA报告与哈希。零差ROI、颜色及阈值边界只描述解码后的有损截图，不证明Canvas/GPU无损像素一致。APP-02辅助20/21及失败原始记录保留，低对比样本成因未定，JPEG可能影响。SVG实际导出2张PNG及24张内嵌PNG独立有效；trace、源码/二进制、退出与窗口观察证据分别保留。状态、冻结验收及31/11/3计数不变。

## 固定参考

UI `1e11641d3ea6b0211c83447268a90b8d58feec25`；core `1d703303ff0759ce4973d54c87eeb8192dddc570`；plugins `3eb445bf34debb638766615282224b77108bf610`。每项具体来源见下表，接口和实现不一致时以后者为准，stub不构成已实现行为。字词段间距等Text集合来自固定ITextStyleInputData，未把未证实的rich runs作为Leafer必选项。

## 去重任务与验收

| ID | 类别/能力 | 状态 | 固定验收集合 | 边界/排除 |
|---|---|---|---|---|
| DAT-01 | 数据 / [元素工厂与属性schema](engine.md) | 有界契约已验证 | 内置及Custom注册、默认值/版本/验证；非法输入原子拒绝 | 运行时回调不序列化；不含Leafer格式导入 |
| DAT-02 | 数据 / [原子属性与历史](engine.md) | 有界契约已验证 | 有类型setter、批量首旧末新、一次revision及UndoRedo，失败无部分修改 | 直接可信宿主底层修改不是权限边界 |
| DAT-03 | 数据 / [版本化文档与有限Leafer映射](display-scenes.md) | 部分实现 | 必选现有Scene JSON v1读写/未知版本拒绝；固定原语tag、transform、基础paint的Leafer JSON映射，未知属性明确拒绝并往返 | 不含JS回调、任意computed缓存或完整上游JSON兼容；迁移仅在存在正式v2时定义 |
| DAT-04 | 数据 / [查询与增量缓存性能](query-indexes.md) | 有界契约已验证 | 固定id/name/tag/class/子树条件与顺序；随机仿射+重挂载对照、固定fixture重算计数及CPU benchmark | 无通用CSS选择器语言；不声称GPU局部绘制 |
| INS-01 | 实例 / [代际身份与树操作](engine.md) | 有界契约已验证 | create/clone/detach/reattach/reparent/destroy保留或更换身份有明确定义；奇异/循环原子拒绝 | 不是多App分层组合 |
| INS-02 | 实例 / [多层App实例组合](layered-app.md) | 有界契约已验证 | 多层画布树、跨层事件/坐标转发、独立生命周期与销毁 | 多个普通SceneView并排不等于App分层 |
| LIF-01 | 生命周期 / [订阅与拥有资源销毁](resource-lifecycle.md) | 有界契约已验证 | once/off/scoped disposal、destroy exactly once、旧token和迟到job不重定向 | 不包含后台加载器硬取消 |
| LIF-02 | 生命周期 / [资源就绪与呈现回执](resource-lifecycle-validation.md) | 有界契约已验证 | Ready/ViewReady/Complete含失败取消，真实加载/替换/销毁/paint事件顺序 | ViewReady是paint回执，不是GPU fence |
| LIF-03 | 生命周期 / [加载取消与媒体资源桥](resource-lifecycle.md) | 部分实现 | 共享加载字节预算、无引用任务硬取消、媒体统一代际票据和错误传播 | 现有GPUI image及有界本地媒体路径不是完整资源服务 |
| EVT-01 | 事件 / [树路由与默认动作顺序](focus-keyboard-validation.md) | 有界契约已验证 | capture/target/bubble、once/off、监听队列和prevent-default；原生适配器trace与纯路由一致 | 限定现有鼠标/默认动作与精确trace；设备扩展归EVT-03，不声称全浏览器事件兼容 |
| EVT-02 | 事件 / [键盘与程序焦点](focus-keyboard-validation.md) | 有界契约已验证 | KeyDown/Up、重复/Space、Focus/Blur重定向、窗口失焦清理及实例隔离 | 不把Kit文本输入当完整IME/a11y |
| EVT-03 | 事件 / [设备元数据与复合手势](keyboard-routing.md) | 部分实现 | pointer type/buttons/time/pressure、触摸多指、tap/double/longpress的稳定事件契约 | 已有mouse/pointer_id、序号与原生Text双击；不是完整事件族 |
| EVT-04 | 事件 / [IME与无障碍输入](text-editing.md) | 部分实现 | 中文输入组合/切换/失焦、Tab可达与语义树，定义受支持平台验收 | Kit Textarea提供基础；平台IME边界明确未验 |
| INT-01 | 交互 / [外部选择移动缩放旋转](text-transform-tools.md) | 有界契约已验证 | 单选/共享root、多选、Image和能力开启Text的仿射手柄；撤销/取消/只读/陈旧capture | Custom及未支持样式拒绝；不改intrinsic尺寸 |
| INT-02 | 交互 / [内在尺寸编辑](text-font-resize.md) | 部分实现 | 按组件定义尺寸/内容/字体resize策略，与布局所有权及单Undo一致 | 已支持固定Text盒子和字体大小模式；其他组件intrinsic resize和auto-size ownership覆盖仍缺，整项仍部分 |
| INT-03 | 交互 / [Text内容编辑会话](text-editing.md) | 有界契约已验证 | 可选内容panel、一次文档Undo、Done/Cancel/blur/target切换、身份和只读保护 | 不是WYSIWYG；IME专项归EVT-04 |
| INT-04 | 交互 / [自定义内外工具协议](text-editing.md) | 部分实现 | registry、load/update/unload/destroy、策略门控与宿主自定义widget挂载和释放 | 现有controller/registry与Text专用面板；任意内层widget不足 |
| INT-05 | 交互 / [路径与倾斜中心编辑工具](line-endpoints.md) | 未开始该能力 | Path控制点、skew和pivot交互须各有真实几何/取消/history验收 | 固定Leafer OSS通用点编辑器是stub，不把自建功能称上游已实现；Line端点现有独立基础 |
| INT-06 | 交互 / [拖动约束与高级视口](drag-viewport-constraints.md) | 有界契约已验证 | 固定世界/父局部矩形drag bounds、可开关网格/边缘吸附、viewport min/max及惯性取消 | 固定AABB及有限惯性；多指归EVT-03，无自动寻路/物理碰撞；持鼠/guide物理覆盖与6文件来源限制单列 |
| APP-01 | 外观 / [有序填充与渐变](appearance.md) | 有界契约已验证 | 多层矢量fill、实色/线性/径向、多stop、透明度与缓存/DPR预算 | Text/Image多paint不在此项支持子集 |
| APP-02 | 外观 / [高级描边](stroke-alignment.md) | 有界契约已验证 | cap/join/miter/dash、闭合及开放轮廓内外对齐、scale-fixed；bounds/hit/paint同源 | 有界共享mesh支持cap/join/miter/dash、闭合/开放对齐及每层scale-fixed；局部shear、模糊Y阈值、部分consumer明确拒绝，Canvas命令仍center-only；无全格式/位等价承诺 |
| APP-03 | 外观 / [隔离混合与蒙版擦除](scene-erasers.md) | 部分实现 | 组透明度/多paint隔离、固定blend集合、mask/erase覆盖关系与hit规则 | Pixel及几何leaf Path、直接父隔离与固定alpha-mask相位已实现；Custom/外部backdrop、Path容器/Text等及完整mask布局/命中兼容仍缺 |
| APP-04 | 外观 / [阴影与固定滤镜集合](multiple-shadows.md) | 部分实现 | 已有leaf/group inner/outer多影；必补spread/box/scale-fixed shadow、blur、backgroundBlur及grayscale并有像素/预算测试 | backgroundBlur仅可在拥有完整背景的有界surface执行；不承诺任意宿主backdrop |
| APP-05 | 外观 / [状态样式与过渡](state-geometry.md) | 有界契约已验证 | hover/press/focus/selected/disabled、命名状态/继承、每态transition及geometry变化 | 固定实色/opacity、每态timing、x/y及六种size；任意属性/rotation/scale/skew等为边界；hard invalidation loading与canonical event-local坐标保留 |
| APP-06 | 外观 / [GPU局部绘制后端](engine.md) | 未开始该能力 | 后端真正消费persistent damage并有像素等价/工作量对照 | CPU dirty和damage记录已归DAT-04；GPUI当前full-frame |
| LAY-01 | 布局 / [固定与内容流式布局](flow-layout.md) | 有界契约已验证 | row/column、wrap/reverse、padding/gap/grow/minmax、auto-size/嵌套固定点/hidden排除 | 百分比/baseline/shrinkContent归LAY-03；不是CSS完整实现 |
| LAY-02 | 布局 / [响应派生与事务所有权](reactive-layout.md) | 有界契约已验证 | 安装binding后普通编辑提交前派生、白名单/有界固定点/冲突拒绝、一次Undo | 不是全局observer/帧合并局部依赖调度 |
| LAY-03 | 布局 / [高级尺寸与局部调度](advanced-flow-layout.md) | 有界契约已验证 | 固定父尺寸百分比、shrinkContent、文字baseline行对齐、依赖dirty子树与单帧合并；循环/所有权冲突原子拒绝 | 受父尺寸确定性约束；不实现全CSS布局或无界observer网络 |
| LAY-04 | 布局 / [同源文字测量与重排](text-measurement.md) | 有界契约已验证 | 同GPUI shaping的Natural/AutoHeight、字体metrics失效、真实测量推动布局 | 富排版的字词段间距归ELM-04 |
| ELM-01 | 元素 / [矢量原语与实际几何](ellipse-arcs.md) | 有界契约已验证 | Rect/Ellipse/Line/Polygon/Star/Path/Pen各独立模块；四角/弧环/数据API贯通bounds/hit/CPU | 不包含通用点编辑器或完整SVG文档 |
| ELM-02 | 元素 / [Path距离查询与显式定位](path-placement.md) | 有界契约已验证 | Kurbo局部弧长/切线、世界点、显式目标定位；自依赖/祖先/奇异/陈旧拒绝 | 持续动画绑定归ANI-02 |
| ELM-03 | 元素 / [Group Box Frame容器裁切](container-corners.md) | 有界契约已验证 | 分离组件/paint phases/四角、嵌套真实clip，bounds/hit/CPU/native一致受能力限制 | 不扩大Custom/特殊effect能力 |
| ELM-04 | 元素 / [Text固定排版属性](text-case-paragraph-spacing.md) | 部分实现 | 已有font/wrap/align/padding/ellipsis/italic/decoration/affine；必补letterSpacing、wordSpacing、paraIndent、paraSpacing、textCase、writingMode和autoSizeAlign | 富runs/WYSIWYG不是此次固定ITextStyleInputData属性集，作为额外范围另立项 |
| ELM-05 | 元素 / [Image真实帧与几何呈现](image-corners.md) | 有界契约已验证 | 静态/动画帧、fit、四角及仿射/复杂clip，真实CPU像素和资源代际 | alpha hit/crop内编辑归ELM-06；解码与URL权限归宿主 |
| ELM-06 | 元素 / [Image像素命中与内编辑](image-crop.md) | 有界契约已验证 | decoded-alpha hit阈值/缩放/当前帧和crop内工具必须一致且可撤销 | OSS未找到Image专属crop inner editor；属于框架明确扩展，不虚称上游已有 |
| ELM-07 | 元素 / [Canvas拥有像素与生命周期](canvas-component.md) | 有界契约已验证 | 独立配置、运行时surface、DPR/smooth/safe-resize/clear/draw/snapshot、代际释放 | 像素不进文档/Undo；非整视图OwnedCanvas |
| ELM-08 | 元素 / [Canvas矢量上下文批次](canvas-clips.md) | 有界契约已验证 | Path填充/描边、transform/alpha/save/restore/冻结clip栈；晚失败原子回滚 | 绘制状态仍batch-local；持久current path归ELM-10，图像归ELM-09 |
| ELM-09 | 元素 / [Canvas图像批次](canvas-image-commands.md) | 有界契约已验证 | immutable image输入、source crop/destination/DPR、仿射/clip/alpha和单代际原子提交 | 不取URL；负尺寸拒绝；不保证浏览器精确filter边缘 |
| ELM-10 | 元素 / [Canvas文字/路径/图案与真实描边](canvas-stroke-text-native.md) | 有界契约已验证 | 必选fillText、measureText、strokeText；beginPath/moveTo/lineTo/quadraticCurveTo/bezierCurveTo/closePath；createPattern的repeat/no-repeat与transform | 原DOM/CSS字体parser及完整browser状态排除不变；stroke限同App/host注册有界静态字体、实色top-left盒，保留Linux/mark/pattern截图与原失败边界 |
| ELM-11 | 元素 / [Canvas与场景导出](scene-raster-export.md) | 有界契约已验证 | Canvas RGBA/PNG已实现；固定Scene区域PNG/JPEG/WebP和SVG，含背景/透明度/裁切范围及缺资源拒绝 | WebP为lossless；SVG保留vector并嵌每节点Image/Canvas/Text PNG，无editable text/outlines或SVG/PNG位等价；不含完整SVG导入 |
| CUS-01 | 自定义元素 / [动态几何命中与宿主呈现](custom-geometry-validation.md) | 有界契约已验证 | custom属性驱动几何/bounds/hit、显式native renderer、实例隔离；拒绝不支持合成 | schema本身归DAT-01；不双计工厂 |
| CUS-02 | 自定义元素 / [自定义resize和异步资源协议](custom-nodes.md) | 未开始该能力 | 自定义尺寸/编辑策略与测量、异步ready/cancel事件正式接入 | 任意宿主回调能实现不等于框架已提供协议 |
| ANI-01 | 交互插件 / [有界属性动画时序](animation-timing.md) | 有界契约已验证 | 稀疏关键帧、delay/repeat/direction/speed、seek/pause/conflict/cancel和单Undo | 不是嵌套timeline；motion binding归ANI-02 |
| ANI-02 | 交互插件 / [持续运动路径动画](motion-path-animation.md) | 有界契约已验证 | 按时钟距离/切线绑定位置/旋转，变化失效/冲突/销毁/取消/history一致 | source/祖先变化保守取消，不自动refollow；无路径生长/文字沿路径/自动timer；完成后显式finish_editor封Undo |
| ANI-03 | 交互插件 / [组合动画时钟与定位控制](animation-timeline.md) | 有界契约已验证 | 必选AnimateList式并行与串行子动画、offset、父时钟seek/pause/reverse、结束样式及一次Undo | jump/join、physics、无限嵌套和跨所有跳帧回调不属于v1有限集合，新增须显式扩项 |

## 自动化、原生与版本证据

所有行的源码包含版本在页首，**不是该SHA整个工程重新跑过全部测试**。历史全套证据WS-20261002的[原日志](validation/2026-10-02-workspace-tests.log)为1215通过、0失败、8原有忽略，命令为`cargo test --locked --workspace --lib --tests`；测试树`3b237095928057d2e00e897a9a3822f8117d088e`与已回读验证的远端`a08670a2fbe2882997d21d6a9f812d4197d3da74`仅一份文档不同，代码/构建/测试输入完全相同。日志与源快照hash见JSON。此记录不含真实窗口，也不冒充后续变更未跑过的全套结果。

| ID | 自动化证据 | 原生证据 | 证据提交 |
|---|---|---|---|
| DAT-01 | engine/registry、engine及metadata测试已在10月2日全套记录通过 | 纯引擎项，不要求窗口 | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| DAT-02 | engine/engine及editor历史测试在全套记录通过 | 纯引擎项，不要求窗口 | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| DAT-03 | Scene v1与12tag严格Leafer映射；24 focused+scene strict通过，未知字段/预算/原子拒绝 | 纯数据；不承诺浏览器/native像素等价 | 本批有限子集，input/computed等更广边界未闭环，仍部分 |
| DAT-04 | 154项engine通过/2 benchmark常规忽略；两个benchmark均显式运行通过。300步query与480步cache参考对照；等工作量索引维护中位开销+1.25/+2.23/+3.40%，无整体提速声明 | 纯数据项，不要求窗口 | `4049da69ddd159972093df5aed04649c24fb4e3d` |
| INS-01 | engine实例、clone/reparent和资源迟到结果测试通过 | 纯引擎项，不要求窗口 | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| INS-02 | 21production层路由+3bridge；UI230、55wheel/event/focus及mirror3、strict通过（重叠） | 19:08–19:12退出0，1140原始记录/38 JPEG截图；持鼠lifecycle/wrong-button保留自动覆盖 | 源码[`1b79a3bb`](https://github.com/emojiiii/gpui-flow/commit/1b79a3bb9d895ac9be29fff5dd58ee151dc229da)，[实际记录](validation/2026-10-02-layered-app-summary.json) |
| LIF-01 | engine生命周期与resources对应测试已通过 | 纯归属逻辑；原生资源回执另列LIF-02 | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| LIF-02 | 18项engine resource与37项UI资源通过，含7项新增；实际Flow Arc释放独立测试 | 16:24–16:29完整16步、62条真实记录零违规；2 Ready/2 ViewReady/6 Complete；旧source/attachment/generation、失败取消和重挂载通过；退出0 | `f5ab200c6f6bfaf786742b9b0ace8a530de0d6cf` |
| LIF-03 | 部分代际/容量测试已过；硬取消/媒体桥未实现 | 未完整验收 | 尚无独立完整验收提交 |
| EVT-01 | 38项focus/keyboard +2 fixture/comparator +179 UI通过；独立断言预期路由而非仅比较相等 | 16:07–16:13真实474条完整输入/输出与独立router一致；insideUp与outsideCancel契约均验；退出0 | `0d81d182407191427019c08f49fdc351837de57b` |
| EVT-02 | 38项focus/keyboard含活动capture/Space失焦清理；2 comparator与179 UI通过 | 真实A/A/B/Clear、repeat/Shift、首Space阻止、重定向/死代际、双实例/host输入与AltTab已验；活动Escape及持有Space失焦仅纯测试，未冒称原生覆盖 | `0d81d182407191427019c08f49fdc351837de57b` |
| EVT-03 | 鼠标/多pointer排斥已有测试；真实时间戳/压力/多指未实现 | 仅鼠标局部验收 | 尚无独立完整验收提交 |
| EVT-04 | 有会话保护测试；平台组合输入/a11y验收缺失 | IME候选/辅助技术待实现或核实 | 尚无独立完整验收提交 |
| INT-01 | 最新251 interaction+150 UI受影响记录通过 | 12:46–12:57原生拖动/Shift/UndoRedo/桥撤销与输入关闭通过 | `066b91fe3650f66ff33c1a46d833b85a3db58fb4` |
| INT-02 | 固定Box已有12新回归；FontSize另有12新回归/282受影响及strict Clippy通过 | Box13:30–13:34与FontSize15:23–15:29均真实窗口退出0，UndoRedo像素一致 | `9b12aac930a5e2678dbc5afe0146c42c0e87ae8a` |
| INT-03 | editor text_tools/UI会话测试通过 | text-editing.md历史窗口验收；066b91fe对应12:55聚焦输入Escape关闭复验；不扩展为IME验证 | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| INT-04 | 工具生命周期测试有；任意自定义UI端到端未完成 | 仅现有Text工具窗口 | 尚无独立完整验收提交 |
| INT-05 | Line已验证；本项三类工具未开始 | 未实现/未验收 | 尚无独立完整验收提交 |
| INT-06 | 28约束+7 wheel、2夹具+2导航focused及strict通过；较早334 interaction非最终全套 | 9224trace/58 JPEG截图/33审计通过；19实际衰减帧、6种tail中断、约束/吸附/只读/Undo，退出0 | `93ee7f7cabfb7979aeca93ec137ccf81644baeb5`；[原生证据与来源限制](validation/2026-10-02-drag-viewport-native-summary.json) |
| APP-01 | appearance/gradient和CPU对应回归通过 | 历史渐变/多paint窗口及Canvas矢量窗口通过 | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| APP-02 | fixed最终1679/10既有ignored、UI275、strict；18helper组合及5PNG/45 Pillow通过；旧open证据保留 | 14行、99点击/32pointer/28live/38 JPEG；265审计，主窗/reopen退出0；stale footer/sticky error/未实抓取消保留 | 源码`981e9918`、夹具`369d250c`；[有界native](scale-fixed-native.md)、[不变原契约核对](scale-fixed-acceptance-audit.md)，旧open辅助20/21保留 |
| APP-03 | 受影响三crate778/3既有ignored、UI focused、strict通过；夹具7CPU+6真实App reset回归 | 修复轮15行/90次点击(73控件+17画布)/18pointer/41receipt，456审计通过；旧reset失败及旧00截图缺失事故单列 | 核心`a46ae005`、夹具`4507492c`；[native证据](scene-eraser-native.md)，整项仍部分 |
| APP-04 | 现有shadow/inner/group回归通过 | 历史受限视觉检查；缺失模式未验 | 尚无独立完整验收提交 |
| APP-05 | 历史74 state；修订UI245含13coherent，最终hook后2导航/4镜像focused复验，strict/链接通过（重叠） | 新72记录/57 JPEG截图，33状态样本完整，7点击符合accepted geometry；12硬失效样本loading保留，退出0 | `93ee7f7cabfb7979aeca93ec137ccf81644baeb5`；[coherent复验](validation/2026-10-02-state-coherent-native-summary.json)，[原始失败证据](validation/2026-10-02-state-geometry-summary.json)保留 |
| APP-06 | 无GPU damage消费测试 | 未实现/未验收 | 尚无独立完整验收提交 |
| LAY-01 | flow/auto_size/reverse/item_boxes/gap/taffy对应全套通过 | 核心是纯measure/arrange契约；窗口专门矩阵仍未宣称 | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| LAY-02 | engine derivation及layout reactive/ratio/text测试通过 | 纯提交契约；Text盒子窗口验证其普通编辑路径 | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| LAY-03 | 238布局/文字回归含28新增、13+1 GPUI文字、2直接baseline、5帧桥状态测试通过；严格Clippy通过 | 真实8个callback，20编辑一次revision/Undo；局部1容器、失败原子保留/重试、取消/替换；12截图像素核验，退出0 | [核心d9a39215](https://github.com/emojiiii/gpui-flow/commit/d9a3921525a1989cb332963900ff84d648ac51ba)、[帧桥ca44fa43](https://github.com/emojiiii/gpui-flow/commit/ca44fa4372128d3d1b11e6148e337cb3404d78a5)；[精确原生工作区证据](validation/2026-10-02-advanced-layout-summary.json)，非ca44整树二进制声明 |
| LAY-04 | text measurement与layout text回归通过 | Text padding/italic/overflow多次同源对照窗口通过 | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| ELM-01 | Ellipse25、Rect20、Path23新回归及headless批次通过 | 模型几何和CPU为主；Ellipse/Rect专项窗口未声称通过 | `6796dc885cecbb155fb00d2eb4736c64bb51afb0` |
| ELM-02 | 查询17及定位17新回归通过 | 只读/显式引擎API，不要求新增窗口 | `d914a1ef0fec79b25fb67cb7f76f2c7f774ec55d` |
| ELM-03 | 容器16新回归；全套1215及后续Text/Image clip回归通过 | 后续Text/Image真实复杂祖先窗口通过；早期角切片原生仅adapter | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| ELM-04 | 新增None/Upper/Lower和非负paraSpacing：22项新回归、1001项受影响通过；Title/small-caps及其他固定属性仍缺 | 15:51–16:01真实双路径7态区域像素一致，Undo恢复一致，退出0；不表示完整Text排版已完成 | `4b47d2597030de8a49fb08d9e2b62587604c37ab` |
| ELM-05 | 16新回归、664受影响通过（1原有忽略） | 08:45–08:49真实静态/动画/旋转/clip/Undo通过 | `cfe28b01e81c8bfba2cdc0e277bdbdb0a0375dc9` |
| ELM-06 | 原alpha18新增/UI171；crop17模型/UI204基线；最终precision9composite+9UIcrop+4UIimage（重叠）及strict通过 | 原alpha窗口加18:26–18:32crop窗口退出0；1434记录/28截图；Undo/外部release rollback像素一致，明确未原生覆盖项见[证据](validation/2026-10-02-image-crop-summary.json) | crop源码[`e88d78af`](https://github.com/emojiiii/gpui-flow/commit/e88d78af5a235d4921414a9789d65be3c0ecb531)；显式crop均CPU |
| ELM-07 | 24新回归及356受影响通过 | Canvas实际Draw/Clear/resize/Undo窗口通过 | `d365f5a9e533e823e43e09db5154935e01b68c49` |
| ELM-08 | 11矢量+7clip新回归、360/367受影响记录通过 | 11:41和11:57窗口、DPR/回滚/重绘像素对照通过 | `e4f7d9455f7aaaf38d160951c330ae03249435d1` |
| ELM-09 | 12新回归；63 composite+18 UI=81聚焦通过，strict Clippy/link通过 | 14:00:18–14:03:12 Linux窗口退出0；DPR1/2、crop/透明源/反射/clip/晚失败/resize/Undo/Clear；三组像素区域一致 | `0ab6ffbf8bdbdb837a30f6cbf193e5506cb0124d` |
| ELM-10 | 旧fill/measure、current-path、pattern独立保留；stroke68文本/362 composite/12重叠focused、scene4+52、UI46+10/feature-off40及strictClippy通过；无新增全workspace声明 | 原20按钮中DPR2 WorkLimit失败保留；修复5按钮/31JSON双DPR两批次成功，双非空8×2拒绝/再生；48对真实raw/PNG的589检查、31几何、21身份格式全过。mark未证、pattern旧48/50与外缘未定保留 | runtime[`84e016b0`](https://github.com/emojiiii/gpui-flow/commit/84e016b04b8192015000bc0ab0de49bc5e83ad9e)、fixture[`747ecf55`](https://github.com/emojiiii/gpui-flow/commit/747ecf5528429b722014b3c065f2690f4c56b41f)；[原生报告/固定范围核对](canvas-stroke-text-native.md)/[摘要](validation/2026-10-03-canvas-stroke-native-summary.json) |
| ELM-11 | 先前栅格21核心+7 UI；SVG28 focused/294 composite/5夹具及strict通过（重叠） | 先前PNG/JPEG/WebP10文件4拒绝；新SVG13尝试8SVG+2PNG/3拒绝、真实resvg及独立审计，退出0 | `93ee7f7cabfb7979aeca93ec137ccf81644baeb5`；[SVG证据](validation/2026-10-02-scene-svg-native-summary.json)，[先前栅格](validation/2026-10-02-scene-export-summary.json) |
| CUS-01 | 9项新回归、12既有registry与184 UI库检查通过；strict Clippy及链接通过 | 16:32–16:36八步真实点击/UndoRedo/双实例/renderer fallback/整域拒绝与恢复；49状态检查0失败、13 renderer输入；退出0 | `09bc537a551da09d4cb7c407e22ee10d49d3c667` |
| CUS-02 | 无完整协议验收 | 未实现/未验收 | 尚无独立完整验收提交 |
| ANI-01 | 10月2日workspace日志包含animation/sequences/timing/editor_playback通过；旧文档pending需修正 | 纯显式时钟契约；不宣称新动画窗口验收 | `a08670a2fbe2882997d21d6a9f812d4197d3da74` |
| ANI-02 | 29新回归；102 animation含doctest和17原placement通过，严格3crate Clippy通过 | 纯显式时钟/引擎与editor契约，未新增原生窗口 | `457bdede25ac8bb06b452bde4038d6c54e6e5dac` |
| ANI-03 | 34项新回归（25 timeline+9 editor），135 animation lib/tests全通过；strict Clippy通过，含失败帧/seek回滚及一次Undo | 纯显式时钟API，无新增原生控件；不把编译当窗口验收 | `7ba93bb9fa28e1b5f0622ae51e8a7ce2c88e6d62` |

## 上游符号与技术依赖

| ID | 固定来源/符号或Flow契约 | 技术依赖 |
|---|---|---|
| DAT-01 | [UICreator / registerUI；类型注册与factory契约](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/platform/platform/src/creator/UICreator.ts) | 现有Rust/GPUI依赖内实施 |
| DAT-02 | [Leaf.set / LeafData属性提交；Flow保留原子事务差异](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display/src/Leaf.ts) | 现有Rust/GPUI依赖内实施 |
| DAT-03 | [UI.one / toJSON / clone（固定可映射属性集合）](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/UI.ts) | 现有Rust/GPUI依赖内实施 |
| DAT-04 | [Finder；core Watcher / Layouter / Renderer](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/find/src/Finder.ts) | 现有Rust/GPUI依赖内实施 |
| INS-01 | [Branch.add / __realRemoveChild / clear；Leaf.destroy](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display/src/Branch.ts) | 现有Rust/GPUI依赖内实施 |
| INS-02 | [App 多层Leafer组合](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/app/src/App.ts) | 先确定多surface坐标/输入转发与所有权；避免GPUI窗口默认事件路由冲突 |
| LIF-01 | [Eventer.on / off / once / destroy；Leaf.destroy](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/event/src/Eventer.ts) | 现有Rust/GPUI依赖内实施 |
| LIF-02 | [Leafer ready/viewReady/created相关阶段](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Leafer.ts) | 现有Rust/GPUI依赖内实施 |
| LIF-03 | [Image资源完成；Flow加载预算/取消为安全扩展](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Image.ts) | 现有Rust/GPUI依赖内实施 |
| EVT-01 | [emit tree phases；Interaction指针默认顺序](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/emit.ts) | 现有Rust/GPUI依赖内实施 |
| EVT-02 | [native keyboard/blur适配；Flow程序焦点为显式契约](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction-web/src/Interaction.ts) | 现有Rust/GPUI依赖内实施 |
| EVT-03 | [Interaction.pointerDown / pointerMoveReal / pointerUp / pointerCancel](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/Interaction.ts) | 现有Rust/GPUI依赖内实施 |
| EVT-04 | [InnerEditor生命周期；IME/a11y是宿主平台验收，不声称OSS含完整协议](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/InnerEditor.ts) | 实际输入法/辅助技术及目标平台窗口；云端Linux鼠标验收不能替代平台IME/a11y |
| INT-01 | [EditBox resize/rotate外部手柄](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/display/EditBox.ts) | 现有Rust/GPUI依赖内实施 |
| INT-02 | [scaleResize / Text.__scaleResize / scaleResizeFontSize](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/resize/src/resize.ts) | 现有Rust/GPUI依赖内实施 |
| INT-03 | [InnerEditor；Flow内容panel保留不同取消语义](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/InnerEditor.ts) | 现有Rust/GPUI依赖内实施 |
| INT-04 | [EditToolCreator / EditTool / InnerEditor](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/EditToolCreator.ts) | 现有Rust/GPUI依赖内实施 |
| INT-05 | [EditPoint / EditorSkewEvent / EditorRotateEvent；通用point工具stub单列扩展](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/display/EditPoint.ts) | 现有Rust/GPUI依赖内实施 |
| INT-06 | [Scroller/config；UI Dragger约束为固定扩展契约](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/scroller/src/Scroller.ts) | 现有Rust/GPUI依赖内实施 |
| APP-01 | [Compute 多层paint及gradient处理](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/paint/src/Compute.ts) | 现有Rust/GPUI依赖内实施 |
| APP-02 | [IStrokeAttrData / IStrokeInputData；strokeAlign、cap/join/dash/scale-fixed语义](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interface/src/ICommonAttr.ts) | 现有Rust/GPUI依赖内实施 |
| APP-03 | [MaskModule及UIRender隔离/混合](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/mask/src/index.ts) | 现有Rust/GPUI依赖内实施 |
| APP-04 | [EffectModule；Shadow / InnerShadow / Blur / BackgroundBlur；ICommonAttr grayscale](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/effect/src/index.ts) | background blur须读取拥有的backdrop，不能读取任意外部宿主画面 |
| APP-05 | [state样式安装/切换；transition集成](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/index.ts) | 现有Rust/GPUI依赖内实施 |
| APP-06 | [Renderer damage消费；需要GPUI后端对应能力](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/renderer/src/Renderer.ts) | 当前GPUI整帧架构；需后端可消费damage接口/实现，不能仅改Flow字段完成 |
| LAY-01 | [flowX / flowY；common gap/wrap，x/y grow/align](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowX.ts) | 现有Rust/GPUI依赖内实施 |
| LAY-02 | [Layouter有序处理；Flow固定点/原子规则不同](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/layouter/src/Layouter.ts) | 现有Rust/GPUI依赖内实施 |
| LAY-03 | [flow layout data；未映射的百分比/baseline为Flow显式扩展](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/common/data.ts) | 现有Rust/GPUI依赖内实施 |
| LAY-04 | [getDrawData / createRows / TextLayout（同源测量契约）](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/text/src/TextConvert.ts) | 现有Rust/GPUI依赖内实施 |
| ELM-01 | [Rect/Ellipse/Line/Polygon/Star/Path/Pen独立display模块](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/UI.ts) | 现有Rust/GPUI依赖内实施 |
| ELM-02 | [getMotionPoint / motionAround / motionRotation](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/motion-path/src/index.ts) | 现有Rust/GPUI依赖内实施 |
| ELM-03 | [Box / Frame / Group；UIRender子级裁切阶段](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Box.ts) | 现有Rust/GPUI依赖内实施 |
| ELM-04 | [ITextStyleInputData固定字段：letterSpacing/wordSpacing/paraIndent/paraSpacing/textCase/writingMode/autoSizeAlign](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interface/src/ICommonAttr.ts) | 现有Rust/GPUI依赖内实施 |
| ELM-05 | [Image尺寸/fit/corner与资源呈现](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Image.ts) | 现有Rust/GPUI依赖内实施 |
| ELM-06 | [UIHit像素命中参考；crop inner工具是明确Flow扩展](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/hit/src/UIHit.ts) | 现有Rust/GPUI依赖内实施 |
| ELM-07 | [Canvas context / draw / clear / resize](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Canvas.ts) | 现有Rust/GPUI依赖内实施 |
| ELM-08 | [CanvasDrawPath / CanvasTransform / CanvasState / CanvasCompositing](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts) | 现有Rust/GPUI依赖内实施 |
| ELM-09 | [CanvasDrawImage.drawImage三个重载](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts) | 现有Rust/GPUI依赖内实施 |
| ELM-10 | [CanvasText / CanvasPath / createPattern / CanvasPattern.setTransform](https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/interface/src/canvas/ICanvas.ts) | 同App字体bridge和字形预算；strokeText需要真实描边语义，不能以bbox替代 |
| ELM-11 | [ExportModule.export / syncExport；JSON和SVG分支，栅格编码](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/export/src/export.ts) | 现有Rust/GPUI依赖内实施 |
| CUS-01 | [registerUI/registerData / 自定义类；Flow确定性geometry/hit职责](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/UI.ts) | 现有Rust/GPUI依赖内实施 |
| CUS-02 | [EditTool协议；Flow异步资源与resize provider扩展](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/EditTool.ts) | 需定义宿主异步回调与identity/attachment cancellation API |
| ANI-01 | [Animate.play / pause / seek，repeat/reverse/swing语义](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/Animate.ts) | 现有Rust/GPUI依赖内实施 |
| ANI-02 | [Transition.register(motion/motionRotation) / getMotionPoint / motionPath](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/motion-path/src/index.ts) | 现有Rust/GPUI依赖内实施 |
| ANI-03 | [AnimateList.play / seek / endingStyle；具体父子组合语义需按固定行为设计](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/AnimateList.ts) | 现有Rust/GPUI依赖内实施 |

## 状态变更记录

- 2026-10-02：初稿45项为17已验证、15部分、3待核实、1开发中、9未开始
- 本审阅版：Canvas图像原生验收通过（ELM-09），提升为有界已验证；运动绑定ANI-02开始开发，不算完成
- 收口开放集合：Canvas文字/路径/图案列举固定操作；Text列举固定属性；导出固定PNG/JPEG/WebP/SVG；明确硬件/平台依赖。新增范围不得静默加入

完整机器可读记录见 [JSON](leafer-task-ledger.json)。下一轮先补待核实的事件/焦点/资源/custom证据，不将不明历史项目当作新代码完成。

- 14:40更新：ANI-02有界持续绑定完成并有headless证据；ELM-10文字盒/测量子集完成但整项仍部分；ELM-06 alpha-hit开发中，不预计通过。当前19已验证、16部分、3待核实、1开发中、6未开始。

- 15:02更新：ELM-06 alpha-hit通过但crop内工具尚缺，故整项部分；DAT-04查询索引与随机benchmark开发中。总数和19/16/3/1/6分类计数不变。

- 15:31更新：INT-02固定Text FontSize模式通过自动和原生验收，其他组件intrinsic/auto-size范围仍缺，保持部分，不增加完成计数。

- 2026-10-02 16:36：以真实trace/paint/renderer回执闭环EVT-01、EVT-02、LIF-02、CUS-01；固定总数45不变，24已验证、21未闭环。ANI-03进入开发；所有原生与纯测试覆盖差异逐项保留。

- 2026-10-02 17:07：ANI-03按冻结有限集合闭环，25项已验证/20项未闭环；Scene PNG/JPEG/WebP原生与独立文件解码验收通过，ELM-11因SVG缺项仍部分。LAY-03进入开发，固定总数45不变。

- 2026-10-02 18:21：LAY-03真实GPUI下一帧队列与像素验收闭环，26项有界已验证/19项未闭环；固定45项不变。原生夹具含当时工作区的来源边界已记录。

- 2026-10-02 18:32：ELM-06按固定alpha+crop合同闭环，27项有界已验证/18项未闭环；DAT-03发布严格12tag子集但保留部分，不缩减冻结验收范围。

- 2026-10-02 19:12：INS-02真实重叠层/输入/坐标/生命周期闭环，28项有界已验证/17项未闭环。APP-05已增加geometry/每态transition但真实过渡出现CPU域闪失，保留部分并修复，不能以settle正确充当通过。


- 2026-10-02 20:51：INT-06固定拖动/吸附/视口/惯性、ELM-11固定四种导出格式、APP-05冻结状态/继承/每态过渡/有界geometry合同闭环，**31项有界已验证、11部分、3未开始，总数45不变**。APP-05 coherent修复有33状态样本和7次accepted-geometry点击证据；原失败记录、hard invalidation loading及canonical event-local边界保留。最初固定验收文字与当前相同，不能把未列入的任意属性/rotation/scale/skew变成新增收口条件。没有新增构建、原生窗口或workspace全套声明。
- 本轮源码已核实发布：`93ee7f7cabfb7979aeca93ec137ccf81644baeb5`，tree `a8f08d035dde97d13eecc932630e8fdda65dfb42`。INT-06的13精确源码/218预链接快照/6仅mtime分类、29取消订阅时间差，以及SVG mixed-raster/非位等价边界不会因发布而消失。

本轮三项的证据报告随本次文档修订新增，JSON的`evidence_commit`暂为空；表中`93ee7f7c`仅表示已发布实现源码，不声称后续报告已在该源码提交内。

- 2026-10-02 21:59：APP-02开放轮廓mask-only closure完成源码及有界native矩阵，scale-fixed仍缺，计数保持31已验证/11部分/3未开始。辅助bbox20/21及原42检查中未满足的一项原始记录保留，不改写为全像素通过。ELM-08边界澄清持久current path归ELM-10，无新增能力或计数。

- 2026-10-02 23:34：ELM-10固定repeat/no-repeat+transform已有真实源码、自动回归及Linux原生证据，仍缺实际outline strokeText，计数保持31已验证/11部分/3未开始。Pattern保留48/50截图检查及两外缘未定观察；327旧桌面截图JPEG勘误已发布db28b3d1，原始文件/哈希和历史失败不变。

- 2026-10-03 04:19：ELM-10按最初05c92dcf的同一操作清单逐条核对，stroke实际轮廓与预算/原子性及限定native补齐，更新为**32有界已验证/10部分/3未开始，总45不变**。未增加或删除必选操作；[范围依据](canvas-stroke-text-native.md#fixed-elm-10-acceptance-audit)说明mark只是额外host-shaper探测，pattern clipping仍有actual-RGBA测试与真实控件证据，两个JPEG外缘样本保持未定而非改写通过。原0338Z WorkLimit、两资源独立事务、受支持字体/平台集合及未实测时序边界保留。DAT-03不因tag范围未枚举而静默升级。

- 2026-10-03 06:05：APP-03有界擦除增量完成真实窗口和独立审计，仍32已验证/10部分/3未开始。旧0518Z reset错误与后续截图目录事故明确保留；旧00原图被覆盖且无真实备份，其他7原图与失败核心记录独立核验。

- 2026-10-03 07:29：APP-02 [scale-fixed有界实现](scale-fixed-strokes.md)完成headless终验：default1679/10既有ignored、UI275及strict和静态复审通过。新原生仍待验收，显式consumer/局部shear拒绝边界保留，状态与32/10/3计数不变。

- 2026-10-03 08:24：APP-02原必选验收字符串不变，scale-fixed源码/自动/14行限定原生与265审计通过后升级有界已验证，计数33/9/3，总45。拒绝域、旧JPEG辅助20/21、sticky错误条/初始stale footer及未物理验证queued callback取消均保留；[范围核对](scale-fixed-acceptance-audit.md)。APP-04 grayscale仍在独立开发，不改状态。

- 2026-10-03 08:38：APP-04增加[有界scalar grayscale](grayscale.md)，1701 headless/10既有ignored、279 UI、strict及format通过；限定原生尚待验收。其余固定效果缺口保留，状态与33/9/3计数不变。
