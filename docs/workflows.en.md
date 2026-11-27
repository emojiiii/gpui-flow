# Workflows and runners

| API | Use it for | Runner input |
| --- | --- | --- |
| `Runtime` | An independent background task | The payload passed to `start` |
| `Workflow` | A directed acyclic graph with a concurrency limit | The node's data and inputs from upstream ports |

Both APIs use `AgentRunner`. The host supplies the runner implementation and
any service credentials. `RunContext` allows a runner to report progress and
check for cooperative cancellation.

## Run local examples

Run both examples from the repository root.

```sh
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin runner
cargo run --locked --manifest-path docs/examples/Cargo.toml --bin workflow
```

The workflow passes `hello canvas` from a source to an uppercase runner and
prints `HELLO CANVAS`. Both examples run without a window or a provider account.

## Standalone task

Start an independent task with `Runtime::start(node_id, payload, runner)`.
Call `poll()` from the host event loop to apply progress and results. After a
task reaches a terminal state, `retry(node_id)` uses its previous input and
runner.

```rust
use gpui_flow_runtime::{AgentRunner, RunContext};
use serde_json::{Value, json};

struct Echo;

impl AgentRunner for Echo {
    fn run(&self, input: Value, context: RunContext) -> Result<Value, String> {
        if context.cancellation.is_cancelled() {
            return Err("cancelled".into());
        }
        Ok(json!({"echo": input}))
    }
}
```

## Workflow data contract

`Workflow::new(graph, runners, concurrency)` validates the graph, checks that
all node kinds have runners, and requires positive concurrency. The workflow
owns the graph snapshot it receives. Call `poll()` to schedule nodes and
apply results.

Each runner receives the node's payload and an array for every input port.
Unconnected inputs have empty arrays. When several edges feed one port, values
arrive in edge ID order.

```json
{
  "node": { "style": "uppercase" },
  "inputs": { "in": ["hello canvas"] }
}
```

Return an object keyed by output port ID.

```json
{ "out": "HELLO CANVAS" }
```

Declaring an unknown output, omitting an output with a downstream connection,
or producing a value with the wrong JSON type fails the node. Named types
require a matching name on the connected ports. The runner is responsible
for validating their payloads.

## Failures, cancellation, and retry

- A node starts after its predecessors succeed. Failure skips downstream nodes
  and leaves independent branches running.
- Use `records()` for each node's status, attempts, progress, and outputs.
  `is_finished()` checks for terminal states. `succeeded()` checks that the run
  completed successfully.
- `cancel()` cancels pending nodes and signals running workers. Active runners
  need to check their cancellation token.
- Once active workers have finished, `retry(node_id)` reruns the affected
  branch and retains successful outputs outside that branch. Use
  `retry_many` for converging branches that need to be rerun together or
  `retry_failed` for all unsuccessful nodes.

In a `FlowCanvas` host, `register_runner` registers independent tasks and
`register_workflow_runner` registers runners with the workflow input envelope.
See [custom nodes](custom-nodes.en.md) and
[graph architecture](architecture.en.md#graph-model-and-connections).
