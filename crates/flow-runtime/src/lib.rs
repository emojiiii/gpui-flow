//! Small provider-neutral background execution interface. No network client is bundled.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread,
    time::Duration,
};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum RunStatus {
    Queued,
    Running,
    Succeeded,
    Failed,
    Cancelled,
}
#[derive(Clone, Debug)]
pub enum RunEvent {
    Started,
    Progress { fraction: f32, message: String },
    Output(Value),
    Failed(String),
    Cancelled,
}
#[derive(Clone, Debug)]
pub struct Event {
    pub run_id: u64,
    pub node_id: String,
    pub attempt: u32,
    pub event: RunEvent,
}
#[derive(Clone, Default)]
pub struct CancellationToken(Arc<AtomicBool>);
impl CancellationToken {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}
#[derive(Clone)]
pub struct RunContext {
    pub cancellation: CancellationToken,
    emit: Arc<dyn Fn(RunEvent) + Send + Sync>,
}
impl RunContext {
    pub fn progress(&self, fraction: f32, message: impl Into<String>) {
        if !self.cancellation.is_cancelled() {
            (self.emit)(RunEvent::Progress {
                fraction: if fraction.is_finite() {
                    fraction.clamp(0., 1.)
                } else {
                    0.
                },
                message: message.into(),
            });
        }
    }
}
/// Implement this trait for your provider. Keep credentials outside serialized node data.
/// Cancellation is cooperative: providers must check the token and cancel remote work themselves.
pub trait AgentRunner: Send + Sync + 'static {
    fn run(&self, input: Value, context: RunContext) -> Result<Value, String>;
}
#[derive(Clone, Debug)]
pub struct RunRecord {
    pub run_id: u64,
    pub attempt: u32,
    pub status: RunStatus,
    pub progress: f32,
    pub message: String,
    pub output: Option<Value>,
}
struct Active {
    token: CancellationToken,
    input: Value,
    runner: Arc<dyn AgentRunner>,
}
/// Drives independent node jobs. Call `poll` on your UI timer; it never blocks.
/// Retrying never starts until a previous attempt is terminal; stale events are ignored.
pub struct Runtime {
    pub records: BTreeMap<String, RunRecord>,
    active: BTreeMap<String, Active>,
    sender: mpsc::Sender<Event>,
    receiver: mpsc::Receiver<Event>,
    next_id: u64,
}
impl Default for Runtime {
    fn default() -> Self {
        let (sender, receiver) = mpsc::channel();
        Self {
            records: BTreeMap::new(),
            active: BTreeMap::new(),
            sender,
            receiver,
            next_id: 1,
        }
    }
}
impl Runtime {
    pub fn start(
        &mut self,
        node: impl Into<String>,
        input: Value,
        runner: Arc<dyn AgentRunner>,
    ) -> Result<u64, String> {
        let node = node.into();
        if self
            .records
            .get(&node)
            .is_some_and(|r| matches!(r.status, RunStatus::Queued | RunStatus::Running))
        {
            return Err("node already running".into());
        }
        let attempt = self.records.get(&node).map_or(1, |r| r.attempt + 1);
        let run_id = self.next_id;
        self.next_id += 1;
        let token = CancellationToken::default();
        let worker_token = token.clone();
        let tx = self.sender.clone();
        let worker_node = node.clone();
        self.records.insert(
            node.clone(),
            RunRecord {
                run_id,
                attempt,
                status: RunStatus::Queued,
                progress: 0.,
                message: "Queued".into(),
                output: None,
            },
        );
        self.active.insert(
            node,
            Active {
                token,
                input: input.clone(),
                runner: runner.clone(),
            },
        );
        thread::spawn(move || {
            let emit: Arc<dyn Fn(RunEvent) + Send + Sync> = Arc::new(move |event| {
                let _ = tx.send(Event {
                    run_id,
                    node_id: worker_node.clone(),
                    attempt,
                    event,
                });
            });
            emit(RunEvent::Started);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                runner.run(
                    input,
                    RunContext {
                        cancellation: worker_token.clone(),
                        emit: emit.clone(),
                    },
                )
            }));
            if worker_token.is_cancelled() {
                emit(RunEvent::Cancelled);
            } else {
                match result {
                    Ok(Ok(output)) => emit(RunEvent::Output(output)),
                    Ok(Err(error)) => emit(RunEvent::Failed(error)),
                    Err(_) => emit(RunEvent::Failed("runner panicked".into())),
                }
            }
        });
        Ok(run_id)
    }
    pub fn cancel(&mut self, node: &str) -> bool {
        if let Some(active) = self.active.get(node) {
            active.token.cancel();
            true
        } else {
            false
        }
    }
    pub fn retry(&mut self, node: &str) -> Result<u64, String> {
        let a = self.active.get(node).ok_or("unknown job")?;
        let input = a.input.clone();
        let runner = a.runner.clone();
        self.start(node, input, runner)
    }
    pub fn poll(&mut self) -> Vec<Event> {
        let mut applied = vec![];
        while let Ok(event) = self.receiver.try_recv() {
            if let Some(r) = self.records.get_mut(&event.node_id) {
                if r.run_id != event.run_id
                    || matches!(
                        r.status,
                        RunStatus::Succeeded | RunStatus::Failed | RunStatus::Cancelled
                    )
                {
                    continue;
                }
                match &event.event {
                    RunEvent::Started => {
                        r.status = RunStatus::Running;
                        r.message = "Running".into();
                    }
                    RunEvent::Progress { fraction, message } => {
                        r.progress = *fraction;
                        r.message = message.clone();
                    }
                    RunEvent::Output(output) => {
                        r.status = RunStatus::Succeeded;
                        r.progress = 1.;
                        r.output = Some(output.clone());
                        r.message = "Completed".into();
                    }
                    RunEvent::Failed(error) => {
                        r.status = RunStatus::Failed;
                        r.message = error.clone();
                    }
                    RunEvent::Cancelled => {
                        r.status = RunStatus::Cancelled;
                        r.message = "Cancelled".into();
                    }
                }
                applied.push(event);
            }
        }
        applied
    }
}
impl Drop for Runtime {
    fn drop(&mut self) {
        for active in self.active.values() {
            active.token.cancel();
        }
    }
}
/// Offline fake runner for the demo. It does not generate real media or call any service.
pub struct MockRunner {
    pub steps: u32,
    pub delay: Duration,
    pub fail: bool,
}
impl Default for MockRunner {
    fn default() -> Self {
        Self {
            steps: 20,
            delay: Duration::from_millis(90),
            fail: false,
        }
    }
}
impl AgentRunner for MockRunner {
    fn run(&self, input: Value, context: RunContext) -> Result<Value, String> {
        for step in 0..self.steps.max(1) {
            if context.cancellation.is_cancelled() {
                return Err("cancelled".into());
            }
            thread::sleep(self.delay);
            context.progress(
                (step + 1) as f32 / self.steps.max(1) as f32,
                format!("MOCK · step {}", step + 1),
            );
        }
        if self.fail {
            Err("Mock failure: retry with another provider or input".into())
        } else {
            Ok(
                serde_json::json!({"mock":true,"input":input,"message":"Offline demo completed; no media generated"}),
            )
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn terminal(rt: &mut Runtime) {
        for _ in 0..1000 {
            rt.poll();
            if rt
                .records
                .values()
                .all(|r| !matches!(r.status, RunStatus::Queued | RunStatus::Running))
            {
                return;
            }
            thread::sleep(Duration::from_millis(1));
        }
        panic!("job timed out");
    }
    #[test]
    fn success_and_retry() {
        let mut rt = Runtime::default();
        let runner = Arc::new(MockRunner {
            steps: 1,
            delay: Duration::ZERO,
            fail: false,
        });
        rt.start("a", Value::Null, runner).unwrap();
        assert!(rt.retry("a").is_err());
        terminal(&mut rt);
        assert_eq!(rt.records["a"].status, RunStatus::Succeeded);
        rt.retry("a").unwrap();
        terminal(&mut rt);
        assert_eq!(rt.records["a"].attempt, 2);
    }
    #[test]
    fn cancellation() {
        let mut rt = Runtime::default();
        rt.start("a", Value::Null, Arc::new(MockRunner::default()))
            .unwrap();
        assert!(rt.cancel("a"));
        terminal(&mut rt);
        assert_eq!(rt.records["a"].status, RunStatus::Cancelled);
    }
    #[test]
    fn failure() {
        let mut rt = Runtime::default();
        rt.start(
            "a",
            Value::Null,
            Arc::new(MockRunner {
                steps: 1,
                delay: Duration::ZERO,
                fail: true,
            }),
        )
        .unwrap();
        terminal(&mut rt);
        assert_eq!(rt.records["a"].status, RunStatus::Failed);
    }
}
