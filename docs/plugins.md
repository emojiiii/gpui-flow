# 输入行为、插件与编辑策略

> 旧示例说明：本文涉及 Studio 的示例名称、源码路径、操作和验证结果均为历史内容，相关演示已移除；库 API 契约继续保留。当前工作台入口见[入门](getting-started.md)，迁移范围见[说明](studio-retirement.md)。

`gpui-flow-interaction` 将指针状态、编辑命令和渲染器分离。插件处理的是实际行为：移除 connection 就不能拖出连线，移除 snapping 则只剩端口本身的命中范围，移除 drag 不会通过旧的 Canvas handler 继续移动节点。

## 先运行交互工作台

历史命令（当前分支不可运行；仅供对应历史版本复现）：

```text
cargo run --locked -p flow-studio --bin interactions
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin plugins
```

工作台提供 Number 源、兼容 Number 输入、不兼容 String 输入和锁定节点。顶部可逐项开关插件、切换只读，并替换自定义最小尺寸约束插件。它使用相同的 FlowCanvas 和公共插件 API，不是另一套示范事件实现。

## 默认操作

- 从端口按下左键开始拖动，兼容目标在 **20 个画布屏幕像素**范围内吸附；有效目标有明显高亮，拒绝目标显示不同颜色/光标
- 左键释放：兼容目标提交一条边；空白或无效目标取消，之后移动鼠标没有悬空跟随线。输入端口也可以作为拖动起点
- 节点主体或标题可选择/拖动；输入框、播放按钮等内容控件自行消耗事件，避免误拖动
- 空白左键拖动框选；Shift 追加/切换选择；中键或 Space+左键平移；Ctrl/Cmd+滚轮围绕指针缩放
- 只有选中且可编辑的节点显示右下角方形缩放把手。外部命中范围固定为屏幕像素，不随缩放缩小到不可点击
- Escape、丢失焦点、窗口失活、捕获被其他鼠标按钮打断、窗口外释放或重新进入时已经没有按键，统一清除预览/光标/捕获并回滚未完成编辑
- 正常拖动的一次按下到释放产生一次 undo；取消不留下半次历史
- Ctrl/Cmd+S 保存文件；Ctrl/Cmd+Shift+S 复制图 JSON；Ctrl/Cmd+Z/Y 撤销/重做；Ctrl/Cmd+D 复制选择

这里的屏幕像素是 GPUI 的逻辑画布像素，系统 DPI 缩放由 GPUI 处理，不是世界坐标距离。

## 安装、禁用、替换

`PluginHost::default()` 是空宿主；`PluginHost::with_defaults()` 安装标准行为。FlowCanvas::new 默认选择后者。

独立插件 ID：selection、drag、resize、connection、snapping、pan、zoom、keyboard。每个插件在独立模块中实现自己的事件行为。插件接收只读文档上下文，发出 CanvasCommand 和 CanvasEffect；只有宿主负责验证和提交文档变化。

常用入口：

```rust
// 在 GPUI entity.update 回调中使用：
canvas.set_plugin_enabled("connection", false, cx)?;
canvas.uninstall_plugin("snapping", cx)?;
canvas.install_plugin(my_snapping_plugin, cx)?;
canvas.replace_plugin(replacement_with_same_id, cx)?;
canvas.set_editable(false, cx);
```

禁用、卸载、替换参与手势的插件及修改编辑策略，会取消其活动捕获并回滚事务。不能把插件列表当普通 Vec 原地替换；使用生命周期 API。

`SnappingPlugin` 是命令中间件，可替换端口近邻吸附与节点定位规则。默认节点对齐阈值 6px，端口 20px；网格默认关闭。`SnapConfig` 可配置 grid_size、node_alignment、threshold_pixels、port_radius_pixels。关闭 snapping 后不保留隐藏的近邻补偿，但端口的 22px 控件命中框仍可直接连接。

## 扩展协议

`CanvasPlugin<Document, Command>`、`PluginContext<Document>` 和 `PluginOutput<Command>` 是可复用的文档适配协议；默认类型对应 Graph Editor 和 CanvasCommand。共用 CanvasEvent、PointerInput、AccessPolicy、PluginLifecycle、光标与报告约定，文档专用行为通过具体命令验证。

- priority 较小先执行；相同 priority 按安装顺序
- on_event 接收统一的 PointerDown/Move/Up/Cancel、FocusLost、Wheel、KeyDown
- prevent_default 跳过后续默认行为插件，观察型插件仍可接收；stop_propagation 停止后续插件
- filter_command 可以约束/改写或拒绝命令；宿主在所有中间件之后再次检查能力、锁定和模型不变量
- on_lifecycle 接收 Installed、Enabled、Disabled、Cancelled、Uninstalled
- 指针捕获和事务归属由宿主管理，插件不能直接把其他插件的未完成编辑混入自己的事务

[可执行自定义插件示例](examples/src/bin/plugins.rs) 将默认 snapping 替换成 10 单位网格，验证实际拖动结果、单步撤销和只读命令拒绝。`interactions` 原生示例还演示可替换的 ResizeNode 中间件。

## 只读不是隐藏工具栏

AccessPolicy 分别控制 Edit、Navigation、Selection、ClipboardRead。read_only 保留浏览、选择和复制，阻止图修改；locked 关闭这些能力。绘制本身不依赖可编辑性。

FlowCanvas 的拖动、缩放尺寸、连接、删除、分组、粘贴、复制节点、历史操作、内容输入以及文件/JSON 导入都走同一命令入口。异步打开在完成时再次经过策略，避免等待期间切成只读后仍替换图。播放媒体和执行器是宿主另外授权的能力，不由 Edit 标志控制。

这是应用内编辑策略，不是权限沙箱。宿主拥有公共 Editor，可直接改模型；不可信插件不能被当作安全隔离代码加载。插件为编译期 Rust trait 实现，没有远程脚本下载、动态库热加载或依赖注入容器。

## 边界

当前输入适配针对单鼠标指针；触摸多指、笔压、跨窗口拖放、操作系统级原生 pointer capture、边路径命中/重连、完整无障碍语义仍需专门实现。窗口外释放依赖 GPUI 的 mouse-up-out 和失焦通知，重新进入无按键是额外的清理保障。

仍不宣称 Leafer API/引擎对等。显示对象树的捕获/冒泡事件与编辑行为插件是不同层：前者描述对象事件传播，后者决定交互默认动作及可编辑命令。

## SceneView 文档适配器

`interaction::scene::ScenePluginHost` 真正执行保留式场景的选择、拖动、缩放尺寸、平移、缩放、历史和键盘命令，使用同一 `CanvasPlugin<SceneEditor, SceneCommand>` 协议。SceneView 不再保留另一份手势状态机。修改插件或进入只读时也会取消捕获并回滚未提交编辑。嵌套对象移动使用世界到父局部坐标转换，缩放沿原局部 bounds 原点计算，文字/图片默认保持等比。

插件 ID 与图适配器保持 selection、drag、resize、pan、zoom、keyboard；SceneSnappingPlugin 为可选的世界网格约束。connection 是 Graph 专用能力，不套到没有端口的普通显示对象。事件对象的捕获/冒泡仍由 scene EventDispatcher 管理，PointerCancel 明确通知被中断的对象事件流。
