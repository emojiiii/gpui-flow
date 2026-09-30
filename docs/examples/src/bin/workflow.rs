use gpui_flow_core::{DataType, Graph, Node, Point, Port, PortRef};
use gpui_flow_runtime::{AgentRunner, RunContext, RunnerRegistry, Workflow};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
struct Source;
impl AgentRunner for Source {
    fn run(&self, _: Value, _: RunContext) -> Result<Value, String> {
        Ok(json!({"out":"hello canvas"}))
    }
}
struct Uppercase;
impl AgentRunner for Uppercase {
    fn run(&self, input: Value, context: RunContext) -> Result<Value, String> {
        if context.cancellation.is_cancelled() {
            return Err("cancelled".into());
        }
        let text = input["inputs"]["in"][0]
            .as_str()
            .ok_or("expected string input")?;
        Ok(json!({"out":text.to_uppercase()}))
    }
}
fn main() -> Result<(), String> {
    let mut graph = Graph::new();
    graph
        .add_node(
            Node::new("source", "Source", Point::ZERO)
                .with_kind("source")
                .with_port(Port::output("out", "Text", DataType::String)),
        )
        .map_err(|e| e.to_string())?;
    graph
        .add_node(
            Node::new("upper", "Uppercase", Point::ZERO)
                .with_kind("upper")
                .with_port(Port::input("in", "Text", DataType::String))
                .with_port(Port::output("out", "Result", DataType::String)),
        )
        .map_err(|e| e.to_string())?;
    graph
        .connect(
            "edge",
            PortRef::new("source", "out"),
            PortRef::new("upper", "in"),
        )
        .map_err(|e| e.to_string())?;
    let runners: RunnerRegistry = [
        ("source".into(), Arc::new(Source) as Arc<dyn AgentRunner>),
        ("upper".into(), Arc::new(Uppercase) as Arc<dyn AgentRunner>),
    ]
    .into();
    let mut workflow = Workflow::new(graph, runners, 2)?;
    let deadline = Instant::now() + Duration::from_secs(5);
    while !workflow.is_finished() {
        workflow.poll();
        if Instant::now() > deadline {
            return Err("workflow timed out".into());
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(workflow.succeeded());
    assert_eq!(
        workflow.records()[&"upper".into()].outputs,
        Some(json!({"out":"HELLO CANVAS"}))
    );
    println!("Workflow source -> uppercase succeeded: HELLO CANVAS (no service calls)");
    Ok(())
}
