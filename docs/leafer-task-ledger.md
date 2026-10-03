# Leafer固定45项任务表

状态：40项有界已验证、4项部分实现、1项未开始。固定ID、验收集合与边界保留；任务数不代表工时或全部上游API兼容。机器可读定义见[任务表JSON](leafer-task-ledger.json)。

功能实现、必要回归和简明契约按模块提交。逐次运行日志、截图、备份收据及历史验收JSON不进入源码仓库；既有提交历史不改写。测试方法见[本地验证](testing.md)。

| ID | 能力 | 状态 | 固定验收集合 | 边界 |
|---|---|---|---|---|
| DAT-01 | 数据 / [元素工厂与属性schema](engine.md) | 有界已验证 | 内置及Custom注册、默认值/版本/验证；非法输入原子拒绝 | 运行时回调不序列化；不含Leafer格式导入 |
| DAT-02 | 数据 / [原子属性与历史](engine.md) | 有界已验证 | 有类型setter、批量首旧末新、一次revision及UndoRedo，失败无部分修改 | 直接可信宿主底层修改不是权限边界 |
| DAT-03 | 数据 / [版本化文档与有限Leafer映射](leafer-json.md) | 有界已验证 | 必选现有Scene JSON v1读写/未知版本拒绝；固定原语tag、transform、基础paint的Leafer JSON映射，未知属性明确拒绝并往返 | 不含JS回调、任意computed缓存或完整上游JSON兼容；迁移仅在存在正式v2时定义 |
| DAT-04 | 数据 / [查询与增量缓存性能](query-indexes.md) | 有界已验证 | 固定id/name/tag/class/子树条件与顺序；随机仿射+重挂载对照、固定fixture重算计数及CPU benchmark | 无通用CSS选择器语言；不声称GPU局部绘制 |
| INS-01 | 实例 / [代际身份与树操作](engine.md) | 有界已验证 | create/clone/detach/reattach/reparent/destroy保留或更换身份有明确定义；奇异/循环原子拒绝 | 不是多App分层组合 |
| INS-02 | 实例 / [多层App实例组合](layered-app.md) | 有界已验证 | 多层画布树、跨层事件/坐标转发、独立生命周期与销毁 | 多个普通SceneView并排不等于App分层 |
| LIF-01 | 生命周期 / [订阅与拥有资源销毁](resource-lifecycle.md) | 有界已验证 | once/off/scoped disposal、destroy exactly once、旧token和迟到job不重定向 | 不包含后台加载器硬取消 |
| LIF-02 | 生命周期 / [资源就绪与呈现回执](resource-lifecycle-validation.md) | 有界已验证 | Ready/ViewReady/Complete含失败取消，真实加载/替换/销毁/paint事件顺序 | ViewReady是paint回执，不是GPU fence |
| LIF-03 | 生命周期 / [加载取消与媒体资源桥](owned-resource-loading.md) | 有界已验证 | 共享加载字节预算、无引用任务硬取消、媒体统一代际票据和错误传播 | 显式owned本地PNG/JPEG/GIF/WebP worker与FFmpeg媒体共享池；外部GPUI cache、URI和GPU内存不在硬取消/字节保证内 |
| EVT-01 | 事件 / [树路由与默认动作顺序](focus-keyboard-validation.md) | 有界已验证 | capture/target/bubble、once/off、监听队列和prevent-default；原生适配器trace与纯路由一致 | 限定现有鼠标/默认动作与精确trace；设备扩展归EVT-03，不声称全浏览器事件兼容 |
| EVT-02 | 事件 / [键盘与程序焦点](focus-keyboard-validation.md) | 有界已验证 | KeyDown/Up、重复/Space、Focus/Blur重定向、窗口失焦清理及实例隔离 | 不把Kit文本输入当完整IME/a11y |
| EVT-03 | 事件 / [设备元数据与复合手势](keyboard-routing.md) | 部分实现 | pointer type/buttons/time/pressure、触摸多指、tap/double/longpress的稳定事件契约 | 已有mouse/pointer_id、序号与原生Text双击；不是完整事件族 |
| EVT-04 | 事件 / [IME与无障碍输入](text-editing.md) | 部分实现 | 中文输入组合/切换/失焦、Tab可达与语义树，定义受支持平台验收 | Kit Textarea提供基础；平台IME边界明确未验 |
| INT-01 | 交互 / [外部选择移动缩放旋转](text-transform-tools.md) | 有界已验证 | 单选/共享root、多选、Image和能力开启Text的仿射手柄；撤销/取消/只读/陈旧capture | Custom及未支持样式拒绝；不改intrinsic尺寸 |
| INT-02 | 交互 / [内在尺寸编辑](intrinsic-resize.md) | 有界已验证 | 按组件定义尺寸/内容/字体resize策略，与布局所有权及单Undo一致 | 内置组件有界尺寸/坐标/Group内容策略及Text盒子/字体模式；布局拥有的覆盖原子拒绝，Text测量须精确相等；退化Line用端点工具，Custom另有正式协议 |
| INT-03 | 交互 / [Text内容编辑会话](text-editing.md) | 有界已验证 | 可选内容panel、一次文档Undo、Done/Cancel/blur/target切换、身份和只读保护 | 不是WYSIWYG；IME专项归EVT-04 |
| INT-04 | 交互 / [自定义内外工具协议](custom-tools.md) | 有界已验证 | registry、load/update/unload/destroy、策略门控与宿主自定义widget挂载和释放 | 宿主AnyElement/Entity及owned属性会话；outer输入插件由宿主提供，managed layer拒绝原生panel |
| INT-05 | 交互 / [路径与倾斜中心编辑工具](geometry-tools.md) | 有界已验证 | Path控制点、skew和pivot交互须各有真实几何/取消/history验收 | Flow扩展：有界Path/leaf Pen控制点、X/Y local shear、render-inert pivot及单选旋转；不是上游OSS既有实现，不含点增删/平滑或多选pivot |
| INT-06 | 交互 / [拖动约束与高级视口](drag-viewport-constraints.md) | 有界已验证 | 固定世界/父局部矩形drag bounds、可开关网格/边缘吸附、viewport min/max及惯性取消 | 固定矩形AABB及有限惯性；多指归EVT-03，无自动寻路/物理碰撞；持鼠Escape/guide像素仅模型覆盖，6个源文件仅mtime来源证据 |
| APP-01 | 外观 / [有序填充与渐变](appearance.md) | 有界已验证 | 多层矢量fill、实色/线性/径向、多stop、透明度与缓存/DPR预算 | Text/Image多paint不在此项支持子集 |
| APP-02 | 外观 / [高级描边](stroke-alignment.md) | 有界已验证 | cap/join/miter/dash、闭合及开放轮廓内外对齐、scale-fixed；bounds/hit/paint同源 | 有界共享mesh支持cap/join/miter/dash、闭合/开放对齐及每层scale-fixed；局部shear、模糊Y阈值、部分consumer明确拒绝，Canvas命令仍center-only；无全格式/位等价承诺 |
| APP-03 | 外观 / [隔离混合与蒙版擦除](mask-modes.md) | 部分实现 | 组透明度/多paint隔离、固定blend集合、mask/erase覆盖关系与hit规则 | 已实现固定blend、五种mask相位/几何裁剪与预算预检、Pixel及leaf Path擦除；完整mask布局/命中、Path容器/Text、Custom和任意外部backdrop仍缺 |
| APP-04 | 外观 / [阴影与固定滤镜集合](multiple-shadows.md) | 有界已验证 | 已有leaf/group inner/outer多影；必补spread/box/scale-fixed shadow、blur、backgroundBlur及grayscale并有像素/预算测试 | backgroundBlur仅可在拥有完整背景的有界surface执行；不承诺任意宿主backdrop |
| APP-05 | 外观 / [状态样式与过渡](state-geometry.md) | 有界已验证 | hover/press/focus/selected/disabled、命名状态/继承、每态transition及geometry变化 | 固定实色/opacity、每态timing、x/y及Rect/Ellipse/Text/Image/Box/Frame尺寸；不含任意属性/rotation/scale/skew/更广插值；hard invalidation仍loading，event.local_position仍canonical |
| APP-06 | 外观 / [GPU局部绘制后端](engine.md) | 未开始 | 后端真正消费persistent damage并有像素等价/工作量对照 | CPU dirty和damage记录已归DAT-04；GPUI当前full-frame |
| LAY-01 | 布局 / [固定与内容流式布局](flow-layout.md) | 有界已验证 | row/column、wrap/reverse、padding/gap/grow/minmax、auto-size/嵌套固定点/hidden排除 | 百分比/baseline/shrinkContent归LAY-03；不是CSS完整实现 |
| LAY-02 | 布局 / [响应派生与事务所有权](reactive-layout.md) | 有界已验证 | 安装binding后普通编辑提交前派生、白名单/有界固定点/冲突拒绝、一次Undo | 不是全局observer/帧合并局部依赖调度 |
| LAY-03 | 布局 / [高级尺寸与局部调度](advanced-flow-layout.md) | 有界已验证 | 固定父尺寸百分比、shrinkContent、文字baseline行对齐、依赖dirty子树与单帧合并；循环/所有权冲突原子拒绝 | 受父尺寸确定性约束；不实现全CSS布局或无界observer网络 |
| LAY-04 | 布局 / [同源文字测量与重排](text-measurement.md) | 有界已验证 | 同GPUI shaping的Natural/AutoHeight、字体metrics失效、真实测量推动布局 | 富排版的字词段间距归ELM-04 |
| ELM-01 | 元素 / [矢量原语与实际几何](ellipse-arcs.md) | 有界已验证 | Rect/Ellipse/Line/Polygon/Star/Path/Pen各独立模块；四角/弧环/数据API贯通bounds/hit/CPU | 不包含通用点编辑器或完整SVG文档 |
| ELM-02 | 元素 / [Path距离查询与显式定位](path-placement.md) | 有界已验证 | Kurbo局部弧长/切线、世界点、显式目标定位；自依赖/祖先/奇异/陈旧拒绝 | 持续动画绑定归ANI-02 |
| ELM-03 | 元素 / [Group Box Frame容器裁切](container-corners.md) | 有界已验证 | 分离组件/paint phases/四角、嵌套真实clip，bounds/hit/CPU/native一致受能力限制 | 不扩大Custom/特殊effect能力 |
| ELM-04 | 元素 / [Text固定排版属性](text-layout-properties.md) | 部分实现 | 已有font/wrap/align/padding/ellipsis/italic/decoration/affine；必补letterSpacing、wordSpacing、paraIndent、paraSpacing、textCase、writingMode和autoSizeAlign | 已补有界letter/wordSpacing、paraIndent、autoSizeAlign，复用textCase/paraSpacing；writingMode的vertical/reverse仍缺，整项部分 |
| ELM-05 | 元素 / [Image真实帧与几何呈现](image-corners.md) | 有界已验证 | 静态/动画帧、fit、四角及仿射/复杂clip，真实CPU像素和资源代际 | alpha hit/crop内编辑归ELM-06；解码与URL权限归宿主 |
| ELM-06 | 元素 / [Image像素命中与内编辑](image-crop.md) | 有界已验证 | decoded-alpha hit阈值/缩放/当前帧和crop内工具必须一致且可撤销 | OSS未找到Image专属crop inner editor；属于框架明确扩展，不虚称上游已有 |
| ELM-07 | 元素 / [Canvas拥有像素与生命周期](canvas-component.md) | 有界已验证 | 独立配置、运行时surface、DPR/smooth/safe-resize/clear/draw/snapshot、代际释放 | 像素不进文档/Undo；非整视图OwnedCanvas |
| ELM-08 | 元素 / [Canvas矢量上下文批次](canvas-clips.md) | 有界已验证 | Path填充/描边、transform/alpha/save/restore/冻结clip栈；晚失败原子回滚 | 绘制状态仍为batch-local；持久current path归ELM-10，图像归ELM-09，不将其他项重复计数 |
| ELM-09 | 元素 / [Canvas图像批次](canvas-image-commands.md) | 有界已验证 | immutable image输入、source crop/destination/DPR、仿射/clip/alpha和单代际原子提交 | 不取URL；负尺寸拒绝；不保证浏览器精确filter边缘 |
| ELM-10 | 元素 / [Canvas文字与路径/图案状态](canvas-stroke-text-native.md) | 有界已验证 | 必选fillText、measureText、strokeText；beginPath/moveTo/lineTo/quadraticCurveTo/bezierCurveTo/closePath；createPattern的repeat/no-repeat与transform | 同App、host提供且startup绑定的有界静态TTF/CFF、实色top-left文字盒；任意系统字体/格式和跨平台不在支持集合。原DOM/CSS字体parser、arc/rect便利构造及完整browser状态排除不变；原失败与未定观察保留 |
| ELM-11 | 元素 / [Canvas与场景导出](scene-raster-export.md) | 有界已验证 | Canvas RGBA/PNG已实现；固定Scene区域PNG/JPEG/WebP和SVG，含背景/透明度/裁切范围及缺资源拒绝 | 固定区域PNG/JPEG/lossless-WebP/mixed-SVG；Image/Canvas/Text为分节点PNG，无editable text/outlines或SVG/PNG位等价；不含字体嵌入/完整SVG导入 |
| CUS-01 | 自定义元素 / [动态几何命中与宿主呈现](custom-geometry-validation.md) | 有界已验证 | custom属性驱动几何/bounds/hit、显式native renderer、实例隔离；拒绝不支持合成 | schema本身归DAT-01；不双计工厂 |
| CUS-02 | 自定义元素 / [自定义resize和异步资源协议](custom-element-protocol.md) | 有界已验证 | 自定义尺寸/编辑策略与测量、异步ready/cancel事件正式接入 | 正式measure/resize策略、token原子尺寸编辑及代际异步producer；每view64任务/每返回1MiB，取消丢弃future并唤醒token；外部线程/进程须自行合作取消 |
| ANI-01 | 交互插件 / [有界属性动画时序](animation-timing.md) | 有界已验证 | 稀疏关键帧、delay/repeat/direction/speed、seek/pause/conflict/cancel和单Undo | 不是嵌套timeline；motion binding归ANI-02 |
| ANI-02 | 交互插件 / [持续运动路径动画](motion-path-animation.md) | 有界已验证 | 按时钟距离/切线绑定位置/旋转，变化失效/冲突/销毁/取消/history一致 | source/祖先变化保守取消，不自动refollow；无路径生长/文字沿路径/自动timer；完成后显式finish_editor封Undo |
| ANI-03 | 交互插件 / [组合动画时钟与定位控制](animation-timeline.md) | 有界已验证 | 必选AnimateList式并行与串行子动画、offset、父时钟seek/pause/reverse、结束样式及一次Undo | jump/join、physics、无限嵌套和跨所有跳帧回调不属于v1有限集合，新增须显式扩项 |

上游参考保持原固定UI/core/plugins版本，具体入口保存在JSON的upstream_reference。Flow扩展、外部宿主限制及未完成能力均按各功能契约明确说明；不以新增的非必选探测扩大原固定验收集合。
