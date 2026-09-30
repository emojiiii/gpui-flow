# 元素引擎与事件路由

这层是基于 Leafer 官方源码职责划分的独立 Rust 实现，不提供 Leafer API 兼容性。源码与许可定位见 [源码对照](leafer-source-map.md)。

## 唯一文档与实例

`Scene` 是可序列化输入，`SceneEngine` 是运行期所有者。`SceneEditor` 持有一个 engine；读取文档使用 `editor.scene()`。`SceneView::from_engine` 接收已经注册自定义元素的 engine，绘制、命中、监听器和编辑都使用它，视图不维护第二份 Scene。

`ElementHandle` 包含引擎身份、槽位与世代。公共字符串 id 可以重新使用，旧句柄不会指向新对象。`detach` 保留整个子树、实例身份和资源；`attach` 重新挂载；`destroy` 递归清理监听器和资源 disposer。Drop 引擎也清理资源。丢弃 detached token 本身不会销毁该子树。

`replace_scene` 用于历史恢复，保留仍存在的实例；`reset_scene` 用于加载另一份文档，使之前所有附着/分离的身份失效。`SceneEditor::load_scene` 使用后者，同时重置历史和选择。克隆复制输入与子树，分配新身份，不复制监听器。

## 注册、属性和原子变化

`ElementRegistry` 管理内置与自定义 `ElementDefinition`。每个定义提供 kind、schema、默认值、验证以及几何生成。自定义定义可提供专用命中逻辑；未知的已保存 Custom 元素保留已验证的回退几何，但不能通过未知 kind 的 factory 创建新元素。

`PropertyDescriptor` 声明类型、默认值及 dirty 类别。`set_property`、批量属性更新、文档 transaction 都先验证再提交。失败不会留下部分输入、revision、布局或生命周期变化。等值更新不增加 revision。JSON 只保存输入，不保存运行期句柄、监听器、缓存或 disposer。

`ChangeSet` 包含属性前后值、层级与生命周期变化、dirty 类别、旧/新世界损伤范围和缓存统计。矩阵、bounds、paint/hit 状态按依赖失效；父变换更新后代、子尺寸更新祖先。颜色更新不重新计算矩阵/bounds。当前提交差异和缓存遍历仍为 O(N)，不是大型场景增量复杂度保证。

`SceneEditor::set_property` 把属性更新接入同一 undo/redo；`SceneCommand::SetProperty` 再增加 capability、继承锁定检查。宿主直接访问 engine 属于可信 API，不受编辑插件的只读策略约束。编辑器在下一次操作时检测外部 revision 变化并清除旧历史/手势快照，避免 Undo 或 Cancel 复活已销毁实例；需要保留历史时使用编辑器或命令入口。动画使用begin_playback/tick_editor安全会话：完成一个Undo步骤，取消恢复基线，外部编辑中断时保留此前历史。

## 树事件先于默认交互

`SceneEventRouter` 是无 GPUI 依赖的输入适配层。顺序为命中/实例检查 → 捕获/目标/冒泡监听器 → 未被阻止的默认插件 → 必须执行的 release/cancel 清理 → 有界排队命令。

Down、Move、Up、Wheel、KeyDown/KeyUp 的 `prevent_default()` 在默认动作前生效。阻止 Up 会回滚活动手势并释放捕获，不会留下悬空事务。Escape、失焦、PointerCancel 的清理不能被阻止。次要 pointer id 不能移动或结束主指针手势。Click 需要同一存活实例、左键、同目标释放且移动小于 4 个逻辑屏幕像素。

Enter/Leave 使用祖先路径差分；Over/Out 走正常冒泡。事件提供规范化键盘/滚轮/鼠标元数据和序号。原生适配器现已接入KeyUp及可取消的Space默认导航，重复键和清理状态按router隔离，见 [键盘路由](keyboard-routing.md)。指针仍只有鼠标，压力、触摸、真实时间戳、双击合成尚未接通，不能据此宣称支持完整手势设备。

监听器可以捕获 `view.action_sender()`，用 `enqueue(origin_handle, SceneCommand)` 请求修改。队列最多 256 条，满/关闭显式返回错误；每次输入最多处理 256 条。命令在当前事件路径结束后执行，且再次检查原始实例身份、能力和锁定。销毁或加载文档后，旧监听器排队的命令不会修改同名新对象。

Graph 的 `CanvasPlugin` 使用相同输入/能力/生命周期协议，但它没有被伪装成元素层级树；graph 节点仍采用独立图文档模型，尚未成为 SceneEngine 元素。这是后续图与显示层整合的明确边界。

## 运行示例与边界

运行 `cargo run --locked -p flow-studio --bin primitives`。右下角 diamond 来自注册的 CustomDefinition，点击后排队修改 radius，几何、bounds、命中与绘制一起变化；Undo 恢复属性。侧栏显示 engine revision 和 dirty 数量。Reset 是新文档，会销毁旧文档监听器；示例为新的 Custom 实例重新安装点击行为，宿主也必须按新句柄注册自己的监听器。

GPUI 仍整帧绘制；damage 是后端可消费的信息，不等于已经做 GPU 局部重绘。现已增加[显式视觉行列布局](flow-layout.md)及[多层矢量填充/描边](appearance.md)。布局支持内容尺寸测量与嵌套固定点，由宿主显式重排、编辑+布局事务或opt-in引擎提交前provider触发，走普通原子变化通道；多层绘制沿用平面、逐 paint 透明度。隔离组透明度/混合、蒙版、擦除、阴影、全局响应式布局调度、状态过渡、完整timeline/运动路径仍未实现；[基础属性动画](animation.md)已接入显式tick与编辑器拥有的安全播放会话。当前已有渐变和视频帧也不能替代这些契约。详见 [实施顺序](engine-contract-plan.md) 与 [支持边界](limitations.md)。

[状态呈现](state-styles.md)通过独立paint-only叠加接入SceneView，不改canonical文档/Undo；实例的attachment_epoch让输入状态无法跨卸载/重挂载复活。

[同步响应布局](reactive-layout.md)可安装一个纯precommit provider；shape/transform/树修改在schema规范化后派生布局，再一次性提交。provider仅可改Affine和六类元素size；8轮上限、递归/越界/panic失败保护。UndoRedo/取消恢复不重排，runtime配置不进入document历史。
