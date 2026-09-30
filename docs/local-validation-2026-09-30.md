# 本地Linux验证记录（2026-09-30）

## 来源与环境

- 源码批次备份：[c87c121](https://github.com/emojiiii/gpui-flow/commit/c87c121959c617beb4ab546a7008ba8e88105238)
- 本次实际验证的修复后源码：[817a68f](https://github.com/emojiiii/gpui-flow/commit/817a68f6e4b014b6897768f1ef75ee5e8545c1dd)
- Linux x86_64云端，Rust/Cargo 1.98.1；使用已安装的native库和字体，没有连接用户电脑
- 全程一个复用target、`CARGO_BUILD_JOBS=1`、`CARGO_INCREMENTAL=0`、dev/test debug=0
- 所有构建使用`--locked`。首次offline发现petgraph 0.8.3未缓存，随后正常下载该固定版本，之后恢复offline；两个Cargo.lock均未修改
- 低于3 GiB可用空间会终止构建的保护未触发；未删除共享缓存或其他项目
- 两次发布均使用`[skip ci]`；精确修复SHA的Actions查询为0条，没有启用或手动触发CI

## 已通过

| 检查 | 当前结果 |
| --- | --- |
| 主workspace与独立文档workspace格式检查 | 通过 |
| 默认headless成员 `cargo test --locked --offline` | 562通过，0失败，8忽略；含doctest |
| 单独执行上述忽略项中的真实媒体回归 | 7通过：6个playback集成测试＋1个进程timeout/cancel测试；SDL dummy音频输出 |
| 完整workspace/all-targets check | 通过 |
| 完整workspace/all-targets strict clippy | 通过，`-D warnings` |
| 独立文档workspace strict clippy | 通过 |
| UI库 `gpui-flow --lib` | 38通过 |
| `gpui-flow-text --features gpui --lib` | 2通过 |
| 可执行文档 | 10个全部执行并通过断言 |
| `flow-studio --bins` | 14个Linux原生二进制全部链接成功 |
| 无窗口真实字体探针 | 通过 |

默认测试的8个忽略项中，7个媒体测试已补跑。剩下1个梯度手工性能测量仍未运行；没有将其算成通过。
新增回归包括：timing 21、topology 5、resources 16、named_states 14、Taffy兼容13、path bounds 12、advanced stroke 12；这些数字是562项中的子集，不额外相加。

10个文档程序是`animation`、`auto_size`、`core`、`focus`、`layout`、`plugins`、`reactive_layout`、`runner`、`state_styles`和`workflow`。
14个原生二进制是`flow-studio`、`animations`、`auto_size`、`clip_chains`、`composition`、`gradients`、`interactions`、`layout_appearance`、`measured_text`、`media`、`primitives`、`reactive_layout`、`state_styles`和`text_measure_probe`。

字体探针使用真实GPUI字体系统（258字体族），验证空文本、窄/宽字形、粗体、中英换行、emoji、末尾换行、缺失字体回退、测量缓存/失效及缩放一致性。响应布局父框高度从87.2变为70.4，单次Undo恢复，重复测量和失效后保持稳定。这是无窗口字体/布局验证，不是鼠标键盘或画面验收。

## 本轮失败与修复

1. 初次offline依赖解析失败：缺petgraph 0.8.3缓存；下载锁定版本后恢复，未改依赖或lockfile
2. Taffy适配器漏导入`FromLength`和`TaffyAuto`，出现14处E0599；补充trait导入后完整headless及workspace检查通过
3. timing的严格clippy报`manual_is_multiple_of`和`collapsible_if`；保持语义修正写法后严格lint及headless复跑通过
4. trait导入触发一次rustfmt换行差异；应用格式后两套fmt检查通过

没有遗留编译、测试或lint失败。文档中的旧源码阶段“未运行”标记已更新；旧增量章节的历史数字仍仅属于当时批次。

## 实际命令

```bash
export CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0
export CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
# 两套workspace复用同一CARGO_TARGET_DIR，已加载本机native依赖环境
cargo fmt --all -- --check
cargo fmt --manifest-path docs/examples/Cargo.toml --package gpui-flow-doc-examples -- --check
cargo test --locked --offline
cargo clippy --locked --offline --all-targets -- -D warnings
cargo check --locked --offline --workspace --all-targets
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
cargo clippy --locked --offline --manifest-path docs/examples/Cargo.toml --all-targets -- -D warnings
for bin in animation auto_size core focus layout plugins reactive_layout runner state_styles workflow; do
  cargo run --locked --offline --manifest-path docs/examples/Cargo.toml --bin "$bin"
done
cargo test --locked --offline -p gpui-flow --lib
cargo test --locked --offline -p gpui-flow-text --features gpui --lib
cargo build --locked --offline -p flow-studio --bins
"$CARGO_TARGET_DIR/debug/text_measure_probe"
```

媒体测试直接运行本轮`cargo test`生成的测试二进制，避免与正在运行的native编译争Cargo锁：

```bash
SDL_AUDIODRIVER=dummy "$CARGO_TARGET_DIR/debug/deps/playback-bf4589a1da4b1a78" --ignored --test-threads=1
SDL_AUDIODRIVER=dummy "$CARGO_TARGET_DIR/debug/deps/gpui_flow_media-63db49099cd141ec" --ignored --test-threads=1
```

二进制hash随环境/feature变化；在其他机器复现可用`SDL_AUDIODRIVER=dummy cargo test --locked -p gpui-flow-media -- --ignored --test-threads=1`，不要照抄旧hash。

## 磁盘记录

开始实际验证时可用20,052,180,992字节，完成编译后可用14,025,080,832字节；期间总可用空间变化约6.03 GB。该差值包含共享环境同期活动，不全部归因于此项目。唯一target占用5,978,148,864字节（约5.57 GiB），已保留供后续复用。

## 未运行与不能推导的结论

- 没有本批真实窗口鼠标、键盘、焦点、disabled按钮、图片加载/失败、状态视觉或paint回执交互验收
- 没有Windows/macOS编译或运行，没有新CI结果
- 没有真实扬声器输出、供应商调用、付费生成或部署；runner示例为离线mock
- 梯度手工性能测量、大规模性能和任意输入数值等价未验证
- UI单测、资源模型测试、链接及字体探针不能证明GPU呈现fence或完整资源端到端行为
- 未实现真正隔离合成、混合/蒙版、inside/outside与scale-fixed stroke、嵌套timeline、motion path或完整Leafer兼容；详见[限制](limitations.md)
