# 显式时钟动画底座

可选 `gpui-flow-animation` 在已有 SceneEngine 上实现真实属性插值和播放控制。输入定义与运行期状态分离；不启动后台线程、定时器、网络调用或隐式引擎监听器。

## 实际支持

- 平移 tx/ty：保留缩放/旋转/skew等其他仿射系数
- opacity：有限数值且范围0..1
- 现有实色基础fill的RGBA颜色：编码通道线性插值、四舍五入到u8；不是线性光或感知色彩空间插值
- 已注册CustomDefinition明确声明的Number属性：默认值解析、范围/schema校验、几何重建、bounds/hit/paint更新走同一引擎管线
- Linear、三次EaseIn/EaseOut以及smoothstep EaseInOut四种明确easing
- 显式tick、pause、resume、seek、cancel/keep、remove和目标生命周期保护
- 1..64段显式时长关键帧序列，每段easing、稀疏字段保持、跨段seek与原子大步tick

`AnimationSpec` 保存duration_ms、easing和目标属性值；开始时捕获当前有效输入作为from。规范可以JSON保存，运行期句柄、控制器ID、时钟、缓存与当前播放状态不进入文档。

## 有界关键帧序列

`animate_sequence(engine, target, AnimationSequence::new(Vec<AnimationSpec>))`
将多个目标关键帧注册成一个动画ID。每段明确提供正的duration_ms、easing及目标tweens。
第一段从注册时当前有效值开始，后续从前段累计姿态开始；省略的属性保持前值，直到包含
它的段才开始插值。全部属性的并集从序列开始就被该记录占用，防止后段突然覆盖新动画。

- 1..64段，每段1..32个不重复属性，整序列最多32个不同属性；累计毫秒数使用checked加法
- 开始前验证每一段的完整累计端点，包括custom schema和同步响应布局所有权；任一晚段
  无效就拒绝整条记录，不先注册前面有效的部分
- seek可以跨任意多段并精确取边界；大delta直接采样最终时间处的累计姿态，仍只有一次
  引擎提交/通知。tick不会向外暴露被跳过段的中间姿态，也没有逐段事件回调
- 每段easing只作用于段内归一化进度。完成后seek回中间仍变Paused，恢复后继续剩余段
- 单段`animate`沿用相同实现和旧API；整个序列沿用相同的pause/resume/cancel、外部属性
  覆盖保护与编辑器PlaybackToken，完成一次Undo，取消保留更早Undo/Redo
- Runtime保存归一化关键帧值，不受调用者随后修改配置影响。AnimationSequence可JSON
  保存；运行进度、句柄、layout配置和播放事务仍不序列化

这对齐上游`Animate.create/setBefore/animate/seek`的累计样式、段内时间与补缺属性契约，
但这里明确从当前姿态开始每个正时长目标序列。没有上游默认的零时长起始pose/自动分配
时长、delay、loop/swing、speed、ending策略、AnimateList或嵌套timeline。不是完整
Leafer关键帧API兼容声明。

## 原子更新

每次 `tick(engine, Duration)` 将所有playing记录在私有副本中推进，计算本帧属性，然后用 `SceneEngine::set_properties_batch(ChangeOrigin::Animation, ...)` 一次提交。多个元素和多个属性共用一个revision/ChangeSet。中间插值或自定义几何无效时，文档与存活动画的时钟不改变；没有先动一部分再报错的半帧。

`validate_properties_batch` 用于开始时验证终点，且不提交事件/revision。用户定义的工厂必须保持纯函数；它们对外部世界的副作用仍不是引擎能够回滚的事务。正常引擎校验和候选文档复制仍存在，不宣称O(变化节点)复杂度或大规模性能。

零delta和相同有效值不产生文档变化。捕获的隐式Custom默认值在零进度时不会被无意义地写入输入。每次成功提交后重新读取规范化的数值，避免f32 opacity舍入被误判为宿主覆盖。

## 播放状态

- Playing：tick累计正Duration并限制到duration；到终点进入Completed
- Paused：tick不累计时间，seek仍实际修改属性；resume恢复
- seek作用于Playing/Paused/Completed记录；完成后seek回区间内变为Paused，需resume继续；seek到终点进入Completed
- 裸engine的cancel保持当前输入；编辑器cancel_editor按下方会话规则恢复基线。已取消记录不能seek/resume复活
- remove释放记录；完成和取消记录最多共保留256项，每项最多32个属性，达到上限明确报错
- Duration为类型化非负时长；零duration定义被拒绝，大delta安全停在终点

没有延迟、循环、反向/swing、速度变化、自动时长分配、动画列表编排、弹簧/物理插值或运动路径。`seek`是给单个动画的原子操作，批量seek多个记录需要宿主协调。

## 目标与属性归属

控制器绑定唯一engine，动画保存带世代的ElementHandle。错误engine、外来animation ID和已移除记录都被拒绝。destroy、detach或reset导致目标不可用时，在下一次tick/sync/info/播放操作时取消，不写入同名替代对象。attachment_epoch也随记录捕获，detach和reattach即使完整发生在两次tick之间也会取消；reattach不会隐式恢复已取消记录。核心模块不安装生命周期回调，宿主在停止tick期间也应在文档变动后调用sync，以立即反映取消状态。

同一控制器的playing/paused记录不能同时占用同一目标属性；不重叠属性可以并行。完成记录seek回活动区间前需要重新取得属性归属。多个控制器没有全局优先级调度，应由宿主协调；它们把彼此的写入视为外部覆盖。

当宿主直接编辑某个被动画拥有的有效属性，下一次sync/tick会取消该记录，保留宿主值。编辑其他属性不取消动画。例如平移动画保留宿主新写入的旋转矩阵系数。取消保持当前值而不是猜测是否应该覆盖用户编辑。

Drop控制器只释放自己的有界数据，不留下线程、定时器或引擎订阅。暂停/完成记录仍占一个槽位，宿主不需要它时应remove。

## 与编辑器和原生宿主组合

编辑器默认使用 `begin_playback()` 取得不可伪造的PlaybackToken，再调用 `tick_editor` / `seek_editor`。引擎属性与缓存仍通过同一个原子batch更新，编辑器同时记录最后一帧；不增加逐帧Undo条目。

- `finish_editor`保留最终姿态，仅提交一个动画Undo步骤。Undo回到播放前用户编辑，再次Undo继续更早历史
- `cancel_editor`恢复播放前文档，保留已有Undo/Redo分支；选择与viewport等宿主状态保持当前值
- 普通transaction/set_property或开始用户拖动会先结束并提交已有播放，再单独记录新编辑。旧token的tick/seek/cancel立即失效，不能覆盖新输入
- 活动播放期间的直接engine写入也会被对账：最后一个拥有的动画帧、新外部编辑分别形成Undo步骤。stale cancel不会自动恢复已被宿主删除的元素；显式Undo可以按常规编辑语义恢复文档，并分配新的运行期身份
- Undo/Redo在播放中会先结束会话，再执行历史操作。零变化播放不制造额外Undo条目
- 显式新文档load/reset继续清除旧文档历史；独立document_epoch保证后续写入不会掩盖reset边界
- 活动用户手势与播放不能同时取得事务所有权；外来/陈旧token被拒绝。Drop控制器不删除编辑器保留的基线，宿主仍可结束或取消它

裸engine的tick/seek/cancel保留给自主管理历史的宿主；将它直接用于 `editor.engine` 而不取得播放所有权，仍属于原有可信外部写入协议。编辑器集成示例和推荐入口均使用上面的安全会话，不把丢失旧历史当成已接受限制。

动画与属性入口仍是可信宿主API，不替代只读权限策略。历史容量遵循SceneEditor已配置的上限；播放控制器不承诺无限保留旧操作。

原生 `animations` 示例用 `Window::on_next_frame` + `Instant` 提供时钟，演示两条各两段/总2秒的关键帧序列，内置Restart/Pause/Resume/Seek 50%/Finish/Cancel/Undo/Redo/Destroy circle。完成自动提交一个播放步骤，取消回到基线；Destroy circle是独立编辑，可以Undo。每帧只排一个弱引用回调，窗口关闭不会靠回调保活整个视图。系统减少动态效果等偏好由宿主决定，核心不依赖平台设置。

```bash
cargo run --locked -p flow-studio --bin animations
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin animation
cargo test --locked -p gpui-flow-animation
```

原生适配代码的编译/链接不等于已经执行真实窗口视觉验收。paint-only状态样式已作为独立呈现层接入，见[state-styles.md](state-styles.md)；binding级fill/opacity自动hover/pressed过渡已实现；高级过渡、完整timeline、运动路径仍是后续模块。

## 官方源码定位

参考的 `leafer-in` 固定版本为 `3eb445bf34debb638766615282224b77108bf610`：

- [Animate](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/Animate.ts)：真实时间推进、播放/暂停/seek和样式插值职责
- [Transition](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/Transition.ts)：按属性选择数字、颜色及其他插值器
- [动画安装与属性重叠处理](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/index.ts)：注册动画/transition属性、替换冲突属性动画
- [AnimateList](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/animate/src/AnimateList.ts)：本批尚未实现的列表/编排层

Rust实现独立编写，以显式宿主时钟、类型化属性和单一原子engine批次替代JS原型改写。上游的完整API、global属性系统和动画选项不在兼容承诺内。
