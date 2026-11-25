//! Workflow orchestration is separate from individual node execution.
use super::*;
use gpui_flow_runtime::{RunContext, RunnerRegistry, Workflow, WorkflowRecord};
use serde_json::{Value, json};

impl FlowCanvas {
    /// Start a frozen graph snapshot. Runners use workflow port envelopes, unlike independent
    /// `runners`, which receive node.data directly. Existing active workflows must finish first.
    pub fn start_workflow(
        &mut self,
        concurrency: usize,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if self.workflow.as_ref().is_some_and(|w| !w.is_finished()) {
            return Err("workflow already running".into());
        }
        let mut workflow = Workflow::new(
            self.editor.graph().clone(),
            self.workflow_runners.clone(),
            concurrency,
        )?;
        workflow.poll();
        self.workflow_nodes = workflow.graph().nodes.keys().cloned().collect();
        self.workflow = Some(workflow);
        self.message = "Workflow started from frozen graph snapshot".into();
        cx.notify();
        Ok(())
    }
    pub fn register_workflow_runner(&mut self, kind: impl Into<String>, runner: impl AgentRunner) {
        self.workflow_runners.insert(kind.into(), Arc::new(runner));
    }

    pub(super) fn workflow_record(&self, id: &NodeId) -> Option<&WorkflowRecord> {
        self.workflow
            .as_ref()
            .filter(|_| self.workflow_nodes.contains(id))
            .and_then(|workflow| workflow.records().get(id))
    }
}
/// Explicit offline fixture, not an image generator or media decoder. Named media outputs
/// are descriptive JSON references so the demo proves typed graph scheduling without fees.
struct DemoWorkflowRunner {
    kind: String,
}
impl AgentRunner for DemoWorkflowRunner {
    fn run(&self, input: Value, context: RunContext) -> Result<Value, String> {
        let data = &input["node"];
        if matches!(self.kind.as_str(), "generation" | "agent") {
            for step in 0..10 {
                if context.cancellation.is_cancelled() {
                    return Err("cancelled".into());
                }
                std::thread::sleep(Duration::from_millis(80));
                context.progress((step + 1) as f32 / 10., "MOCK workflow provider");
            }
        }
        let output = match self.kind.as_str() {
            "text" => json!({"out":data["text"].as_str().unwrap_or("")}),
            "image" | "audio" | "video" => {
                json!({"out":{"source":data["source"],"media_type":self.kind}})
            }
            "generation" | "agent" => {
                json!({"out":{"mock":true,"prompt":data["prompt"],"inputs":input["inputs"],"message":"Offline workflow result; no media generated"}})
            }
            _ => json!({}),
        };
        Ok(output)
    }
}
pub(super) fn demo_workflow_runners() -> RunnerRegistry {
    BUILTIN_KINDS
        .iter()
        .map(|kind| {
            (
                kind.to_string(),
                Arc::new(DemoWorkflowRunner {
                    kind: kind.to_string(),
                }) as Arc<dyn AgentRunner>,
            )
        })
        .collect()
}
