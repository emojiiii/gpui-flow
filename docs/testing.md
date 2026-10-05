# 本地验证

开发时只运行受影响模块的聚焦测试；代码冻结后运行一次必要的全量、严格检查与格式检查。功能代码、可复现回归和简明契约一起提交。原始失败日志与窗口截图保留在工作产物中，不把逐次报告、备份收据或机器路径清单提交源码仓库。

## 常用命令

在仓库根目录使用锁定工具链与依赖：

```bash
cargo fmt --all -- --check
cargo test --locked -p gpui-flow-creative --lib
cargo test --locked -p gpui-flow-scene
cargo test --locked -p gpui-flow-interaction --test text_tools
cargo test --locked -p gpui-flow-composite --lib
cargo test --locked -p gpui-flow --lib
cargo clippy --locked --workspace --all-targets -- -D warnings
```

按变更选择实际受影响的crate或test，不必每次执行全部命令。可选功能沿用同一feature graph，避免重复编译相同依赖。`--offline`只在所需依赖已经缓存时使用。完整GPUI构建需要对应平台原生依赖。

文档示例属于独立workspace：

```bash
cargo test --locked --manifest-path docs/examples/Cargo.toml
cargo clippy --locked --manifest-path docs/examples/Cargo.toml --all-targets -- -D warnings
```

## 怎样判断通过

- `cargo check`证明类型检查，不代替链接或实际运行
- `cargo build --locked -p gpui-flow-creative --features desktop --bin flow-creative`证明当前工作台链接成功
- 模型/CPU回归保留独立预期值、错误原子性、取消/历史、像素和预算边界；不要用实现自身生成唯一expected，也不要为通过放宽门槛
- 原生窗口只补充自动测试无法证明的真实输入、可见效果、焦点/布局与关闭释放；无需在窗口夹具中重复整套CPU数学测试
- 同一源码哈希不重复整套测试；代码变化只补受影响检查，最终模块记录一份简明结果及已知限制
- 真实失败不能删成通过。截图保留实际MIME；桌面JPEG不作为GPU无损像素证明

## CI与平台

工作流定义见[ci.yml](../.github/workflows/ci.yml)。存在定义不表示某次提交已运行或通过。本轮开发使用本地检查与`[skip ci]`提交，不自动启动Actions、部署或跨平台测试。Linux窗口结果不外推到其他平台、IME、辅助技术或真实付费供应商。

固定任务范围见[45项任务表](leafer-task-ledger.md)，各功能的API与支持集合留在对应契约中；旧 Studio 的示例命令仅为历史记录，当前入口见[入门](getting-started.md)。旧运行报告仍可从Git历史恢复，不再作为当前分支的文档主体。

## 旧演示退出后的回归

`examples/studio` 已退出 workspace。核心 crate 的回归与 `docs/examples` 保留；
旧原生探针的历史通过结果不代表当前工作台已验收。独立保存的三类容器共享预算
回归位于 `crates/flow-composite/tests/multi_domain_shadow_budget.rs`。
详细覆盖对应关系见[迁移说明](studio-retirement.md)。
