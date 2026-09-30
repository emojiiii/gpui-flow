# 键盘事件、Space 导航与释放清理

本层是 SceneEditor/SceneView 的键盘输入契约。Graph 的 FlowCanvas 仍使用独立图输入适配器；不要把元素树路由承诺扩大到图节点树兼容。

## 同一路径

`CanvasEvent::KeyDown { key, modifiers }` 与 `KeyUp` 都进入 `SceneEventRouter`，发给最近一次未被阻止的 PointerDown 建立的焦点实例，经过捕获、目标、冒泡，再执行未被阻止的插件默认动作。原生 SceneView 的 Space 也走这个入口，不再提前返回。

`KeyCode::Space` 是明确的规范化按键；原生 `space` 和单个空格都映射到它。其他单字符保持大小写，Escape/Delete/Backspace 是类型化按键，其他命名键通过 Other 保存。`EventInput::Key` 保留键名、Shift/Ctrl/Alt/Meta 及重复标志。

重复状态根据本路由器已观察到的 Down/Up 对维护：第一次 Down 的 repeat=false，同一键未释放再次 Down 为true，Up 后重新开始。各个画布的按键集合独立。它不是平台物理键code、IME/文本输入事件或完整的系统快捷键服务；没有KeyHold单独事件。

没有焦点元素时，不虚构元素树 target。原生键盘入口仍要求 SceneView 获得GPUI焦点。元素树 `prevent_default()` 只管理本框架默认行为，不声称已经禁止操作系统或宿主应用快捷键。

## Space 默认动作

- 第一次Space Down没有被阻止才启用本router的Space导航状态
- 被阻止的第一次Down不能因auto-repeat或once监听器已移除而绕过；需要释放后再次按下
- 状态供指针默认插件和光标读取，SceneView没有另一份竞争的Space状态
- Space+左键使用平移插件，保留原选择和元素几何；释放指针结束其捕获
- Space Up一定清除导航状态，即使Up监听器阻止默认动作。已捕获的指针手势继续到PointerUp/PointerCancel，不把普通按键释放当作突然撤销拖动
- Escape、PointerCancel、失焦及活动实例失效都清理按键状态；无法通过prevent_default留下悬空捕获/编辑事务

宿主直接发送带 `Modifiers.space=true` 的PointerInput仍是显式输入，独立于键盘监听器。这是可信宿主输入协议，不是权限隔离。通过原生适配器或完整的KeyDown/Up序列使用可取消Space行为。

## 生命周期和回调

焦点保存带世代的实例句柄。destroy或新文档reset后，同名新对象不会收到旧焦点的KeyUp；旧监听器由engine释放。detach时保留监听器但暂停焦点派送；reattach后需要显式点击重新获得焦点。once/off、捕获顺序和有界action queue沿用同一引擎机制。

监听器排队的KeyUp编辑在传播完成后提交，仍校验原始实例、只读能力和锁定策略。只读模式不允许借KeyUp回调删除对象。原生焦点丢失只转发一次FocusLost，让router统一完成PointerCancel/Blur和清理，避免重复发送取消。

## 上游来源与保留差异

研究的官方源码版本仍为 leafer-ui `1e11641d3ea6b0211c83447268a90b8d58feec25`：

- [Interaction.keyDown/keyUp](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/Interaction.ts#L240-L270) 区分前置派送、首次保持和释放状态
- [KeyEvent](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/event/src/KeyEvent.ts) 定义down/hold/up及before事件
- [Keyboard](https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/event/src/Keyboard.ts) 维护按键状态与Space判断

Rust采用每router隔离的状态与已有捕获/默认动作协议，没有复制上游全局键盘对象、JS before/hold事件命名或完整焦点管理器。双击、触摸/压力、原生时间戳、IME、完整多设备输入仍未接入。本批自动测试不代替真实窗口/键盘验收。
