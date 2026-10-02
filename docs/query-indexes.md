# 附着元素查询索引与 CPU 缓存验收（DAT-04）

这是固定有界数据能力，不是 Leafer 的 JavaScript API 兼容层，也不是 GPU 局部绘制。固定参考为 [Finder.ts](https://github.com/leaferjs/leafer-in/blob/3eb445bf34debb638766615282224b77108bf610/packages/find/src/Finder.ts)：目标/branch 本身参与深度优先查询、id/className/tag 查询、移除和 id 变化时缓存失效。本文对应的实现直接读取该固定提交；未引用浮动 main。

## 固定条件与顺序

`SceneEngine::query(ElementQuery)` 返回当前附着节点的 `ElementHandle`。`query_with_stats` 同时返回候选节点检查次数和匹配数。查询不会修改场景、revision、历史或索引。

```rust
use gpui_flow_engine::{ElementQuery, QueryOrder};

let matches = engine.query(ElementQuery {
    name: Some("button"),
    tag: Some("rect"),
    class: Some("interactive"),
    subtree: engine.handle(&"toolbar".into()),
    order: QueryOrder::Paint,
    ..Default::default()
})?;
```

注意：若必需的 subtree 不存在，调用者应先检查 `handle` 的 `Option`，不能把 `None` 当成空子树。`None` 表示整个已附着 forest。

- `id` 精确匹配文档 `DisplayId`；它不是实例 generation，也不缓存旧 handle
- `name` 精确匹配 `DisplayObject::name`，包括空字符串
- `tag` 精确匹配小写 `Shape::tag()`，例如 `rect`、`frame`、`text`、`custom`。Custom 的注册 kind 不被悄悄当成 tag
- `class` 匹配 `DisplayObject::classes` 中单个完整成员；不拆词，不解析 `.class`、`#id` 或 CSS
- 所有字符串大小写敏感；同时提供的条件按 AND 组合。没有条件就是全部附着节点
- `subtree` 使用本 engine 的存活附着 handle；默认包括根。`include_root: false` 只排除该 scope 根。外来、已销毁、reset 前的 handle 拒绝；分离根返回 `DetachedHandle`，即使其他条件没有匹配也先验证 scope
- `Tree` 是 roots/children 插入顺序的深度优先前序；`Paint` 在每层按 z-index 稳定排序，平级相同 z-index 保持插入顺序，仍为父先于子
- 两种查询都包含隐藏、锁定、透明和被裁切节点。`Paint` 只选排序，视觉可见性仍由 `paint_list`/hit APIs 负责

`name`、多 class 集合、复合 AND 条件和可选 Paint 顺序是 Flow 明确的类型化契约。固定上游 `getBy` 的 id/tag/className 分支有优先级，且 className 是单字符串；这里不声称复刻这些 JS 调用约定。现有 `Scene::find_by_name` / `find_by_class` 不变，它们按 Scene 的 id map 遍历；新引擎 API 的顺序契约独立且明确。

## 生命周期与原子提交

运行期索引包含 name/tag/class 的 `BTreeMap<String, BTreeSet<DisplayId>>`、树/paint 顺序和子树半开区间；rank/范围的 `positions` 使用只做 key lookup 的 std `HashMap`，输出顺序仍完全由显式 rank 决定。只保存文档 id，不保存任何 handle。结果在查询时解析当前实例身份，避免 id 重用、种类替换和 reset 后指向旧对象。

`apply_validated` 先完成 schema、派生布局、调用者校验和布局缓存构建，再在发布通知前同步索引。只有 dirty id 的元数据会被检查，只有实际改变 name/tag/classes、加入或移除的节点更新 bucket。树结构或 z-index 改变重建顺序/区间；仅 paint、仿射或元数据更改保留顺序缓存。`paint_list` 和反向 hit 遍历复用这个已提交的 paint 顺序。

- create/clone 加入新文档 id；reparent/reorder 更新子树范围和顺序
- detach 将整个子树移出查询，但实例仍被 engine 保留；attach 重新加入并保留原 handle
- destroy 删除 bucket/范围；再次创建同 id 返回新 handle
- `replace_scene`（UndoRedo 恢复通道）同步所有恢复的条件和树顺序；仍存活实例保持身份，已删除再恢复的实例不复活旧 handle
- reset 退休所有旧附着/分离身份；即使文档完全相同，查询也解析新 handle
- `destroy_detached` 不改变附着索引，并将该次提交的索引工作计数清零
- 失败批次、非法 reparent、无效 scope 和等值编辑都不会部分改写索引；no-op 保留上一实际提交的 `query_index_stats`
- 索引、统计和 handle 不进入 Scene JSON/Undo 文档

## 自动对照

`crates/flow-engine/tests/query.rs` 有 8 个测试：精确/组合条件、两种稳定顺序、scope 边界和错误优先级、元数据/kind/z-index 更新、detach/remount/reparent/clone/destroy/id 重用、UndoRedo/reset/销毁分离树、晚失败批次和 no-op，以及候选工作量。

其中 seeded 对照使用 3 个固定 seed（`1`、`0x5eed`、`0xdeadbeef`）各 100 步。每步都与不读索引的完整树递归扫描比较；覆盖 metadata、z-index、重挂载、历史恢复、种类替换、仿射和 reset，并检查不同 scope 的所有 id、name/tag/class 组合和两种顺序。随机选择使用 LCG 高位，避免低位短周期把某种操作固定为同一分支。

`crates/flow-engine/tests/layout.rs` 另有 3 个固定 seed（`0x5eed`、`0xcafef00d`、`0x12345678`）各 160 步，41 节点、嵌套 Group/Frame、负比例、旋转、双轴 skew、平移、叶/父变换、普通/保持 world 的 reparent、整子树 detach/remount、尺寸和继承状态、clip 类型替换、UndoRedo/reset。每一步（分离中间状态也包括）比较：

1. Scene 独立完整计算的 world transform、local/world bounds、paint 列表与 clips
2. 完全丢弃缓存后新建 engine 的所有 LayoutEntry 字段（仅 revision 不要求相同），包括 inverse、content/effect bounds、继承 opacity/visibility/lock/hit 和 clips
3. 固定世界点的完整 Scene hit 结果，含锁定/不可见选项

现有固定 fixture 的颜色修改零 matrix/bounds 重算、叶/父变换传播、移除损伤、clip 失效等回归继续保留。

验证命令（从 workspace 根运行）：

```sh
CARGO_BUILD_JOBS=1 cargo test --locked --offline -p gpui-flow-engine --lib --tests
CARGO_BUILD_JOBS=1 cargo clippy --locked --offline -p gpui-flow-engine --all-targets -- -D warnings
CARGO_BUILD_JOBS=1 cargo test --locked --offline -p gpui-flow-engine \
  --test layout benchmark_query_cache_cpu -- --ignored --nocapture
CARGO_BUILD_JOBS=1 cargo test --locked --offline -p gpui-flow-engine \
  --test layout benchmark_commit_pipeline_cpu -- --ignored --nocapture
```

2026-10-02 纯引擎回归结果：154 passed、0 failed、2 ignored。两个新增 ignored 分别是查询/缓存 CPU benchmark 和同工作量 commit baseline；显式执行结果单列，不把 ignored 算为通过；clippy all-targets / `-D warnings` 通过。这里不代表同一代码全 workspace/native 窗口已验收。本项为纯数据项，无窗口前置条件。

## 可复现实测和工作量

benchmark 是上面 tests/layout.rs 中固定 `benchmark_query_cache_cpu`，不加入新依赖或统计阈值。545 节点（1 root + 32 groups + 512 rect leaves），10,000 次精确 name 查询和 100 次同一叶子的 transform commit。对照 scan 遍历全部 545 个 Scene nodes、执行相同 name 条件、解析相同结果 handle；因为只有一个匹配，不存在树/字典顺序差异。cold build 每次从当前完整文档 clone 创建新 engine，完整重建布局、索引和身份，不是另一个等价 transaction 实现。

2026-10-02，同一 Linux x86_64 容器、AMD EPYC 9V74、rustc 1.98.1 (`48a229cea`, 2026-09-01)。默认 dev/test profile，engine 未优化；workspace 配置令 flow-scene opt-level=2。单 cargo job，`CARGO_INCREMENTAL=0`、`CARGO_PROFILE_DEV_DEBUG=0`，缓存依赖，连续 3 次测量；非专用隔离机器，没有 GPU 计时。数值为微秒：

| 样本 | 10k indexed query | 10k full scan | 100 incremental commits | 100 cold engine builds |
|---|---:|---:|---:|---:|
| 1 | 12,779 | 277,803 | 3,273,102 | 751,281 |
| 2 | 12,707 | 269,053 | 3,374,507 | 778,420 |
| 3 | 12,300 | 279,320 | 3,320,437 | 806,838 |

每次测量的确定性计数完全相同：

| 工作 | Indexed / incremental | Full scan / cold build |
|---|---:|---:|
| 查询候选检查（10k 次） | 10,000 | 5,450,000 |
| 布局节点访问（100 次） | 54,500 | 54,500 |
| world 重算（100 次） | 100 | 54,500 |
| local bounds 重算（100 次） | 200（两层祖先） | 54,500 |

每个 transform commit 的 index metadata_updated=0、order_nodes_rebuilt=0；随后一次 rename 的 metadata_examined=1、metadata_updated=1、order_nodes_rebuilt=0，且 world/bounds 重算均为 0。基准中这些都是断言，不只是打印数字。

结论只限该固定实验：选择性查询减少了候选检查，缓存减少了矩阵/bounds 重算。**整体 incremental commit 在此 dev 测量中比 cold build 更慢**，因为事务还承担文档 clone、schema/验证、diff、变更/生命周期整理和全树访问；不能从少量几何重算推出整体提交加速。

## 同工作量的索引前/后 commit baseline

cold build 不是等价事务，因此另用 `benchmark_commit_pipeline_cpu` 比较新增索引前后的**相同 commit pipeline**。该测试仅使用索引前就存在的 API，固定 545 节点，每类操作从独立 fresh fixture 开始，各 100 次真正改变文档的 transform、rename、reparent；fixture/engine 初始化和结束的完整 Scene oracle 验证不在计时区间。两版本逐操作的 LayoutStats、revision、最终 handle 和完整 bounds/world reference 一致才算通过。

索引前来源：本地 checkpoint `6edddbbb41cd99a5bc5f0c02928dcf92284cec3b`，tree `ed04add17394263c367cd081d91ff6a214d8a8b0`；它对应发布时已通过 GitHub API 回读的远端 `17f92d2fec9b136027eceb3c8d8a063fc84cb013`，tree 相同。没有声称重建工作区拥有这个远端 commit 的本地 git 对象。

测量使用两份独立 `git archive` 快照，索引后版本只覆入 `engine.rs`、`lib.rs`、`query.rs` 三个生产文件。逐字节比较确认 flow-core、flow-scene、Cargo.toml/Cargo.lock 及其余 engine 输入相同；两树使用同一份独立测试 harness（fixture + commit test，SHA256 `3ae050b3f73bdbffb0973b57ce8b12b696318dd2ba8838f5804433cbe7ee6cfe`），故不受正在进行的其他功能编辑干扰。

复现：在上述 baseline 提交与本版本各自的独立工作区，把本版本 tests/layout.rs 中 `benchmark_fixture` 与 `benchmark_commit_pipeline_cpu` 原样复制成同名独立测试（导入 `gpui_flow_engine::*`、Scene/Affine/DisplayObject/display 及 `serde_json::json`）。同一 Cargo dev/test 环境编译后固定两个测试可执行文件；各预热一次，然后交错运行各三次 `benchmark_commit_pipeline_cpu --ignored --nocapture --test-threads=1`。不要把编译或归档时间混进 commit 时间，也不要在测量中调用新 query API。

初版 rank 用 BTreeMap，同一实验曾观测 reparent 中位 +10.93%。随后仅将不依赖迭代顺序的私有 positions 改为 std HashMap，以减少重建时的查找/分配常数；全部语义对照重新通过。以下为最终 HashMap 版本重新测量，未混用初版数据；两轮都只有少量 dev 样本，不把变化全部归因于单个容器类型。

同 profile、固定可执行文件、每版本一次预热后交错三轮。下表为每个操作 100 次提交的微秒值；预热数据不计入中位数：

| 操作 | 索引前样本 1 / 2 / 3 | 索引后样本 1 / 2 / 3 | 前中位数 | 后中位数 | 观测变化 |
|---|---|---|---:|---:|---:|
| transform | 3,309,550 / 3,123,423 / 3,327,831 | 3,350,968 / 3,224,158 / 3,451,303 | 3,309,550 | 3,350,968 | +1.25% |
| rename | 3,238,375 / 3,226,459 / 3,361,211 | 3,310,646 / 3,328,751 / 3,287,064 | 3,238,375 | 3,310,646 | +2.23% |
| reparent | 3,097,424 / 2,886,181 / 3,080,958 | 3,158,689 / 3,235,830 / 3,185,597 | 3,080,958 | 3,185,597 | +3.40% |

所有预热与测量均显式通过 oracle；两版本每轮工作量完全相同：

| 操作（各 100 次） | visited | world 重算 | bounds 重算 | paint invalidated |
|---|---:|---:|---:|---:|
| transform | 54,500 | 100 | 200 | 0 |
| rename | 54,500 | 0 | 0 | 0 |
| reparent | 54,500 | 3,400 | 400 | 3,185 |

新查询能力有索引维护成本，尤其结构变化重建排序/区间。这个样本的中位提交时间均增加；因此**不声称原有提交性能无回归，也不声称整体加速**。三次 dev 测量没有置信区间，部分范围重叠，具体百分比不是可跨机器推广的常数；这里只给出实际观察和完全相同的确定性工作量。查询高收益不抵消这里的结构提交成本，是否适合真实负载须按查询/编辑比例判断。

第二个 ignored benchmark 在两份隔离快照各运行 4 次（1 预热 + 3 计时）全部通过；这 8 次是显式执行证据，与正常 suite 的两个 ignored 分开记录。

## 保留的性能边界

- 整体提交仍有 O(N) 全树遍历，不是 O(changed nodes)。现有 diff 的 sibling 查位置等部分可更差，不保证总提交严格线性
- 结构/z-index 更新会重建全树顺序；平级 z 排序最坏 O(N log N)。普通 transform/paint/metadata commit 不重建顺序
- 选择性查询从 id 或最小 name/tag/class bucket/子树范围取候选，按缓存 rank 排结果；bucket 和 engine handle 查找仍有 BTree 的对数成本，positions/rank 使用 HashMap 的平均常数查找；结果排序最坏 O(K log K)（不把哈希平均复杂度当成最坏保证）
- 全量查询、常见 class 查询、paint/hit 遍历仍可能访问整棵树；索引不是空间索引，不加速几何 broad phase
- 空间成本为 O(N + class memberships)，外加字符串/id 拷贝。未承诺任意规模内存上限
- benchmark 是 CPU 数据路径的可重复测量；时间会随机器/负载/profile 波动，无速度阈值验收，不声称 release 的相同比例
- GPUI 仍 full-frame；damage 记录不等于 GPU 局部绘制。APP-06 不因本项完成而改变状态
