# Leafer source architecture map and Rust alignment plan

Status: historical source research plus a separately labelled current source map, **not a full parity claim**. Reviewed 2026-09-30. The original research changed no Rust source and executed no upstream code, install, build script or example. Later implementation slices below must not be confused with that original read-only report.

> 当前批次已恢复Linux本地验证：headless及UI回归、workspace check/严格clippy和可执行文档通过；精确范围见[本地验证记录](local-validation-2026-09-30.md)。CI、真实窗口交互与跨平台仍未验，自动回归不表示完整Leafer能力已经完成。

> 基线说明：下方差距矩阵记录源码研究时的 `43eda10` 状态。后续已实现的引擎身份/生命周期/schema/dirty 缓存及原生事件接入见 [engine.md](engine.md)，不要把历史矩阵的“absent”误读为最新状态。后续批次实现了[多层矢量 paints](appearance.md)和[显式行列布局](flow-layout.md)；另有基础属性动画和编辑器安全播放会话；完整外观合成/效果、全局响应式布局调度、完整timeline/高级状态过渡/运动路径仍然缺失。

## 当前增量状态（源码能力，不覆盖历史基线）

- 引擎实例、生命周期、registry/schema、dirty 缓存、默认动作前事件路由：已实现，见 [engine.md](engine.md)
- 外观：有序多层矢量 fill/stroke、渐变纹理逐层缓存、可见描边 bounds/hit、原子属性更新；新源码增加居中local-space cap/join/miter/dash，Kurbo负责dash、Lyon负责stroke mesh，并用于绘制/命中。文本/图像不接受新layers；闭合矢量inside/outside已复用i_overlay有界Boolean mesh；开放对齐轮廓和scale-fixed仍缺。新增[有界矢量/静态及动画图片隔离合成](compositing.md)已接入SceneView，覆盖整体opacity、域内Normal/Multiply/Screen及alpha sibling mask；域内solid纯文字已增加宿主显式共享原生字体后端桥，复杂文字样式/变换、Custom、外部backdrop、完整阴影/擦除仍不支持；新增单个矢量/Image/Text叶节点外阴影，复用image Gaussian和同域真实alpha，并扩展为[有界Group/Box/Frame子树阴影](group-shadows.md)与[最多8层多重外阴影](multiple-shadows.md)，详见[范围与限制](compositing.md#bounded-leaf-outer-shadows)
- 视觉布局：可选 `gpui-flow-layout`，固定/内容尺寸 Box/Frame 的行/列、换行、padding/gap、对齐、grow/尺寸范围、隐藏与排除区别、嵌套测量/安排、一次事务/撤销和独立配置持久化。显式pass、编辑+布局事务或opt-in提交前自动派生，具备内容auto-size和嵌套测量固定点；已有可选真实文字测量与响应高度；没有全局响应式调度、百分比或shrink，见 [flow-layout.md](flow-layout.md)
- 新原生示例 `layout_appearance` 将布局输出送入同一 SceneView；编译/模型测试不等于新窗口视觉验收
- 键盘增量：KeyUp/Space进入同一路由，首按可取消、Up/取消/失焦必清状态、重复键和焦点按实例隔离，见 [keyboard-routing.md](keyboard-routing.md)。没有新增双击/触摸/IME支持
- 动画增量：1..64段显式关键帧、稀疏字段保持/累计端点预检、跨段seek；显式tick的平移/opacity/实色/自定义数值插值，原子跨元素更新、pause/resume/seek/cancel、旧身份保护及完成一次Undo/取消恢复基线的编辑器播放会话，见 [animation.md](animation.md)。新源码还有整序列delay/repeat delay、有限/无限遍历、正反/交替方向、正有理数speed和AnimationClip组合；没有自动时长/嵌套timeline/运动路径，见[animation-timing.md](animation-timing.md)；paint-only状态过渡由interaction呈现层提供
- 自动尺寸增量：固定/内容尺寸、有限min/max、wrap交叉轴测量、底向上测量与顶向下安排、32-pass上限、尺寸所有权冲突拒绝；custom geometry先规范化再测量，编辑+布局一个Undo步骤。不是全局observer调度
- 状态呈现增量：selected/focus/hover/press/disabled优先级、paint-only填色/opacity叠加、不改文档/Undo、PointerLeave和挂载期隔离已接入SceneView；已有实色/opacity进出过渡、连续反转和逐帧调度；已有宿主程序focus/clear和native同步；新源码增加命名palette、固定框空Text placeholder和父/祖父最近button继承；每态transition覆盖、geometry状态仍缺，见[state-styles.md](state-styles.md)与[named-states.md](named-states.md)
- 同步响应布局增量：安装live绑定后普通shape/transform/树编辑在一次提交内派生，严格size/transform白名单、8轮外层固定点、失败与播放历史保护；UndoRedo精确还原且runtime配置不序列化。没有帧合并/局部依赖调度，见[reactive-layout.md](reactive-layout.md)
- 程序焦点增量：宿主/监听队列请求，选中分离、Blur阶段处理动作、失效目标抑制、256预算及native一次同步；不含Tab/IME/无障碍，见[focus-routing.md](focus-routing.md)
- 反向布局增量：RowReverse/ColumnReverse逆序遍历后按正常坐标排布，wrap_reverse独立反转完整行/列；原树/paint顺序与runtime身份保持，嵌套auto-size/ownership/reactive单Undo沿用既有管线。未把shrinkContent误当CSS shrink
- 文字测量增量：字体/字号/字重、同源GPUI shaping/paint、换行/空行、真实尺寸进入响应布局，绑定/动画/Undo边界及显式字体metrics失效后同步reflow；本轮仅纯文本子集，无富文本编辑/IME/百分比/完整文字排版，见[text-measurement.md](text-measurement.md)
- 原生裁切增量：Text/Image共享矩形clip链，嵌套Frame/Box、平移与正等比缩放，Text保留完整对齐框、Image保留fit框并负偏移裁切；空交集不paint不hit，矢量圆角/仿射裁切保持。原生圆角/任意路径mask仍缺，见[native-clipping.md](native-clipping.md)
- 复用增量：Kit Button负责外围控制激活；petgraph 0.8.3负责图可达/拓扑遍历；Kurbo 0.11.3负责tight curve bounds；Taffy 0.13.0私有适配器处理普通行内数值分配，Flow仍保留分行/内容测量/reverse/事务及极端数值f64 fallback。均待编译/运行验证
- 资源增量：新SceneResources身份/attachment epoch/源generation票据与SceneView原生image/text结果、资源事件、Ready/ViewReady/Complete receipt已接入源码。Complete含失败/取消，ViewReady为GPUI paint回执，不是GPU呈现fence；媒体桥接、共享加载硬取消和字节预算仍缺，见[resource-lifecycle.md](resource-lifecycle.md)
- 余项继续按 D/E 的实际行为和测试推进，不以字段或同名模块存在认定对齐

## 九类能力当前边界（含待验证源码增量）

这里描述当前源码接入的行为与仍待实现的契约；本批新增项已有本地自动回归，下方历史源码差距表保留原研究基线。

| 类别 | 当前实现 | 主要未覆盖契约 |
|---|---|---|
| 事件 | 实例持有、捕获/目标/冒泡、once、队列、默认动作前路由、KeyUp、不可取消Blur/Focus及related target | 多指/压力、双击/长按、IME及完整事件族 |
| 交互 | 选择/拖动/缩放/捕获取消、Space平移、插件策略、程序焦点/有界重定向/native同步 | 专用内外编辑工具生命周期、复杂手势 |
| 外观 | 多层矢量paint、线性/径向渐变、共享mesh的cap/join/dash描边、named/placeholder/button状态及fill/opacity过渡、Text/Image矩形祖先clip链、有界矢量整体opacity/内部blend/alpha mask | 开放轮廓inside/outside与scale-fixed stroke、geometry状态、复杂文字样式/变换、域内Custom、外部backdrop、更多mask、不安全PassThrough组阴影、Custom/spread/box/scale-fixed shadow及通用filter |
| 布局 | 固定/内容轴、双轴reverse遍历/wrap-reverse、wrap测量、真实字体文字Natural/AutoHeight、嵌套有界收敛、opt-in提交前自动派生 | 帧合并/局部依赖调度、百分比、shrinkContent、文本baseline/富排版 |
| 数据 | 类型属性/schema/default、原子变化、dirty分类、JSON | 完整input/computed分离及Leafer导入导出 |
| 元素 | 主要原语、场景树、变换、矢量完整裁切及原生矩形clip、统一几何命中 | 命令式Canvas元素、各类型高级属性完全覆盖 |
| 自定义元素 | 注册factory/schema、确定性自定义几何/picking、数值动画 | 自定义resize/编辑工具协议、异步资源就绪整合 |
| 实例 | 独立引擎/代际handle、clone/detach/reparent/destroy | 多层App实例组合与跨层转发 |
| 生命周期 | 原子结构事件、销毁清理、动画目标/历史保护；新源码桥接原生image/text结果、资源票据和Ready/ViewReady/Complete回执 | 媒体资源桥接、共享后台加载硬取消/字节预算、GPU呈现fence |

动画源码具备基础属性时钟、有界关键帧序列、整序列timing和编辑器播放事务；新timing的21项本地回归已通过，完整嵌套timeline、每态transition覆盖/geometry状态过渡和运动路径仍待实现。

本轮过渡复用纯插值器；按binding配置transition/transitionOut回退，反转从当前显示值开始。文档同通道变化取消旧motion、失败整帧原子、生命周期清理且不改Undo。None/gradient离散切换，未实现每态配置、延迟/循环或geometry transition。

## 1. Provenance and license boundary

The official UI README explicitly identifies separate core, UI, and official-plugin repositories ([repository map][U-readme]). All three were cloned for inspection and left clean:

| Repository | Inspected commit | Local reference checkout | Commit subject |
|---|---|---|---|
| `leaferjs/leafer-ui` | `1e11641d3ea6b0211c83447268a90b8d58feec25` | `/workspace/shared/leafer-reference/leafer-ui` | `v2.3.0` |
| `leaferjs/leafer` | `1d703303ff0759ce4973d54c87eeb8192dddc570` | `/workspace/shared/leafer-reference/leafer` | `feat(Leaf): update complex` |
| `leaferjs/leafer-in` | `3eb445bf34debb638766615282224b77108bf610` | `/workspace/shared/leafer-reference/leafer-in` | `v2.3.0` |

The UI and plugin snapshots are tagged-release commit subjects; the core snapshot is the subsequently inspected branch HEAD. They are independently pinned references, **not a tested combined upstream release build**. Source links below include the full commit, not mutable `main` URLs.

All 103 discovered `LICENSE` files across these checkouts have identical SHA-256 `59646fafdf669b2550a07550553707b31c95734246de753d63463f8eed760a66`: MIT, copyright 2023-present Chao (Leafer) Wan ([core license][L-license], [UI license][U-license], [plugin license][I-license]). This report describes contracts in original prose; it does not import implementation or change gpui-flow's license. Any future copying of substantial source requires retaining the applicable copyright/license notices and a separate provenance review. Commercial products referenced by upstream documentation are not assumed to have these permissions.

Rust paths refer to the `/workspace/shared/gpui-flow` working tree inspected during this review. Its source directory is not itself a Git checkout, so this document does not invent a Rust commit pin. Later integration can change line numbers; symbols and paths are the durable Rust references.

## 2. Main conclusion: align contracts before adding menu switches

Leafer is not one large canvas handler with flags. Its source separates:

1. **Element input and data processors**: declared defaults, input/computed values, serialization, property-change categories
2. **Live elements and branches**: runtime identity, parent/root membership, event ownership, creation/mount/removal/destruction
3. **Derived layout and rendering state**: dirty matrices/bounds/surfaces, watched changes, ordered layout, damage accumulation
4. **Event and interaction layers**: per-instance subscriptions, picked tree paths, capture/bubble dispatch, normalized pointer state, gesture synthesis
5. **Appearance and backend modules**: geometry, paints, stroke rules, intermediate compositing, masks, resources
6. **Factories and extension definitions**: registered element types, processors, edit tools, platform service creators
7. **Optional feature modules**: viewport, editor, state styles, flow layout, animation, transitions, motion paths, export, query, resize
8. **Application/engine lifecycle**: controller start/stop, readiness stages, next-render queues, layered app composition, resource cleanup

Our `CanvasPlugin` host is a useful **default-action and command-policy subsystem**. It is not an equivalent of all eight layers. Existing scene geometry, validated document editing, rollback, gradients, and media are reusable foundations; the next architectural work should join them through explicit engine contracts.

Also, Leafer's own `Plugin.list` is only a presence/dependency diagnostic map. Actual extensions assign service implementations, register classes/properties, compose modules, or replace methods at module-import time ([Plugin][L-plugin], [module/rewrite mechanism][L-rewrite], [editor installation][I-editor-install]). Port the contracts, not JavaScript's global prototype mutation. Runtime install/remove/replace safety remains a deliberate Rust design requirement, not something proven by this upstream registry.

## 3. Concrete source map

Prefixes in this table denote the pinned repositories above: **L** core `leafer`, **U** `leafer-ui`, **I** `leafer-in`.

| Contract | Official source and entry points | Current Rust location | Alignment/gap |
|---|---|---|---|
| Common element | L `packages/display/src/Leaf.ts`: `Leaf`, `reset`, `__bindLeafer`, `destroy`; U `packages/display/src/UI.ts`: `UI` | `crates/flow-scene/src/object.rs`, `scene.rs` | Common serializable state exists. A live instance/root-membership/resource lifecycle does not |
| Tree operations | L `packages/display/src/Branch.ts`: `add`, `__realRemoveChild`, `clear`; L `LeafHelper.drop` | `flow-scene/src/scene.rs`: `insert`, `remove`, `reparent` | Atomic validated tree edits exist. Detach retaining a whole subtree, instance identity, and lifecycle events are absent |
| Primitive modules | U `packages/display/src/{Rect,Ellipse,Line,Polygon,Star,Path,Pen,Text,Image,Group,Box,Frame,Canvas}.ts` | `flow-scene/src/display/*.rs` | Separate primitive modules exist. `Canvas`-like imperative content and several advanced per-kind contracts do not |
| Class/factory registration | L `packages/platform/platform/src/creator/UICreator.ts`; L `decorator/src/class.ts`; U `UI.one/registerUI/registerData` | `flow-scene/src/display/custom.rs`; `flow-ui/src/display/registry.rs` | Custom fallback geometry + renderer override exist. No pure element schema/factory/measure/hit/lifecycle registration |
| Property metadata | L `decorator/src/data.ts`: `defineLeafAttr`, `positionType`, `boundsType`, `surfaceType`, etc. | `flow-scene/src/object.rs`, `style.rs`; `Scene::update_object` | Typed fields + validation exist. No metadata for defaults, mutation categories, interpolation, or custom property validation |
| Input/computed data | L `display-module/data/src/LeafData.ts`; U `display-module/data/src/UIData.ts` | serde document + computed geometry methods | No separate computed property store, normalized paint cache, or structured old/new property delta |
| Watch/layout/render pipeline | L `partner/{watcher,layouter,renderer}/src` | `Scene` queries; `flow-ui/src/scene_view/render.rs` | Recomputes/validates whole data in common paths. No engine-wide dirty dependency pipeline or damage plan |
| Event subscription | L `event/src/Eventer.ts`: `on`, `off`, `once`, `on_`, `off_`, `emit` | `flow-scene/src/events.rs`: `EventDispatcher` | Capture/once/token off exist. Not owned by instance generations; no subscription scope; no typed property/lifecycle families |
| Tree dispatch | U `interaction/interaction/src/emit.ts` | `EventDispatcher::dispatch` | Tree phases exist. Routing/default execution remain split in the GPUI adapter |
| Normalized interaction | U `InteractionBase`, `Dragger`; U `interaction-web/src/Interaction.ts` | `flow-interaction/src/event.rs`, both plugin hosts; `flow-ui/src/{canvas,scene_view}/interaction.rs` | Mouse/capture/cancel policy exists. Missing coherent hover/focus/pointer-kind/button-mask/time/gesture event protocol |
| Picking | L `partner/selector/src/Picker.ts`; U `hit/src/UIHit.ts` | `Scene::hit_test_all`, graph adapter geometry | Nested transformed vector picking exists. Separate fill/stroke/pixel hit rules, through/proxy picking, and hit-path policy need contracts |
| Appearance/compositing | U `UIRender`, `UIData`, `partner/{paint,gradient,effect,mask}`; L `LeafRender` | `flow-scene/src/style.rs`, `gradient.rs`; `flow-ui/src/display/*` | Real vector linear/radial gradients exist. Layered paints, isolated group opacity, blend/mask/effect pipeline are absent |
| Editor tools | I `editor/src/Editor.ts`, `tool/{EditToolCreator,EditTool,InnerEditor}.ts`, `display/EditBox.ts`, `helper/EditDataHelper.ts` | `flow-editor`; `flow-interaction/src/scene/plugins/*`; GPUI selection overlay | Bounded per-controller registry and plain Text sessions: [text editing](text-editing.md). [Eight single-selection transform handles](resize-handles.md) reuse this lifecycle; focused Linux model/UI/native checks passed with remaining coverage documented. [Shared multi-selection resize](selection-resize.md) now has fresh affected/strict/UI/native validation after source reconstruction, with explicit bounded coverage. Path/rotation/skew/pivot tools and arbitrary inner widgets remain absent |
| Flow layout | I `flow/src/{Flow,index,decorate}.ts`, `layout/*` | `display/box_shape.rs`, `frame.rs` | Painted containers are not an auto-layout engine. No row/column/wrap/grow/measure/arrange contract |
| Animation/state/motion | I `animate`, `transition`, `state`, `motion-path` | No general scene animation subsystem | Workflow scheduler and media clock do not implement visual property animation |
| Engine/app lifecycle | U `display/src/Leafer.ts`, `app/src/App.ts` | GPUI `SceneView`, resource-specific caches | No general Created/Ready/ViewReady/Complete stages, app layers, or instance-resource teardown contract |

## 4. Events: actual semantics and required decisions

### 4.1 Listener storage and invocation

`Eventer.on` maintains separate capture/bubble maps, supports several registration forms, and deduplicates a listener function within one event type/phase. `off(type, callback, options)` removes that phase's listener; `off(type)` clears both maps for the type; `off()` clears all. `on_` returns a removal handle including emitter, listener, and options; `off_` handles lists of such handles. `once` sets an option ([Eventer lines 27–134][L-eventer]).

`emit` iterates the live listener array in registration order. A once-listener is removed **after** invocation; `isStopNow` stops the rest of that emitter's list. `emitEvent` sets `current`. `destroy` releases both maps and `syncEventer` ([Eventer lines 136–173][L-eventer]). This is not evidence for snapshot-safe reentrant mutation: live-list mutation and post-callback once removal have semantics Rust should specify explicitly rather than blindly reproduce.

`Event.stopDefault`, `stop`, and `stopNow` are distinct; they also forward to the native `origin` through `Platform.event`. `stopNow` sets both immediate and propagation stop. `stopDefault` alone is not a general undo or command-veto contract ([Event][L-event]).

The event-type boundary is concrete: `EventCreator.register/get` maps event names to constructors; `UIEvent` adds path, buttons, modifiers, timestamp and coordinate-conversion helpers; `PointerEvent` adds pointer type, pressure, tilt and related pointer metadata ([event factory][L-event-creator], [UIEvent][U-ui-event], [PointerEvent][U-pointer-event]). These inputs belong in typed routing data, not an unstructured style/config map.

Our dispatcher already removes once-handlers before invocation, has listener tokens, fixes target/current routing identity, and computes each listener's local coordinate. Preserve these good properties. But listeners are held in one external map keyed only by `DisplayId`, `retain_existing` is manual, and callbacks receive only `&mut SceneEvent`. There is no explicit safe mechanism to enqueue subscriptions/tree edits/default commands from a listener. A new object reusing an old string ID can inherit stale listeners unless the runtime invalidates ownership.

### 4.2 Tree path and phases

The upstream picker constructs a target-to-root `LeafList`. `emit` walks its reverse for capture, then forward for bubble. Capture includes the target with phase 1; target bubble uses phase 2; ancestors use phase 3. Destroyed elements are skipped. `hitSelf` and exclusion paths gate emission. State-style updates happen in the non-capture pass **before** user listeners. An app has special forwarding for move/zoom/rotate/key events to sibling Leafer layers ([picker][L-picker], [tree emitter][U-emit]).

Important differences from generic DOM expectations:

- The emitter creates a typed event object for each receiving element/phase via `EventCreator`, rather than necessarily sharing one event object across the entire path
- `stopDefault`'s flag is not aggregated by this path loop into an application command-policy result
- Enter/leave use previous/current path exclusion, not just a generic `bubbles = false` shortcut
- Source has `syncEventer` and multi-layer app forwarding; neither is required for our first contract, but both need an explicit later place

Rust currently uses one event object, target capture and non-capture both have `EventPhase::Target`, and enter/leave default to non-bubbling. These are reasonable documented choices, **not exact Leafer parity**. Do not change them merely to match names. Tests must establish the chosen contract.

### 4.3 Pointer order, picking, drag, release, cancellation

The relevant sequence is explicit in upstream source:

1. `pointerDown`: default button → `updateDownData/findPath` → path mode adjustment → `BEFORE_DOWN` → optional repick → `DOWN` → tap/long-press setup → `dragger.setDragData` **after** down listeners ([InteractionBase 96–121][U-interaction])
2. `pointerMoveReal`: `BEFORE_MOVE` on default/root path → test screen-distance threshold → cancel tap waits → `Dragger.checkDrag` → repick hover if not viewport-moving → pointer MOVE → hover transitions → drop over/enter transitions → cursor ([137–165][U-interaction])
3. `Dragger.dragStart`: emit `drag.start`, then determine draggable elements and save their initial points. `drag` derives incremental and total displacement. For ordinary drags it calls `dragReal` **before** `drag.before_drag` and `drag` events. The word “before” is therefore not proof that handlers can prevent every underlying position change ([Dragger 75–139][U-dragger])
4. `pointerUp`: repick current path, union in the original down path, clear `downData` **before** BEFORE_UP/UP, touch leave, tap/menu if not canceled, drag-end, then cursor restoration ([167–196][U-interaction])
5. `pointerCancel`: invoke pointerUp with `isCancel = true`. It suppresses tap/menu in the shown method. It does **not** restore a document snapshot; drag-end still runs. Missing buttons during viewport movement also lead to cancellation ([198–202][U-interaction], [Dragger 54–73, 165–221][U-dragger])
6. Browser adapter uses canvas-local down and document/window move/up/cancel listeners. Pointer/touch/mouse de-duplication, multi-touch cancellation of single-pointer state, button conversion, and native defaults are a separate platform layer ([web Interaction][U-web-interaction], [InteractionHelper][U-interaction-helper])

Our transaction rollback on lost capture, disabled/uninstalled plugins, policy changes, focus loss, and Escape is stronger than the above source behavior and should remain part of our editor contract.

### 4.4 Current integration defect exposed by this mapping

In `crates/flow-ui/src/scene_view/interaction.rs` at review time:

- Down dispatches `SceneEvent::PointerDown` before the default-action host and honors its prevention result
- Move calls the plugin host first, then tree dispatch, ignoring the dispatch return value
- Up calls the plugin host first, allowing commit, then tree dispatch, ignoring prevention
- Wheel and key enter only the plugin host; declared scene event variants do not establish actual routing
- Click is synthesized in the GPUI adapter, while capture/gesture state belongs to the pure host

This split is an architectural correctness gap. One pure router should own event ordering and click/hover/focus state. Proposed order: normalize → flush required derived geometry → pick immutable path → dispatch tree event → run permitted default-action plugins → apply queued commands/change sets → emit resulting domain events → request render. Mandatory capture/transaction cleanup must run even when propagation/default actions are prevented. This is our explicit contract, not a claim that Leafer uses exactly this order.

## 5. Property metadata, data processors, and change invalidation

### 5.1 Source contract

`defineLeafAttr` combines an accessor descriptor with a corresponding data-processor field. `defineDataProcessor` handles inherited descriptors, callable defaults, lazily cloned object defaults, and custom `setX` methods. Width/height may resolve from natural size or computed box bounds. Thus an element's public input API is not simply its internal computed representation ([decorator data 18–25, 300–361][L-decorators]).

The decorators encode **which derived subsystem is dirty**:

| Upstream category | Examples in UI | Effect |
|---|---|---|
| position / scroll | x, y, offsets, scroll | local/world matrix; scroll-world support |
| scale / rotation | scaleX/Y, rotation, skew | matrix plus scale/rotation flags |
| bounds / path | width/height, path, padding | box/local bounds and hit data |
| stroke bounds | stroke, stroke width/alignment | stroke spread, bounds and hit data |
| render bounds | effects/render spread | render spread, bounds and hit data |
| surface | fill, blend mode | appearance redraw |
| opacity / visible | opacity, visible | inherited world opacity; collapsed layout differs from hidden |
| sort | zIndex | parent's ordered children, sometimes flow bounds |
| hit / cursor | hitSelf, hitChildren, hitFill, cursor | hit cache/cursor state |

Sources: [UI declarations][U-ui], [decorator setters][L-decorators], [LeafLayout flags][L-layout]. These are invalidation categories, not authorization capabilities; `editable` policy belongs elsewhere.

`LeafDataProxy.__setAttr` compares old input to new input (objects force processing), writes through the processor, then emits property changes or queues the element with its watcher's fast path. Rich old/new events depend on `trackChanges` or a special zoom-layer case. Decorator setters then mark their specific layout flags. Therefore property-event callbacks are not automatically a “layout already finished” notification ([LeafDataProxy][L-data-proxy]).

`LeafData` separates original input, intermediate/computed values, and generated rendering data. `__getInputData` serializes explicit input and tag, not all caches. `UIData.setPath` normalizes string/object input; paint inputs are normalized later and image resources recycled when replaced ([LeafData][L-data], [UIData][U-data]).

### 5.2 Rust gap and replacement contract

`DisplayObject` is a good serde value. `Scene::update_object` clones, applies an arbitrary closure, validates the candidate, then swaps it in. That guarantees rejected mutations are atomic. It does not reveal which property changed or whether geometry, paint, picking, or parent bounds need invalidation. Repeated full-scene clones/validation and traversal are visible in source; their runtime cost should be measured, not advertised from upstream benchmarks.

Recommended pure contracts, **proposed and not yet present**:

- `ElementDefinition`: kind/version, property schema/defaults, validation, geometry/measure/hit hooks, child policy, optional tool selection; native callbacks are outside serialized data
- `PropertyKey` plus `PropertyDescriptor`: type/default/validator and `Invalidation` flags. Keep built-in fields statically typed; do not replace the model with arbitrary untyped JSON
- `ScenePatch`/typed setters → validated atomic `ChangeSet` containing old/new property values and structural changes
- `ChangeSet` includes origin (user edit, layout computation, animation, import, undo) so history and observers do not confuse derived updates with new user actions
- No-op changes emit no dirtiness. Failed mutations emit neither committed property events nor lifecycle events. Nested/batched edits have one well-defined commit boundary
- Raw document access remains an explicit trusted escape hatch; runtime mutation APIs must be the path used by rendering/editor adapters if caches are to stay sound

## 6. Layout, bounds, transforms, and real partial rendering

### 6.1 Upstream ordered pipeline

A property/tree change reaches `Watcher.add/addChild`, records affected elements when partial layout is enabled, increments `changed`, and requests render when running. On `WatchEvent.REQUEST`, the watcher publishes the updated list and resets its accumulated state ([Watcher][L-watcher]).

`Layouter.layout`: START → `layoutOnce` → END. A pass requests watch data, then performs full layout on the initial pass or partial layout later. Partial layout records **before bounds**, emits BEFORE, updates matrices, updates bounds, updates appearance/state/opacity, records **after bounds**, emits LAYOUT and AFTER, and hands blocks to the renderer. Matrix updates propagate parent-to-descendant; bounds are updated deepest-first through ancestors. Additional layout passes are bounded to prevent runaway feedback ([Layouter][L-layouter], [LayouterHelper][L-layout-helper], [LayoutBlockData][L-layout-block]).

`LeafLayout.getTransform/getBounds` force pending layout before returning results; detached trees can receive a standalone full layout. Bounds are distinguished as content/box/stroke/render and by inner/local/world/page/relative-element space. `LeafMatrix` composes local layout fields, offset/pivot, parent scroll-world transform, and world state ([LeafLayout][L-layout], [LeafMatrix][L-matrix]).

`Renderer.renderOnce` requests layout before painting. The partial renderer intersects/expands damage against the canvas, clips and clears that region, and renders it. `LayoutBlockData.updatedBounds` unions old and new bounds so moving/removing content erases the old pixels too ([Renderer][L-renderer], [LayoutBlockData][L-layout-block]).

### 6.2 Rust foundation and gaps

`Affine`, inverse transforms, relative reparent preservation, local/world coordinate conversion, inherited opacity/locks, stable z sorting, transformed vector clips, and validated finite bounds already exist in `flow-scene`. These should not be discarded.

Missing contracts are cached derived state with revisions/dependencies, dirty subtree/ancestor scheduling, distinct content/box/stroke/render bounds, frame-level change coalescing, and damage tracking. Current `Scene::local_bounds` is a geometric subtree bound including stroke/clipping; `world_bounds` transforms its AABB. `visible_bounds` intersects clip AABBs, while point picking tests transformed rounded clips precisely. Do not call all these concepts the same “bounds.”

A `LayoutSnapshot` should expose explicit coordinate space and bound kind. Querying it either requires a flushed revision or performs one deterministic flush, never returns silently stale matrices. For singular transforms, continue returning errors/skip-hit semantics rather than producing infinities.

An incremental layout/damage planner would reduce CPU recomputation. **It would not by itself establish partial GPU rendering in GPUI.** The current backend still emits a whole-frame element scene. Until a verified scissor/persistent-surface backend consumes damage regions, describe the feature as incremental layout/render preparation, with full-frame GPUI painting.

## 7. Element identity, factories, clone, reparent, detach, destroy

### 7.1 Exact upstream lifecycle

`UICreator.register` indexes constructors by the prototype tag; `get` constructs the registered type and returns undefined with a warning for an unknown tag. `registerUI` is the decorator entry point. This factory is distinct from `Creator`, which supplies platform/engine services ([UI factory][L-ui-creator], [registration decorators][L-class]).

- `Leaf` allocates an internal `innerId`, constructs its data/layout processors, applies input, then emits CREATED if listeners were installed via input
- `Branch.add` converts JSON through `UICreator` if needed, removes the old parent, sets the new parent and ordered position, invalidates child box/matrix, calls parent-change observers, emits ADD, binds frame, binds Leafer/root recursively, and forwards structural change to the root watcher/event path
- `__bindLeafer` assigns root membership and depth, emits MOUNTED/UNMOUNTED, then recursively binds descendants. Animation-in may start during mounting
- Removal emits the child's REMOVE before clearing parent, unbinds root recursively, reports the structural removal, clears hit resources, and unbinds inherited frame
- `remove` does not imply destruction; `clear` destroys children. `UI.clone` serializes/deep-clones inputs, optionally overlays data, and calls the registered factory, constructing fresh runtime state
- Ordinary `add` reparents with local properties preserved. `dropTo` first computes a transform relative to the destination, then adds, preserving geometry through the explicit transform path
- `UI.destroy` clears paints and kills animation. `Leaf.destroy` removes from parent, recursively destroys children, emits DESTROY, destroys data/layout/event state, then marks destroyed. Destruction is guarded against repeated execution

Evidence: [Leaf construction/root binding][L-leaf], [Leaf destruction][L-leaf-destroy], [Branch][L-branch], [UI clone/factory/destroy][U-ui-factory], [drop helper][L-leaf-helper], [ChildEvent][L-child-events]. Some removals may be deferred by animation-out; this should be a separate optional animation lifecycle policy in Rust, not hidden inside basic tree validation.

### 7.2 Current Rust behavior

`DisplayId` is one serialized string, not separate runtime identity. `SceneNode` keeps parent/children private to validated scene operations. `Scene::reparent(..., preserve_world)` already has an explicit and useful policy. `Scene::remove` returns a flat parent-first `Vec<DisplayObject>`; parent/child edges are not included in that return type. `Clone` of a scene/object is a value snapshot with the same IDs; scene paste remaps IDs in the interaction host. These are not yet general live-instance clone/detach/destroy semantics.

`Custom { kind, bounds, contours, data }` provides safe serializable fallback geometry. `DisplayRegistry` overrides the GPUI rendering body by kind. It cannot currently create/validate a custom element kind, compute its dynamic layout, specialize hit behavior, own runtime resources, or choose inner/outer editors. Those are separate responsibilities.

### 7.3 Proposed instance contract

Introduce a runtime alongside the serializable document, tentatively `SceneEngine` in a pure `flow-engine` crate or equivalent module:

- `ElementHandle { slot, generation }` is runtime identity; `DisplayId` is document identity. Stale handles/listener tokens cannot target a replacement using the same document ID
- `ElementRegistry` resolves a versioned kind to an `ElementDefinition`. Deserialization remains inert; explicit validated hydration creates runtime instances
- `DetachedSubtree` preserves nodes, roots, ordering, IDs, and required metadata. `detach` retains a valid instance/subtree; `destroy` is terminal and cleans instance-owned subscriptions, capture, animations, caches, and resources
- `clone_subtree` has explicit ID remapping and a result map. Input state is cloned; subscriptions, live decode jobs, GPU handles, and runtime clocks are not
- `ReparentPolicy::{PreserveLocal, PreserveWorld}` remains explicit. Validate destination/cycle/depth/transform constraints before committing or emitting events
- Lifecycle notifications carry old/new parent/root and are delivered from a committed operation. Specify ordering in tests instead of emitting callbacks halfway through a fallible mutation
- Undo restores document state and rehydrates valid runtime instances; it must never revive stale listener tokens or destroyed resource jobs by copying runtime snapshots
- Removing the captured target, changing its root, or destroying the engine forces gesture cleanup and rejects queued commands targeting dead generations

An external runtime is appropriate in Rust; “instance-owned listeners” means deterministic ownership/cleanup, not necessarily callbacks stored inside serde structs.

## 8. Appearance is a compositing contract

`UIData` normalizes fill/stroke arrays and tracks complex paints, transparency, image ownership, stroke extents, effects, and whether a temporary single-compositing layer is needed. `Paint.compute` dispatches solid, image/film/video, linear, radial, and angular paints. `UIRender.__draw` defines the order of shadows, outside-stroke special cases, fills, content, inner shadows, strokes, and filters; temporary surfaces are recycled. These are backend stages, not just more fields on a style struct ([UIData][U-data], [paint compute][U-paint], [UIRender][U-render]).

A significant semantic difference: `LeafData.__checkSingle` isolates a branch with opacity below one (and other compositing cases), and `BranchRender.__render` draws ordinary groups into a temporary canvas before compositing; `LeafRender` provides the corresponding isolated element path ([LeafData 129–140][L-data], [BranchRender 31–42][L-branch-render], [LeafRender][L-render]). Our flattened `PaintItem.opacity` multiplies parent opacity into every child. Two opaque overlapping children in a 50%-opacity group can therefore accumulate approximately 75% alpha in the overlap instead of compositing the group once at 50%. The same distinction matters for overlapping fill and stroke on one element. This is a source-derived compositing gap; no pixel test was run in this research task.

Upstream masks include path, opacity-path, alpha, grayscale, and clipping variants with scope across ordered siblings. They allocate mask/content surfaces where required. A rounded frame clip is not a replacement for those semantics ([mask module][U-mask]).

Recommended render contract: a hierarchical render plan with `PushTransform`, `PushClip`, `BeginLayer`, paint operations, `EndLayer/Composite`, and explicit resource references. Backends report supported paint/composite/clip/effect capabilities before execution. Preserve real current gradient rasterization and its bounded caches, but do not silently approximate unsupported blend modes, isolation, masks, or text/image transforms. Damage bounds must include stroke/effect spread and both old/new content.

## 9. Optional plugin layers: editor, layout, state, animation, motion

These menu categories must remain visible in the architecture roadmap even if not all implemented in the first engine milestone.

### Editor and custom tools

The editor is an overlay element with selection, mask, edit box, simulated multi-selection target, and a **tool registry**. `updateEditTool` unloads the previous tool, resolves target-specific `editOuter`, applies a policy hook, loads its edit box/tool, and updates it. `InnerEditor` has create/load/update/unload/destroy and owns a view plus event handles. Opening an inner editor unloads the outer tool and emits before-open/open; closing emits before-close/close and restores outer editing. `EditTool` applies world-space transforms while batching layout ([editor install][I-editor-install], [Editor tool switching][I-editor], [inner editor switching][I-editor-inner], [EditToolCreator][I-tools], [InnerEditor][I-inner], [EditTool][I-edit-tool]).

Rust `flow-editor` provides document history/selection/transforms, not this extensible tool/view lifecycle. Keep it as the document editor. Add optional tool definitions/overlay outputs over the engine and existing command policy. Per-element resize behavior, text/path inner editing, and selection visuals should be replaceable tool contracts rather than special cases in the canvas.

### Flow auto-layout

`Flow extends Box`, defaults its flow direction to x, and marks itself auto-layout. Installing flow registers property categories and supplies Box flow/content/bounds functions. Row/column algorithms measure eligible children, account for padding, grow/percent sizing, range constraints, gaps, wrap, alignment, reverse order, and second-pass auto sizing. Layout writes suppress ordinary change notifications during computation ([flow install][I-flow], [Flow class][I-flow-class], [flowX][I-flow-x]).

Rust Box/Frame are presently geometry + children + clipping. A proposed optional `LayoutProvider` should return measure/arrange outputs into derived state, with bounded convergence and no undo entry per child per pass. `flow-runtime`'s workflow DAG scheduling is unrelated to visual row/column flow layout.

### State styles and focus

The state plugin declares normal/hover/press/focus/selected/disabled styles and named states. It installs `State.updateEventStyle`; enter/leave set/unset hover, down/up set/unset press. Focus has a single interaction owner and is changed through the state layer. Temporary style changes can lock the normal style so transient effects do not overwrite persistent input ([state plugin][I-state], [state event update][I-state-events]).

Rust selection and cursor state do not constitute a style cascade. Add a deterministic style resolver with explicit precedence and transient state store. Its output goes through appearance/layout invalidation; listeners and effects must clean up on detach/destroy/focus loss.

### Animation, transitions, and motion paths

The animation plugin registers attribute interpolators, animation/transition properties, and methods that create `Animate`/`AnimateList`. `Animate` owns time, easing, delay, speed, loops/reverse, play/pause/seek/kill, and schedules frames through `Platform.requestRender`. Each update computes an interpolated style and writes through `target.set`; destruction releases its target and timers. Property conflicts when starting a new animation are explicitly considered by `killAnimate` ([animation install][I-animate-install], [Animate][I-animate]).

Motion-path is a separate extension: registers motion interpolators/properties, caches measured path data, samples position/rotation by distance, and updates dependent elements or reveals a path. It uses layout/matrix hooks, not workflow execution ([motion path][I-motion]).

Proposed Rust contracts: injectable deterministic `Clock`, `AnimationTrack`/`InterpolatorRegistry`, transient property overrides, start/update/complete/cancel events, and explicit bake-to-document behavior. Test with fake time. Media playback, workflow scheduling, and visual animation may share a clock abstraction but must not be presented as interchangeable implementations.

## 10. Engine/application lifecycle and platform boundary

`Leafer.init` creates canvas, renderer, watcher, layouter, selector, interaction, and managers through `Creator`. `start/stop` act on controllers; layout locking is distinct from stopping the entire engine. Creation/readiness are separate: first render start marks created; layout end marks ready; render end marks view ready; resource completion plus a later render marks view complete. Destruction stops controllers, emits end, removes subscriptions, destroys controllers/managers/canvas, and then destroys the element subtree ([Leafer][U-leafer]).

`App` composes ground/tree/sky Leafer layers, installs an editor in sky when configured, aggregates readiness, shares platform resources, and forwards layout/render/lifecycle activity ([App][U-app]). These abstractions are absent from a single `SceneView` even though GPUI itself has entity/window lifecycles.

A Rust engine should expose a platform-neutral lifecycle, frame request interface, and resource ownership scopes. The GPUI adapter should translate native coordinates/buttons/focus and submit a render plan; it should not own a second independent click/hover/default-action state machine. Native draw completion and asset readiness should be reported honestly: if GPUI offers only frame scheduling rather than GPU completion, name the event accordingly.

## 11. Menu breadth coverage, with honest scope

| User-facing category | Architectural home | Existing Rust scope | Planned contract / remaining work |
|---|---|---|---|
| id/name/class/data/parent/root | Element document + live instance | IDs/name/classes/data/tree | Generation-safe runtime/root membership, query indexes |
| create/clone/import/export | Registry + serialization | serde scene + paste remapping | Factories, clone policies, detached subtree; image/SVG export separately |
| remove/destroy | Instance lifecycle | subtree deletion | Detach vs terminal destroy, scoped resources and subscriptions |
| position/scale/rotate/skew/offset/origin | Transform/layout | affine composition + editor transforms | Explicit transform property schema, pivot/offset API, computed snapshots |
| width/height/resize/bounds/coordinates/z | Layout + per-kind resize + ordering | primitive sizes/bounds/reparent/z | Bound kinds, measure/arrange, constraint-aware per-kind resizing |
| opacity/visibility/fill/stroke/gradient | Style + compositing | solid/linear/radial, opacity, round stroke | Group isolation, layered paints, stroke alignment/cap/join/dash, collapsed visibility |
| image/texture/shadow/fade/cutout/clip/erase | Resource and render-effect providers | image element; vector rounded clips; media | Image paint transforms/alpha hits, masks, blend/erase, effects and resource events |
| hit/edit/drag/cursor | Picker + policy + default-action plugins | implemented bounded graph/scene plugins | Per-element hit/edit traits, pointer kinds/through targets, drag bounds, consistent router |
| state/hover/press/focus | State-style plugin + router | selection/cursor only | State cascade and stable enter/leave/focus transitions |
| interaction/element/platform/base events | Event hub + adapters | scene phases, mouse events, host effects | Typed families, scopes, commands from listeners, consistent prevention/order |
| engine/app/create/style/lifecycle/config | Scene engine + app composition | GPUI view wrappers | Instance/controller lifecycle and resource readiness; layered surfaces |
| zoom/navigation | Optional viewport interaction | pan/zoom/fit | bounds/inertia/multi-touch policies as separate features |
| partial rendering | Derived state + backend | full-frame GPUI paint | First incremental CPU layout; GPU damage consumption only when implemented |
| editor props/config/events/tools/inner editor | Document editor + optional tool layer | selection/history/basic transforms | Custom outer/inner tool registry and lifecycle, typed edit events |
| flow layout/child properties | Optional layout provider | no scene flow engine | row/column/wrap/grow/constraints and child measure policies |
| animation/transitions/motion path | Optional animation/motion providers | no scene property animation | deterministic time/interpolators/tracks/path sampling |
| force render/query/custom properties | Engine diagnostics/schema/indexes | scene name/class scan; GPUI notify | explicit invalidation/flush, indexed query, validated custom props |
| math/lists/resources/debug/plugin | Pure utilities/runtime services | geometry, caches, plugin command host | service scopes, diagnostics, declarative dependencies; avoid an all-purpose global registry |

This table is a coverage map, not a promise that every menu item is already implemented or a fixed delivery estimate.

## 12. Prioritized implementation and executable acceptance contracts

### Phase A: live instance and registry foundation (first)

Proposed modules: `flow-engine/{engine,instance,registry,lifecycle,subtree}.rs`, with `flow-scene` retained as the inert document/geometry layer. Names can change, but responsibilities must remain separate. Engine depends on scene; optional editor/input adapters depend on engine interfaces. Do not make the engine depend on GPUI or the workflow executor.

Deliverables:

1. Versioned element definitions with built-in registrations and one custom element exercising factory, schema, dynamic geometry, picking, and lifecycle
2. Generational handles and engine-scoped event/resource ownership
3. Atomic attach/detach/reparent/destroy/clone with an intact `DetachedSubtree`
4. Explicit committed lifecycle records; host drop/engine stop cleanup

Acceptance tests:

- **A01** Unknown kind has an explicit inert fallback or error policy; JSON cannot execute callbacks during parse
- **A02** Built-in and custom factory create equivalent validated documents; bad custom data fails atomically
- **A03** Detach preserves exact subtree edges/order and instance identity; reattach restores root membership
- **A04** Destroy is idempotent, invalidates descendants and listener tokens, releases owned resources exactly once
- **A05** Reusing a document ID never lets an old handle/listener/job completion target the new instance
- **A06** Clone creates fresh runtime identities and remaps document references without copying subscriptions/jobs
- **A07** Reparent preserves chosen local/world geometry under rotation, negative scale, and nested transforms; singular destination/cycle/depth errors leave state and event log unchanged
- **A08** Removing/destroying a captured element cancels the gesture and clears overlays/pressed state; undo does not revive dead tokens

### Phase B: property transactions and derived update pipeline

Proposed modules: `property`, `change`, `watcher`, `layout`, `snapshot`, `damage`. Every engine mutation produces a change set; arbitrary document mutation must invalidate/reconcile explicitly.

Acceptance tests:

- **B01** Same-value setter emits no delta; invalid setter emits no committed event and no partial mutation
- **B02** Multi-property batch preserves first old/last new value, validates once at the intended boundary, and yields one coherent revision
- **B03** Fill-only change invalidates paint, not unrelated geometry; width affects own bounds and ancestor aggregation; parent transform updates descendants; z changes parent ordering
- **B04** Input serialization excludes computed defaults/caches/runtime handles, round-trips custom data/version
- **B05** Bounds/transform query immediately after mutation returns the latest committed geometry via an explicit flush contract
- **B06** Dirty layout equals a full reference recomputation across nested random transforms and reparenting
- **B07** Damage includes old and new stroke/effect bounds for move/resize/remove/hide; viewport/DPI invalidation is separately accounted for
- **B08** A layout provider requesting another pass reaches a bounded fixed point or returns a diagnostic; it does not recurse forever
- **B09** Benchmark and counters show unaffected branches are not recomputed. Report measured CPU work; do not call this partial GPU paint

### Phase C: one input/tree/default-action router

Reuse the existing command gates, gesture rollback, and plugin lifecycle. Add a router independent of GPUI. Use an event/tree adapter for graph-specific node/port/edge targets; do not silently force workflow topology to become the scene hierarchy. Specify explicit pointer-down target vs current hover target vs captured target.

Acceptance tests:

- **C01** Exact capture → target capture → target non-capture → bubble order, with the documented target phase; stop-propagation differs from immediate-stop
- **C02** Once/off/scoped removal/destroy work during queued reentrant operations; newly added listeners follow a documented snapshot policy
- **C03** Prevent-default on down/move/up/wheel/key suppresses the corresponding default commands before mutation/commit; cleanup still runs
- **C04** Over/out and enter/leave differences use old/new ancestry correctly; moving between siblings does not spuriously leave/reenter their common ancestor
- **C05** Up outside still reaches the required down/captured targets; cancel/focus loss/missing or mismatched button does not click, drop, or commit accidentally
- **C06** Event position contains screen/world/current-local coordinates plus pointer identity/type/buttons/modifiers/time; resize/connection tolerances remain fixed in screen pixels
- **C07** Removing/reparenting an element during dispatch uses a fixed path and queued mutation rules, without use-after-destroy or duplicate delivery
- **C08** Read-only gates apply equally to listener-produced commands, plugins, shortcuts, tools, paste/history/import/data edits; allowed navigation/selection remains configurable
- **C09** Plugin disable/remove/replace during gestures cancels safely; disabled selection cannot make drag move unrelated old selection
- **C10** Native GPUI regression tests compare adapter event trace with pure-router trace, covering canvas blur and release outside

### Phase D: render-plan correctness and resource lifecycle

Acceptance tests:

- **D01** Overlapping opaque children in a half-opacity group produce half-opacity composited output, not independently accumulated opacity
- **D02** Multiple paints/fill+stroke isolation and blend scopes preserve explicit ordering
- **D03** Nested transformed clips/masks have matching rendered pixels and picking policy; unsupported backend features produce visible diagnostics
- **D04** Existing multi-stop linear/radial gradient tests, raster memory caps, cache lifetime, zoom/DPI behavior continue passing
- **D05** Async asset completion after destroy or source replacement is discarded by generation/revision; resource leases release on detach/destroy according to policy
- **D06** Only a backend that actually consumes persistent damage regions may claim partial painting; full-frame GPUI remains an explicit capability

### Phase E: independent optional feature providers

Implement vertical slices on the preceding contracts rather than adding inert fields:

- **E01** Row/column flow layout: padding/gaps/wrap/grow/auto sizing/hidden-vs-collapsed; nested measure convergence and no history spam
- **E02** State style resolver: hover/press/focus/selected/disabled precedence, restoration of base values, detach cleanup
- **E03** Custom outer/inner editor tool: registry selection, load/update/unload/destroy ordering, policy gating, resource cleanup
- **E04** Animation: fake-clock pause/seek/reverse/loop/conflict/cancel/destroy, property invalidation, optional bake into one history transaction
- **E05** Motion path: distance/tangent sampling under transforms, changed-path invalidation, animation integration
- **E06** Advanced appearance/query/export/drag-bound features added only with renderer/behavior implementations and focused tests

## 13. What to preserve, what to avoid

Preserve validated pure documents, finite affine math, per-primitive files, exact vector hit/clipping, real gradients with bounded resources, typed graph port rules, command capability gates, screen-space snapping, and transactional undo/rollback. These are substantive existing capabilities.

Avoid a new enum or boolean for every screenshot label without an executable contract; a single universal plugin callback owning all engine concerns; serializing callbacks/resources; conflating detach with destroy; copying runtime listeners into undo snapshots; using one AABB for every bound concept; claiming workflow scheduling is visual flow layout; claiming damage bookkeeping is partial GPU rendering; or labeling a custom renderer as a complete custom-element system.

The smallest convincing next milestone is **one registered custom live element** that can be created, mounted, observed, changed through metadata, laid out, picked, rendered, cloned, detached/reparented, and destroyed with verified cleanup. Then route existing graph/scene editing through that engine instead of making another demo-specific event layer.

## 14. Verification of this research

- Cloned and pinned the three official repositories; checked clean working trees
- Read license files and compared all discovered package-license hashes
- Inspected source implementation, not only reference-menu pages, for the contracts and order described above
- Inspected the current Rust source paths and distinguished existing behavior, inferred gaps, and proposed APIs/tests
- No Rust changed; no upstream scripts, builds, package installs, or tests executed
- Proposed tests above are an acceptance plan, not passing-test claims

## Pinned source references

[I-animate]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/Animate.ts#L114-L527
[I-animate-install]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/index.ts#L19-L85
[I-edit-tool]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/EditTool.ts#L21-L88
[I-editor]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/Editor.ts#L154-L205
[I-editor-inner]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/Editor.ts#L317-L368
[I-editor-install]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/index.ts#L31-L66
[I-flow]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/index.ts#L13-L110
[I-flow-class]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/Flow.ts#L7-L22
[I-flow-x]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/flow/src/layout/flowX.ts#L20-L103
[I-inner]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/InnerEditor.ts#L45-L126
[I-license]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/LICENSE
[I-motion]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/motion-path/src/index.ts#L12-L142
[I-state]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/index.ts#L39-L104
[I-state-events]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/state/src/event.ts#L8-L22
[I-tools]: https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/editor/src/tool/EditToolCreator.ts#L9-L35
[L-branch]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display/src/Branch.ts#L71-L181
[L-branch-render]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/BranchRender.ts#L10-L68
[L-child-events]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/event/src/ChildEvent.ts#L6-L23
[L-class]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/decorator/src/class.ts#L4-L13
[L-data]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/data/src/LeafData.ts#L5-L150
[L-data-proxy]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/LeafDataProxy.ts#L12-L75
[L-decorators]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/decorator/src/data.ts#L18-L361
[L-event]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/event/src/Event.ts#L5-L39
[L-event-creator]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/platform/platform/src/creator/EventCreator.ts#L8-L36
[L-eventer]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/event/src/Eventer.ts#L27-L173
[L-layout]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/layout/src/LeafLayout.ts#L13-L381
[L-layout-block]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/layouter/src/LayoutBlockData.ts#L7-L37
[L-layout-helper]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/layouter/src/LayouterHelper.ts#L11-L75
[L-layouter]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/layouter/src/Layouter.ts#L54-L160
[L-leaf]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display/src/Leaf.ts#L120-L199
[L-leaf-destroy]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display/src/Leaf.ts#L491-L526
[L-leaf-helper]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/helper/src/LeafHelper.ts#L238-L240
[L-license]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/LICENSE
[L-matrix]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/LeafMatrix.ts#L9-L47
[L-picker]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/selector/src/Picker.ts#L25-L184
[L-plugin]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/debug/src/Plugin.ts#L6-L27
[L-render]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/display-module/display-module/src/LeafRender.ts#L8-L58
[L-renderer]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/renderer/src/Renderer.ts#L116-L212
[L-rewrite]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/decorator/src/rewrite.ts#L18-L58
[L-ui-creator]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/platform/platform/src/creator/UICreator.ts#L8-L31
[L-watcher]: https://github.com/leaferjs/leafer/blob/1d703303ff0759ce4973d54c87eeb8192dddc570/packages/partner/watcher/src/Watcher.ts#L35-L126
[U-app]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/app/src/App.ts#L9-L151
[U-data]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/data/src/UIData.ts#L12-L189
[U-dragger]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/Dragger.ts#L41-L240
[U-emit]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/emit.ts#L8-L68
[U-interaction]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/Interaction.ts#L96-L202
[U-interaction-helper]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction/src/InteractionHelper.ts#L9-L79
[U-leafer]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/Leafer.ts#L98-L484
[U-license]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/LICENSE
[U-mask]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/mask/src/index.ts#L11-L113
[U-paint]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/partner/paint/src/Compute.ts#L10-L103
[U-pointer-event]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/event/src/PointerEvent.ts#L7-L48
[U-readme]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/README-EN.md#L124-L134
[U-render]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display-module/render/src/UIRender.ts#L11-L114
[U-ui]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/UI.ts#L13-L355
[U-ui-event]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/event/src/UIEvent.ts#L8-L56
[U-ui-factory]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/display/src/UI.ts#L551-L585
[U-web-interaction]: https://github.com/leaferjs/leafer-ui/blob/1e11641d3ea6b0211c83447268a90b8d58feec25/packages/interaction/interaction-web/src/Interaction.ts#L42-L280

## Text crate extraction (same behavior)

`gpui-flow-text` is a standalone dependency-free measurement contract with an
optional `gpui` native adapter. Layout and UI preserve their previous public
exports. Both measurement and painting call the same existing gpui-fast shaper;
no new typography or layout engine was introduced. See
[text measurement crate boundaries](text-measurement.md#crate-boundary-and-upstream-reuse).


当前新增[有界叶节点多层内阴影](inner-shadows.md)。支持范围、真实paint alpha与Leafer几何shape的差别、fill/inner/stroke顺序、完整有界文字源和拒绝规则见该契约；此前阶段的内阴影缺项记录保留为历史验证范围。

Current [bounded container inner shadows](container-inner-shadows.md) extend the
leaf slice to Group/Box/Frame, preserving explicit paint phases, actual subtree
alpha, independent own effects and existing budgets. See that contract for the
current validation scope; older slice receipts above remain historical.
