# 显示对象、场景树与图形编辑

本层与节点图、agent 执行完全解耦。它解决“有哪些图形、如何组合、在哪个坐标系、如何命中和传播事件”。节点工作流仅是富画布的一种上层应用。

## 目录与职责

- `crates/flow-scene/src/display/`：每个原语独立文件，持有本地几何与资源描述
- `object.rs`：共同 `DisplayObject`，统一 id/name/classes/data/transform/style/visible/locked/z_index/hit_policy
- `scene.rs`：唯一父子关系、根列表、重挂载、变换链、paint list、命中、校验、JSON
- `events.rs`：捕获 → target → 冒泡，once/off/prevent_default/stop propagation
- `crates/flow-editor/`：独立场景编辑控制器，选择与有界历史；不依赖 GPUI
- `crates/flow-ui/src/display/`：GPUI 绘制后端与能力 diagnostics
- `crates/flow-ui/src/scene_view/`：坐标换算、场景交互、选择覆盖层；不内置 Studio 面板
- `crates/flow-ui/src/components/` 与 `nodes/`：可单独组合的节点 UI，不是 SceneView 的必需依赖

## 一个场景

```rust
use gpui_flow_scene::{display::{Rect, Group}, *};

let mut scene = Scene::default();
scene.insert(
    DisplayObject::new("group", Group::new())
        .with_transform(Affine::translation(80.0, 60.0) * Affine::rotation(15.0)),
    None, None,
)?;
scene.insert(
    DisplayObject::new("card", Rect::new(180.0, 100.0).rounded(16.0))
        .with_style(Style::fill(Color::hex(0x63e2bf))),
    Some("group".into()), None,
)?;
let json = scene.to_json()?;
let scene = Scene::from_json(&json)?;
// 在已初始化 GPUI Kit 的宿主中：cx.new(|cx| SceneView::new(scene, cx).unwrap())
```

`Affine` 是六参数二维仿射矩阵；`parent * local` 先应用 local。坐标是 f32。`world_transform`、`local_to_world`、`world_to_local` 与绘制/命中共享同一树；不能只把 parent 的平移手工相加。

`Scene::update_object` 在候选场景上执行并验证；非法修改不会半提交。`reparent(..., preserve_world=true)` 保持世界变换。父子不能形成环，叶节点不能持有子节点。z_index 同级排序，顺序相同保留树顺序。

## SVG 路径数据导入

`display::Path::from_svg(d)` 接受 SVG `path` 的 `d` 字符串，复用已锁定的
`svgtypes 0.15.3` 解析语法、`kurbo 0.11.3` 转换椭圆弧，不另写字符串词法器。
支持 M/L/H/V/C/S/Q/T/A/Z 及对应相对命令、隐式重复、子路径、紧凑数值和指数。
结果是现有绝对 Move/Line/Quadratic/Cubic/Close 命令，可直接传入 DisplayObject，
复用场景序列化、拾取、描边、CPU 合成和编辑器 Undo/Redo。

```rust
let path = gpui_flow_scene::display::Path::from_svg(
    "M0 80 A70 35 -25 0 1 135 20 S150 80 120 90"
)?;
```

这是路径数据入口，不是 SVG XML/CSS/滤镜/外链文档导入。弧转换为误差目标
0.1 本地单位的 cubic，并非保留精确 A 命令或原始字符串；f32 存储另有舍入。
输入上限 1 MiB、原始及归一化命令各 16384、坐标和修正后弧半径/圆心限定
±1e6。非零半径不大于 1e-5 或不同端点距离小于 1e-9 的弧明确拒绝，避免
底层数值退化被静默当成直线。零半径为直线、同端点弧省略。错误不返回半条路径。
`primitives` 示例的第二行展示导入的旋转弧和 smooth cubic。

本批 Linux 验证：741 项 headless（8 项原有媒体忽略）、83 项 UI 单测、
workspace/all-target 严格 Clippy 与格式检查通过。新增覆盖路径语法、退化弧、
数值/工作量限制、边界/拾取、JSON、编辑历史及 CPU 真实弧像素。
实际 GPUI 窗口已显示导入曲线，点击选中、R 旋转、Ctrl-Z 恢复均观察到正确结果，
backend diagnostics 为 0。未运行 Windows/macOS、完整 SVG 文档、外部素材或性能验收。

## 容器类型

- Group：无自身几何，由子元素组成；父级 transform/opacity/visibility/locked 继承
- Box：有矩形边界和可选裁切
- Frame：有边界，默认裁切子内容，可圆角

场景树支持嵌套容器，不等于节点图的单层 Group。两者是不同领域模型。

## 自定义显示元素

`display::Custom::new(kind, bounds, contours).with_data(json)` 提供业务类型标识、声明边界、可序列化的后备矢量路径与自定义属性。缺少业务 renderer 时仍能显示其后备几何；反序列化从不执行任意应用代码。

宿主实现 `gpui_flow::display::DisplayRenderer`，并调用 `view.renderers.register("my.element", renderer)` 覆盖该类型的 GPUI 呈现。renderer 接收完整 `RenderObject`（世界/视口变换、继承透明度、裁切链）和 GPUI Window/App。定制 renderer 必须遵守自身声明的裁切/坐标约定。图节点对应的另一个扩展接口是 `NodeRenderer`；两者各自服务不同层。

## 图形编辑

`SceneEditor` 包含 scene、selection、viewport。事务先在副本上校验，再记录历史；`begin_edit/commit_edit/cancel_edit` 将连续 pointer move 合并成一次撤销。选择同时含祖先和子孙时，只变换选择根，避免双重移动。世界空间移动会转换到父级本地空间，尊重父级旋转/缩放。锁定祖先也保护后代。

`SceneView` 默认提供：

- 点击 / Shift-click 选择；Ctrl-click 选父级
- 拖动图形移动；右下选择柄沿本地坐标缩放
- 左键拖动空白框选，Shift 追加；Space+左键或中键平移
- Ctrl/Cmd-wheel 锚点缩放；F 适配
- R / Shift-R 旋转 ±15°，+ / - 缩放；Delete 删除
- Ctrl/Cmd-Z、Shift-Z / Y 撤销重做；Esc 取消当前手势

宿主可设置 `editable=false` 并订阅 `SceneViewEvent`。只读模式不启动编辑手势；视口仍可浏览。

## 事件

`SceneView::on/off` 接收真实 GPUI pointer 事件，转换成世界/本地坐标后派发。监听器属于运行时，不写入 JSON。

```rust
view.on(
    "frame", EventKind::PointerDown,
    ListenerOptions { capture: true, once: false },
    |event| {
        // ancestor capture listener sees events for descendants
        if event.data.get("read_only").is_some() { event.prevent_default(); }
    },
);
```

可以使用 `off(id)` 移除监听，`stop_propagation`/`stop_immediate_propagation` 控制传播。`prevent_default` 会阻止 SceneView 的默认 pointer-down 选中/拖动。其他事件类型可由宿主显式 dispatch；本示例不是所有设备手势的完整事件适配器。

## GPUI 后端支持边界

已实现的矢量绘制并非用轴对齐 Div 假装旋转：先在本地空间三角化，再将三角形做仿射变换，最后对祖先 Frame 的凸圆角 mask 做几何裁切。非均匀缩放也改变 stroke 几何。Path/Pen 的曲线有可控精度的线段展开。

- 支持：实色与多色标线性/径向渐变 fill/stroke、opacity、任意仿射矢量变换、嵌套圆角矢量裁切、NonZero/EvenOdd 填充
- Text：GPUI 原生排版、字体、字号、换行/对齐；正向轴对齐等比缩放可用
- Image：本地图片资源、Contain/Cover/Fill；从导入场景不会自动发起网络请求
- 当前 GPUI 后端不绘制：文字/图片的渐变蒙版、旋转/斜切/翻转/非等比缩放文字或图片、圆角/旋转/斜切/非等比变换的 Frame 裁切内文字/图片。这些会记录在 `SceneView.diagnostics`，界面给出诊断计数，不静默假装支持
- 矢量渐变由真实颜色采样生成设备分辨率纹理；支持硬色标、透明度和局部坐标映射。大小限制、缓存与错误诊断见 [渐变渲染](gradients.md)
- 另有可选[行列/内容与响应布局](flow-layout.md)、[基础属性动画及有界关键帧序列](animation.md)、[真实文字测量](text-measurement.md)，以及可替换 FFmpeg 媒体解码/播放后端
- 未实现：通用 SVG 文件解析、滤镜/混合模式/阴影、完整动画时间线、任意蒙版、文本轮廓化、协作/CRDT、百万元素性能保证

可以通过独立图形 primitive/Scene APIs 构建自己的 adapter；当前支持矩阵应作为后端契约而非整个框架未来能力上限。

## 验证

纯场景与编辑器不需要显示器：

```bash
cargo test --locked -p gpui-flow-scene -p gpui-flow-editor
cargo test --locked -p gpui-flow --lib
cargo run --locked -p flow-studio --bin primitives
cargo run --locked -p flow-studio --bin composition
```

原语 Gallery 展示独立 scene；Composition 展示独立 UI 组件和 FlowCanvas 宿主组合。两者不依赖付费服务。


SceneView 使用 `interaction::scene::ScenePluginHost`，与 FlowCanvas 共用 CanvasPlugin 泛型协议、CanvasEvent、PluginLifecycle 和 AccessPolicy。场景文档命令为 SceneCommand，图文档命令为 CanvasCommand；二者不能混用。set_editable、set_access_policy、install_plugin、uninstall_plugin、replace_plugin、set_plugin_enabled 均经相同的取消/只读规则。SceneSnappingPlugin 可选安装；场景没有图端口，因此不安装 connection 插件。

Text/Image 已支持平移与正等比缩放下的嵌套矩形 Frame/Box 裁切，图片 fit 和文本对齐使用原完整尺寸；空交集不绘制、不命中。见 [原生矩形裁切](native-clipping.md)。
