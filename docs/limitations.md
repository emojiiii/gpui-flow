# 支持边界、安全与待实现项

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

本项目是早期、可扩展的 Rust 原型，不承诺 React Flow/Leafer UI API 兼容、生产稳定性或任意规模性能。以下区分已落入源码的子集与仍待实现的契约，不代表全部功能已经完成。

> 2026-09-30 当前批次已通过Linux本地headless/UI单测、workspace check/严格clippy及文档示例。精确命令与原生链接结果见本地验证记录（历史记录见Git历史）。真实窗口交互、跨平台、CI及下列未实现能力仍不在通过声明内。

## UI 与交互

- [八方向单选transform手柄](resize-handles.md)已通过聚焦模型/UI与Linux原生拖动验收，未验范围见契约。未声明字体bridge时Text及其父子树仍限定uniform；[有界Text变换能力](text-transform-tools.md)增加可选非均匀/旋转手柄。Custom、退化/奇异或未支持的呈现不提供手柄。另有[多选共享缩放框](selection-resize.md)与[中心旋转手柄](rotation-handles.md)；旋转支持vector/Image子树，详见[Image编辑手柄](image-transform-tools.md)，仍无路径控制点、倾斜/中心点编辑；Text已有显式[固定盒子resize](text-box-resize.md)，其他intrinsic-size改写仍缺

- Studio 默认显示左侧组件、画布和 Inspector；FlowOptions 可隐藏六类辅助 UI，但尚未提供任意插槽化工作台布局 API
- Graph 内容输入按节点 kind 编辑一个 `text` / `prompt` / `source` 字段；retained Scene Text 新增可选内容编辑面板与单次Undo会话，见 [text-editing.md](text-editing.md)。不是排版所见即所得编辑器，几何、标题、端口等属性仍没有通用编辑表单
- 连线采用拖放与屏幕距离吸附，释放空白/不兼容处取消；尚无已有边拖放重连、边标签编辑或精确路径命中测试
- core 支持边/组选择、单层组移动和解组；Studio 未提供完整对应交互
- 自定义 NodeRenderer 只接管主体，不能覆盖标题、端口锚点、外框和把手；未提供完整的自定义节点布局协议
- 吸附由 SnappingPlugin 独立控制，节点对齐默认启用、网格默认关闭；宿主可配置网格间隔，图形网格显示样式仍为独立设置
- MiniMap 支持点击重新居中：`on_navigate` 已接入画布平移；尚不支持拖动视口导航
- 画布裁剪视口外节点，但仍遍历图、绘制全部边；没有空间索引、LOD、性能基准或万节点保证
- Scene 层可选显式行列布局，见 [flow-layout.md](flow-layout.md)；支持Box/Frame内容尺寸测量，但不包含图节点自动布局、全局响应式布局调度、对齐线、嵌套图分组、协同编辑、CRDT、评论、版本冲突解决或多文档标签页
- 所有键盘、输入法、触摸、无障碍、不同 DPI、多屏和中断手势尚需目标平台验收
- `FlowEvent` 是集成提示，不是每次选择/视口/宿主直接修改都覆盖的完整事件流

## 场景外观

- 矢量支持有序多层填充和居中描边，含实色/线性/径向渐变；文本、图像与无几何 Group 的额外 layers 会明确拒绝
- 新源码已有居中local-space stroke的Butt/Square/Round cap、Bevel/Miter/Round join、miter limit、dash pattern/offset，复用Kurbo dash与Lyon三角化，同一受预算限制的mesh用于高级stroke绘制/命中；headless几何与UI适配器单测通过。闭合矢量Inside/Outside已通过i_overlay接入同一mesh；开放轮廓、Text/Image/Group及超预算/精度输入明确拒绝，[边界见此](stroke-alignment.md)。Scale-fixed描边仍未实现。`Path::from_svg` 已支持有界 SVG d 命令及椭圆弧转 cubic，详见[路径导入边界](display-scenes.md#svg-路径数据导入)；不等于完整 SVG 文档或精确弧往返支持
- SceneView新增[有界矢量/静态及动画图片离屏合成](compositing.md)：整体组/多paint透明度、域内Normal/Multiply/Screen和alpha sibling mask；静态及GIF/WebP动画Image已复用GPUI资源/cache进入真实像素合成，并共享native/CPU选帧与运行时pause/seek；域内纯solid文字现可通过宿主显式共享原生字体后端桥进入合成（换行/wrap/对齐及正向均匀缩放）；默认无桥、复杂文字样式/变换、Custom与任意宿主GPUI背景混合仍整域明确报错；显式OwnedCanvas已支持SceneView自身底色/grid/完整Scene统一根域内的顶层背景混合，沿用硬预算且不做部分原生fallback，单个矢量/Image/Text叶节点外阴影已复用image Gaussian和真实alpha进入该域；Group/Box/Frame另支持[有界子树阴影](group-shadows.md)，不安全PassThrough组合拒绝；[最多8层多重外阴影](multiple-shadows.md)已接入相同预算与alpha路径；Custom、spread/box/scale-fixed阴影、擦除及通用模糊仍缺
- 默认 paints 上限32，避免无界三角化/渐变纹理工作；具体边界见 [appearance.md](appearance.md)

## 媒体

- image默认将本地路径交给GPUI；新源码的SceneView资源适配器复用GPUI公开asset/cache API，只有宿主显式resolver授权后才可映射URI。没有另建下载器、解码器、上传或文件选择器；默认策略不是目录沙箱。资源观察/失败/就绪事件已接入源码，详见[resource-lifecycle.md](resource-lifecycle.md)；模型/适配器单测通过，真实窗口资源加载/paint回执仍未验
- video/audio 可通过显式启用的 FFmpeg/FFplay 后端实际解码、播放、暂停和点击时间轴 seek；需要宿主安装工具，音频需要系统设备。音频波形只显示宿主提供的采样数据，不伪造波形
- 当前为有界预览后端，使用共用位置/单调时钟与独立音频设备时钟；没有采样级 A/V 同步、无缝 seek、录制或导出保证；详见 [媒体后端](media.md)
- 只支持允许封装的普通本地文件，不支持网络/直播/播放列表。FlowCanvas 普通构造默认关闭媒体后端，启用后最多同时创建 8 个播放器
- Linux 已验证真实解码与 SDL dummy 音频输出路径；没有声卡/扬声器的环境不能验证实际可听声音，macOS/Windows 仍需目标平台验收
- generation/agent 是离线 mock；不生成真正图片、视频或音频

## 执行

- Runtime 执行独立任务；Workflow 提供冻结 DAG 调度、端口传值、并发上限和失败传播。尚无条件分支、循环、分布式 worker 或持久任务恢复
- MockRunner 的输出只是 JSON，不会自动写回 Graph/下游节点
- 取消是合作式的，无法强制终止线程，也不会自动调用远端服务取消 API
- Workflow 有并发上限，仍按每次任务创建线程；独立 Runtime 无全局并发上限。尚无超时、自动重试、费用限制或持久任务存储
- 画布删除节点时 forget 对应任务，导入图/Open 时重置 Runtime；宿主绕过画布直接编辑模型仍须自行处理生命周期和迟到结果
- Runtime 保留最后一次输入、runner 和记录以支持 retry；可手动 forget，但没有自动淘汰或长期任务存储策略

## 持久化与模型

- Studio Save file/Open 可读写含视口文档；无系统文件选择器时用 `--file PATH` 或宿主 `document_path`，异步文件任务串行；Save/Load snapshot 仅为内存快照，Ctrl/Cmd + S 保存文件，Ctrl/Cmd + Shift + S 复制 Graph JSON
- Graph 与 FlowDocument 格式不同；前者没有 viewport，二者都不含历史、选择、任务和媒体字节
- 文件读写限制 32 MiB，采用临时文件+sync+rename；没有自动保存、脏状态/覆盖提示、备份、多实例冲突保护或跨平台断电恢复保证
- Open 会清空原图选择、历史和运行记录；保存捕获点击时的快照，不包含文件对话框等待期间的后续编辑
- schema_version 为 1，未知版本会被拒绝，未提供旧版本迁移器
- 公开模型字段允许宿主自由编辑，但也允许构造非法中间状态；导入/保存前必须校验
- 任意 JSON data 不带业务 schema，`locked` 不是权限隔离
- 分组只有一层，组边界不自动追随独立移动的成员节点
- 历史为完整图快照；高频输入与大图可能产生明显内存开销，应制定合并/保存策略

## 安全边界

1. 凭证始终留在图之外。任何图导出、日志、系统剪贴板都可能被分享；不要在 data 中放秘密
2. 导入 JSON 是不可信输入。除拓扑校验外，限制文件大小、节点/边数量、字符串长度、应用数据结构和媒体路径
3. 本地图片路径与显式启用的音视频 source 会触发文件读取。宿主应限制可用目录和文件类型，不要将不可信图视为任意路径读取授权；FFmpeg 可执行文件路径只能来自可信宿主配置
4. 真实 agent 的工具执行、HTTP 访问、shell、文件操作均需由宿主实现权限与审批；本框架没有沙箱
5. 节点字段、模型输出、供应商错误和进度消息可能含敏感信息，应在日志/遥测前脱敏
6. 本地取消、关闭画布或撤销编辑不等于撤销远端任务、付费请求或已经发出的外部操作
7. 本仓库不附带 API keys、真实付费请求或自动化发布/部署操作

## 平台与发布

CI 配置目标为 Ubuntu 24.04、macOS 15 和 Windows（windows-latest / MSVC）的原生编译、单测与示例链接；每个平台的实际结果以对应提交的 Actions 为准。编译成功不能证明桌面交互、GPU、图形驱动或系统剪贴板都正常。Windows 和 macOS 的真实窗口尚未人工验收；移动端和 Web 未列入验证矩阵，不能从底层 GPUI 的能力推导本框架已经支持。

当前 crate 不发布到 crates.io。没有为项目源代码提供开源许可证，第三方依赖按各自许可证使用；“参考设计”不是复制其代码或使用其许可证的声明。公开分发前需审查依赖许可、资产许可和构建产物。

## 状态呈现边界

支持selected/focus/hover/press/disabled的实色base fill与opacity运行时叠加；新源码增加本地命名palette、空Text固定框placeholder及父/祖父最近button状态继承，14项named-state回归通过。Geometry/custom-property状态和任意几何变化仍未实现。已有binding级实色/opacity自动transition，未覆盖每态设置、延迟/重复及gradient/None插值。叠加不改Scene/Undo/JSON，hit与fit仍使用canonical geometry；disabled只控制呈现，不代替交互权限。活跃状态采用私有Scene副本绘制，未作大场景增量性能保证。详见[state-styles.md](state-styles.md)

## 响应布局边界

可选引擎提交前provider已支持普通属性/树编辑自动派生layout；没有全局异步watcher、帧合并、局部依赖调度或input/computed分离。runtime配置不序列化/不随Undo回滚；删除后恢复的新instance需宿主重新绑定。严格size/transform白名单、同一次提交/Undo及动画冲突规则见[reactive-layout.md](reactive-layout.md)

## 动画序列边界

已有1..64段显式正时长/easing关键帧、稀疏字段保持、跨段seek和整段一次Undo；新源码增加整序列delay/repeat delay、有限/无限遍历、正反/交替方向和正有理数speed，21项timing回归通过。没有自动分配时长、逐关键帧独立timing、AnimateList/嵌套timeline或motion path。全部属性从序列开始占用，跳跃tick只原子提交采样时刻，不产生逐段回调。见[animation.md](animation.md)

## 程序焦点边界

已有显式/队列focus、不可取消Blur/Focus通知、related target和GPUI一次同步；可选[场景语义导航](scene-accessibility.md)提供有界语义树、Tab遍历与焦点圈，Linux X11/Fcitx候选/语言切换/失焦、AT-SPI树与Text Click已做有界实窗验证；独立AT Focus、其他IME/平台与纯程序化原生组合中断仍未验，GraphicsObject平台状态映射限制见该契约。不包含自动waitLeafer挂载等待或操作系统激活。256动作预算终止监听器焦点循环；详见[focus-routing.md](focus-routing.md)

反向行/列与反向换行已支持；其语义是遍历/行序反转，坐标轴起点仍由正常justify/align决定。已有独立flow-text/GPUI纯文本内在测量；新Taffy 0.13.0私有适配器处理普通行内grow/间距/安全对齐/stretch，保留Flow分行、reverse、所有权/事务及f64数值域fallback。13项兼容回归及既有自动尺寸/响应布局测试通过；任意输入收敛、舍入等价和大规模性能仍无保证。仍没有CSS flex-shrink、百分比尺寸或完整富文本排版；upstream shrinkContent指padding内容bounds处理。


当前新增[有界叶节点多层内阴影](inner-shadows.md)。支持范围、真实paint alpha与Leafer几何shape的差别、fill/inner/stroke顺序、完整有界文字源和拒绝规则见该契约；此前阶段的内阴影缺项记录保留为历史验证范围。

Current [bounded container inner shadows](container-inner-shadows.md) extend the
leaf slice to Group/Box/Frame, preserving explicit paint phases, actual subtree
alpha, independent own effects and existing budgets. See that contract for the
current validation scope; older slice receipts above remain historical.

Image now supports [own scalar/four-corner clipping and automatic geometry CPU
domains](image-corners.md) for complex transforms/ancestor masks. Its direct native
fast path remains bounded to exact supported geometry. Pure picking is not
decoded-alpha sampling. Image rotation/nonuniform outer handles now have their
separate [interaction/lifecycle contract](image-transform-tools.md).

Text beneath rounded or affine ancestors now uses [automatic bounded glyph
domains](text-ancestor-clips.md), with an explicitly supplied same-App font
bridge. Rectangular native masks stay fast; a later [affine Text slice](text-affine.md)
adds transformed glyphs. Arbitrary Text paint styles remain unsupported.

Text now supports [bounded opt-in end ellipsis](text-overflow.md) using shared
native/CPU shaping and grapheme boundaries. Default clipping and intrinsic
measurement stay unchanged; custom markers, rich text and arbitrary typography
remain outside this slice.

Canvas now has [physical straight-RGBA exchange and raw PNG bytes](canvas-pixels.md).
The bounded export excludes retained style/ancestor presentation and does not
provide browser contexts, multi-format export or document pixel persistence.

Text [italic face selection and factory font weight](text-italic.md) now flow
through the real measurement/native/CPU pipeline. Font matching belongs to the
host, explicit boxes still clip overhang, and richer typography remains separate.

Text now supports [bounded underline/strikethrough decoration](text-decoration.md)
with shared native/CPU row geometry. Opaque text remains native; nonopaque
object/fill alpha requires an explicit font bridge and an automatic CPU domain.
Custom decoration color/offset, wavy/dashed styles, skip-ink, letter/word spacing and rich runs remain unsupported. Paragraph spacing and
None/Upper/Lower presentation case are now supported as a bounded subset; see
[text case and paragraph spacing](text-case-paragraph-spacing.md). Plain decoration does not alter
measurement or permit outside-box paint.

Canvas [vector command batches](canvas-commands.md) now support owned Path
fill/stroke, gradients and batch-local transform/alpha/save/restore through the
existing compositor. They do not provide a persistent browser context, current
path builder, clearRect, image/text commands or additional composite operations.
[Per-draw Path clip stacks](canvas-clips.md) now support frozen transforms,
intersection and save/restore inside a batch; they do not change retained
Box/Frame clipping schemas. Atomicity is per surface batch.

Text [rotation, reflection, nonuniform scale and shear](text-affine.md) now use
bounded local glyph pixels plus the existing CPU image transform. A same-App
font bridge is required; exact local content masks and ancestor geometry remain
enforced. Native standalone Text, transform handles, transformed content editing
and existing effect-specific restrictions are unchanged.

[Text outer transform tools](text-transform-tools.md) now follow a default-off
interaction capability, enabled by a validated SceneView font bridge. The plain
solid/effect-free subset supports nonuniform resize and rotation, including
shared roots. Capability changes cancel active captures; intrinsic box/font-size
rewriting, skew/pivot tools and WYSIWYG editing remain outside this slice.


[Intrinsic fixed Text-box resizing](text-box-resize.md) is now an explicit opt-in
single-selection mode. Width/height and anchoring translation change while font
size, padding and affine linear terms stay fixed. Default transform-only behavior
and shared-selection transforms remain. Font-size dragging, auto-height ownership
overrides, arbitrary component intrinsic resizing and WYSIWYG editing are absent.


Canvas [decoded-image batches](canvas-image-commands.md) now share vector/context
transform, alpha, clipping and atomic publication. Crop coordinates are physical
source pixels and destinations are logical units. Negative rectangles are rejected;
images remain explicit immutable host inputs. Text draw commands and persistent
context/pattern APIs remain outside this slice. The [task ledger](leafer-task-ledger.md)
tracks bounded completion separately from partial and unverified capabilities.


[Motion-path animation](motion-path-animation.md) now provides an explicit bounded
single-target clock/seek binding and token-owned editor history. Source/ancestor
edits cancel conservatively; there is no automatic refollow, path reveal or
text-on-path. Canvas now also has [same-backend text-box draw and measurement](canvas-text-commands.md);
this is top-left box layout, not browser baseline overloads. Stroke text, persistent
current paths and patterns remain open ledger scope.


[Image source-alpha picking](image-alpha-hit.md) is now an opt-in runtime attachment
policy in SceneView, using validated current decoded frames and nearest source
pixels. Default/pure-engine picking remains geometric. Source alpha is not final
composited opacity/masks, and this does not add crop inner editing.


[Text font-size resize](text-font-resize.md) now joins Transform and fixed Box modes.
It scales the fixed box/font/padding while preserving affine linear terms and
Flow's line-height multiplier. It does not add general component intrinsic resize
or override responsive/auto-size ownership.
