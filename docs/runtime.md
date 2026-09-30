# AgentRunner 与后台执行

## 这层负责什么

`gpui-flow-runtime` 是一个无 GPUI、无网络客户端的任务桥接层。它按节点 ID 启动独立后台线程，把进度、成功输出、失败和取消事件送回调用线程。它不是 DAG 引擎，不自动读取 Graph，也没有内建供应商、API 密钥、队列服务或计费逻辑。

可运行教程位于 [`docs/examples/src/bin/runner.rs`](examples/src/bin/runner.rs)：

```bash
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin runner
```

示例把字符串变为大写，验证成功状态、输出和重试；没有真实服务调用。

## 实现接口

```rust
use gpui_flow_runtime::{AgentRunner, RunContext};
use serde_json::{Value, json};

struct EchoRunner;

impl AgentRunner for EchoRunner {
    fn run(&self, input: Value, context: RunContext) -> Result<Value, String> {
        if context.cancellation.is_cancelled() {
            return Err("cancelled".into());
        }
        context.progress(0.5, "Processing input");
        Ok(json!({"echo": input}))
    }
}
```

`AgentRunner: Send + Sync + 'static`，方法本身是同步接口，但 Runtime 在后台线程调用它。异步供应商客户端需要在适配器内桥接自己的异步 runtime；不要在 GPUI 主线程等待网络。

`RunContext` 提供：

- `cancellation.is_cancelled()`：读合作式取消标记
- `progress(fraction, message)`：发送进度，有限值夹到 0–1，非有限值按 0 处理，已取消后不再发送

供应商错误使用 `Err(String)` 返回；不要把包含请求凭证、完整敏感响应或私有文件内容的错误字符串直接展示或记录。

## 启动、轮询、取消与重试

```rust
use gpui_flow_runtime::Runtime;
use serde_json::json;
use std::sync::Arc;

let mut runtime = Runtime::default();
let run_id = runtime.start("node-1", json!({"prompt": "hello"}), Arc::new(EchoRunner))?;

// 在 UI timer / 宿主事件循环中调用，不阻塞：
for event in runtime.poll() {
    println!("{} {} {:?}", event.run_id, event.node_id, event.event);
}

// 发出合作式取消；不会强行中止后台线程：
runtime.cancel("node-1");
```

这段展示调用关系；运行中不要用紧密 busy-loop 轮询。原生 `FlowCanvas` 已经每约 40ms 轮询，无需再启动第二个轮询循环；非 UI 的完整等待示例包含超时，见可执行教程。

状态由 `poll()` 应用事件后更新，顺序通常为：

```text
Queued → Running → Succeeded
                 → Failed
                 → Cancelled
```

`start` 立即记录 Queued 并返回 `run_id`。后台工作已经结束但宿主尚未 `poll` 时，记录仍可能显示 Queued/Running。

`RunEvent` 包括 `Started`、`Progress { fraction, message }`、`Output(Value)`、`Failed(String)`、`Cancelled`。外层 `Event` 带 `run_id`、`node_id`、`attempt`。`RunRecord` 保留状态、进度、消息与成功输出。

- 同一节点处于 Queued/Running 时，`start` / `retry` 返回错误
- 终态后 `retry(node)` 复用之前的 runner 和输入，attempt 加 1
- 要使用更新输入或不同供应商，在终态后重新 `start(node, new_input, runner)`
- `poll` 忽略 run_id 不匹配的旧事件，也忽略已进入终态的后续事件
- runner panic 会转成 `Failed("runner panicked")`；panic hook 仍可能打印诊断
- `cancel` 只置位令牌；`true` 表示该节点存在可访问的任务令牌，不证明工作已经停止
- runner 返回时若令牌已取消，最终状态采用 Cancelled，即使 runner 返回了成功值
- `forget(node)` 取消并移除该节点的任务/记录；其已排队事件随后被忽略，不能再 retry
- Runtime 被释放时会标记已有令牌，但不会 join 或强行终止不合作的线程

## 与画布集成

`FlowCanvas.runtime` 为公共字段。宿主可以在 GPUI 实体更新回调中调用：

```rust
canvas.update(cx, |canvas, cx| {
    let result = canvas.runtime.start(
        "agent-1",
        serde_json::json!({"text": "input prepared by host"}),
        std::sync::Arc::new(EchoRunner),
    );
    cx.notify();
    result
})?;
```

画布定时器随后会发出 `FlowEvent::RunChanged(node_id)`。节点 body 会根据同 ID 的 RunRecord 展示进度与状态。画布删除节点时会 forget 对应任务，import_json/Open 时会重置 Runtime；宿主直接操作 editor 时要自行维护这些生命周期。默认 renderer 不把 `output` 写入图，也不在输出节点里显示结果正文。宿主决定是否将结果变成一个媒体资源引用、更新节点 data 或追加一个新的结果节点。

### 按 kind 注册 runner

```rust
canvas.register_runner("agent", EchoRunner);
canvas.run_label = "Run agent".into();
```

`FlowCanvas::new(graph, cx)` 不注册任何 runner。`register_runner(kind, runner)` 将实现存入公共 `runners` 映射；同 kind 再注册会覆盖。Run 控件只运行已注册 kind 的选中节点，没有选中节点时运行全部已注册 kind 节点，输入是该节点的 `data` 副本。

`FlowCanvas::demo` 为 generation/agent 注册 `MockRunner::default()`，并将标签设为 `Run · MOCK`。更改 `node.data.provider` 不会自动注册或选择真实供应商。宿主应按授权、成本与输入类型注册适配器，并在必要时禁用/替换自己的运行入口。

## 从节点任务走向工作流

若要按连线执行，可在应用层显式实现：

1. 校验图并按业务选择 DAG 或循环语义
2. 解析输入端口和依赖，准备每个任务的输入值
3. 只有依赖成功后才启动后续节点，定义失败/跳过/取消传播策略
4. 限制总并发、供应商并发、请求速率、超时、重试与最大费用
5. 将结果应用到图时核对文档版本、节点是否仍存在、run_id 是否仍有效
6. 在结束/关闭/重试时释放资源，必要时调用供应商自己的取消接口

当前 Runtime 没有线程池、并发上限、任务超时、退避、持久队列、跨进程恢复或自动记录淘汰策略。为大规模/长期任务接入成熟调度器比无限创建线程更合适。

## 凭证、安全与费用

不要把 API key、OAuth token、密码放进 `Node.data`、Graph、剪贴板、文档快照或示例文件。推荐让 runner 持有由宿主创建的客户端/凭证句柄，实际凭证从环境变量、系统钥匙串或用户授权的 secret store 读取，图中最多保存无秘密的 provider 配置标识。

接入真实服务前，由宿主获取必要授权并展示费用/上传范围；限制模型与工具权限、校验外部输出、脱敏日志。取消本地令牌不代表远端停止计费或资源已删除，必须按供应商 API 确认远端状态。仓库 CI 与教程不需要、也不应设置真实供应商密钥。
