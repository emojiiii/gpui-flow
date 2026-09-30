use gpui_flow_runtime::{AgentRunner, RunContext, RunStatus, Runtime};
use serde_json::{Value, json};
use std::{
    sync::Arc,
    thread,
    time::{Duration, Instant},
};

/// An offline example: no credentials, HTTP client, or external service.
struct UppercaseRunner;

impl AgentRunner for UppercaseRunner {
    fn run(&self, input: Value, context: RunContext) -> Result<Value, String> {
        let text = input["text"].as_str().ok_or("text must be a string")?;
        if context.cancellation.is_cancelled() {
            return Err("cancelled before work".into());
        }
        context.progress(0.25, "Input validated");
        let result = text.to_uppercase();
        if context.cancellation.is_cancelled() {
            return Err("cancelled during work".into());
        }
        context.progress(0.9, "Result prepared");
        Ok(json!({"text": result}))
    }
}

fn wait_until_terminal(runtime: &mut Runtime, node: &str) -> Result<(), String> {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        for event in runtime.poll() {
            println!(
                "run={} attempt={} {:?}",
                event.run_id, event.attempt, event.event
            );
        }
        let record = &runtime.records[node];
        if matches!(
            record.status,
            RunStatus::Succeeded | RunStatus::Failed | RunStatus::Cancelled
        ) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            runtime.cancel(node);
            return Err("example timed out".into());
        }
        thread::sleep(Duration::from_millis(1));
    }
}

fn main() -> Result<(), String> {
    let mut runtime = Runtime::default();
    runtime.start(
        "uppercase",
        json!({"text": "hello canvas"}),
        Arc::new(UppercaseRunner),
    )?;
    wait_until_terminal(&mut runtime, "uppercase")?;
    assert_eq!(runtime.records["uppercase"].status, RunStatus::Succeeded);
    assert_eq!(
        runtime.records["uppercase"].output,
        Some(json!({"text": "HELLO CANVAS"}))
    );

    runtime.retry("uppercase")?;
    wait_until_terminal(&mut runtime, "uppercase")?;
    assert_eq!(runtime.records["uppercase"].attempt, 2);
    assert_eq!(runtime.records["uppercase"].status, RunStatus::Succeeded);
    println!("Offline runner and terminal-state retry passed.");
    Ok(())
}
