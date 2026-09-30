# 源码对照后的引擎契约计划

状态：A/B 引擎基础和 C 的鼠标/滚轮/KeyDown/KeyUp 事件路由已接入 SceneEditor/SceneView，正在进行完整回归与原生验收。D 的多层矢量 paints 和 E 的显式行列布局已分批实现；内容尺寸/嵌套测量、基础属性动画/安全播放历史会话及paint-only状态呈现已实现；隔离合成、全局响应式布局调度、状态过渡和完整timeline/运动路径仍为后续计划，不是完整功能完成清单。实现 API 与准确边界见 [engine.md](engine.md)。完整上游文件对照见 `leafer-source-map.md`。

## 为什么还需要这层

现有 Scene 是可序列化的树，SceneEditor 是历史控制器，CanvasPlugin 是输入默认行为。三者不能自动构成完整的元素实例系统。尤其缺少：元素定义与属性 schema、实例身份/生命周期、属性变化驱动的布局/绘制失效、所有输入共用的树事件路由、资源与实例销毁的归属。

上游 `leafer/packages/display/src/Leaf.ts` 把输入数据处理器、布局处理器、实例 innerId、parent/leafer、事件与 destroy 生命周期组合在 live Leaf 上；`Branch.ts` 负责添加/移除及挂载传播。Rust 实现应保留这些契约的职责边界，而不照搬 JS prototype/decorator 的全局修改方式。

## 层次与唯一数据所有权

1. **ElementDefinition / Registry**：tag、默认输入、PropertySpec、验证与几何构造。注册自定义元素必须同时描述数据和几何行为，不能只是覆盖绘制函数
2. **Engine / InstanceStore**：拥有 live instance 的稳定句柄、attached/detached/destroyed 状态、事件订阅和资源租约；文档状态只有一个所有者
3. **Mutation / ChangeSet**：所有属性、层级、历史恢复和插件命令经过原子入口；不成功时不产生部分属性变化、dirty revision 或生命周期事件
4. **Layout / Invalidation**：根据 PropertySpec 标记 Matrix/Bounds/Paint/Hit/Children/Resource 变化；子树变换失效向下传播，容器 bounds 失效向上传播
5. **EventRouter**：规范化输入→命中/固定路径→捕获/目标/冒泡→允许的默认行为→清理和变化事件。Down、Move、Up、Wheel、Key 使用同样的顺序
6. **Render adapter**：消费计算后的元素与 FramePlan，保持后端能力可查。GPUI 当前整帧绘制，不能把 damage rectangle 计算当作已经实现 GPU 局部重绘
7. **可选工具/插件**：图连接、图形编辑、状态样式、自动布局、动画、运动路径以明确依赖安装；不要求任何绘制实例都可编辑

GPUI View 是适配器，不应再拥有另一份与引擎并行的 document。旧 Scene/SceneEditor 的纯算法可继续作为内部基础，但新的引擎入口和视图必须共享同一规范化状态与 mutation pipeline。

## 依赖顺序及验收门槛

### A. 元素定义、实例、挂载与销毁

先实现 ElementInput（可序列化输入）、ElementHandle（运行期身份）、Registry（factory/schema）及实例状态。`remove/detach` 与 `destroy` 必须不同：前者保留子树结构/身份，后者递归清理订阅和资源并使旧句柄失效。同一公共 id 再次创建不能让旧句柄或迟到任务复活。

测试：内置/自定义 tag 构造；非法自定义数据拒绝；重复 id；克隆重新分配实例身份且不复制监听器；detach→reattach 保留树；reparent 保持双向关系/可选 world transform；destroy 顺序、幂等与旧句柄；销毁清除异步资源回调。

### B. 属性、变化批次、布局缓存

PropertySpec 声明默认值、类型/验证、序列化规则及失效类别。批量 set 验证后一次提交；无值变化不发事件/不递增 revision。保存的是输入，计算出的世界矩阵/包围盒、监听器、任务、GPU 句柄不混入 JSON。

测试：等值 set 不产生变更；一次批处理一次 frame；非法批次完整回滚；父变换使所有后代 world cache 失效；子尺寸使祖先 bounds 失效；颜色只影响 paint；改资源地址释放旧租约；移动/删除损伤范围包含旧/新 bounds；JSON/import/factory roundtrip；undo/redo 经同一变化通道。

### C. 统一树事件与交互默认动作

旧基线问题：SceneView 的 Move/Up 默认动作在树事件之前执行，Wheel/Key 不进入树分发。现已由 SceneEventRouter 统一接入，让 prevent_default 在动作前生效，同时无论被阻止与否都完成 release/cancel 清理。回调中的写操作通过有界命令队列，在固定事件路径完成后执行，避免直接重入可变借用和树结构半修改。

测试：capture→target capture→target bubble→bubble 顺序；once/off；同节点 stop 与 immediate stop 区别；Down/Move/Up/Wheel/Key 的 default prevention；hover enter/leave路径差分；pointer捕获/按钮不匹配/窗口失焦；已删除目标；事件回调排队修改/删除；取消清理不能被 prevent_default 阻断。

### D. 布局、外观、查询与自定义元素的实际接入

提供可查的布局/外观属性及内外/世界坐标方法，不只暴露一个矩阵。自定义元素的属性修改必须驱动几何、命中与渲染一起变化。查询应按 id/name/tag/class/子树有确定语义。后端未实现的阴影、混合、擦除、任意文本变换必须可查询并明确诊断；不以保存了字段冒充已经绘制。

测试：命名变换与矩阵一致；绕不同 origin 变换；层叠与命中顺序一致；自定义尺寸修改更新 bounds/path/hit；schema 默认值一致；交互状态样式恢复基础样式；多画布实例不串 registry/listener/resource 状态。

### E. 可选自动布局与动画

现有FlowLayout和基础属性动画已实现，完整Timeline仍后续。布局与动画都应产生普通 mutation，不允许直接旁路改渲染缓存。明确支持的布局测量规则、插值属性、时间步长、取消与销毁行为。

测试：固定输入确定布局；父子尺寸依赖环；一次 tick 原子更新；暂停/恢复/取消；销毁元素清理 timeline；属性覆盖/恢复规则；无资源/时钟的确定性 headless 测试。

## 保留的 Rust 差异

- 使用注册的 trait/factory 和类型化 mutation，不做 JS 全局 prototype 修改
- 当前输入插件取消未完成编辑会回滚；上游 pointerCancel 会走带 isCancel 的 pointerUp 流程，这不是同一事务语义，必须明确说明
- 内存安全和模型验证是库契约；宿主有权直接访问底层模型，但绕开引擎入口也会绕开事件/失效保证。常规公共 API 不应诱导这么做
- 不以“同名字段/模块存在”认定源码对齐；以可运行的生命周期、事件顺序、数据流和测试为验收依据
