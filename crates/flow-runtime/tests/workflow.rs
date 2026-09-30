//! Scheduler contract tests. All runners are offline and waits have bounded deadlines.
use gpui_flow_core::{DataType, Graph, Node, NodeId, Point, Port, PortRef};
use gpui_flow_runtime::{
    AgentRunner, RunContext, RunnerRegistry, Workflow, WorkflowStatus, value_matches,
};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
    time::{Duration, Instant},
};

const DEADLINE: Duration = Duration::from_secs(5);

struct FunctionRunner<F>(F);
impl<F> AgentRunner for FunctionRunner<F>
where
    F: Fn(Value, RunContext) -> Result<Value, String> + Send + Sync + 'static,
{
    fn run(&self, input: Value, context: RunContext) -> Result<Value, String> {
        (self.0)(input, context)
    }
}

fn runner<F>(function: F) -> Arc<dyn AgentRunner>
where
    F: Fn(Value, RunContext) -> Result<Value, String> + Send + Sync + 'static,
{
    Arc::new(FunctionRunner(function))
}

fn registry(
    entries: impl IntoIterator<Item = (&'static str, Arc<dyn AgentRunner>)>,
) -> RunnerRegistry {
    entries
        .into_iter()
        .map(|(kind, r)| (kind.into(), r))
        .collect()
}

fn node(id: &str, kind: &str) -> Node {
    Node::new(id, id, Point::ZERO)
        .with_kind(kind)
        .with_data(json!({"id": id}))
}

fn source(id: &str, kind: &str, data_type: DataType) -> Node {
    node(id, kind).with_port(Port::output("out", "Output", data_type))
}

fn transform(id: &str, kind: &str, data_type: DataType) -> Node {
    source(id, kind, data_type.clone())
        .with_port(Port::input("in", "Input", data_type).with_capacity(None))
}

fn graph(nodes: impl IntoIterator<Item = Node>) -> Graph {
    let mut graph = Graph::new();
    for node in nodes {
        graph.add_node(node).unwrap();
    }
    graph
}

fn connect(graph: &mut Graph, edge: &str, source: &str, target: &str) {
    graph
        .connect(
            edge,
            PortRef::new(source, "out"),
            PortRef::new(target, "in"),
        )
        .unwrap();
}

fn status(workflow: &Workflow, node: &str) -> WorkflowStatus {
    workflow.records()[&NodeId::from(node)].status.clone()
}

fn poll_until(workflow: &mut Workflow, condition: impl Fn(&Workflow) -> bool) {
    let deadline = Instant::now() + DEADLINE;
    loop {
        workflow.poll();
        if condition(workflow) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "scheduler timed out: {:?}",
            workflow.records()
        );
        thread::sleep(Duration::from_millis(1));
    }
}

fn terminal(workflow: &mut Workflow) {
    poll_until(workflow, Workflow::is_finished);
}

fn wait_flag(flag: &AtomicBool, context: &RunContext) -> Result<(), String> {
    let deadline = Instant::now() + DEADLINE;
    while !flag.load(Ordering::SeqCst) {
        if context.cancellation.is_cancelled() {
            return Err("cancelled while waiting".into());
        }
        if Instant::now() >= deadline {
            return Err("test runner gate timed out".into());
        }
        thread::sleep(Duration::from_millis(1));
    }
    Ok(())
}

#[test]
fn typed_values_and_node_payload_flow_through_explicit_port_envelopes() {
    let received = Arc::new(Mutex::new(Vec::new()));
    let captures = received.clone();
    let mut graph = graph([
        source("source", "source", DataType::Number)
            .with_data(json!({"value": 21, "private_config_ref": "demo-only"})),
        transform("double", "double", DataType::Number).with_port(Port::input(
            "optional",
            "Optional",
            DataType::String,
        )),
    ]);
    connect(&mut graph, "edge", "source", "double");
    let mut workflow = Workflow::new(
        graph,
        registry([
            (
                "source",
                runner(|input, _| {
                    assert_eq!(input["inputs"], json!({}));
                    Ok(json!({"out": input["node"]["value"]}))
                }),
            ),
            (
                "double",
                runner(move |input, _| {
                    captures.lock().unwrap().push(input.clone());
                    Ok(json!({"out": input["inputs"]["in"][0].as_i64().unwrap() * 2}))
                }),
            ),
        ]),
        2,
    )
    .unwrap();
    assert!(!workflow.is_finished());
    assert_eq!(status(&workflow, "double"), WorkflowStatus::Pending);
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert_eq!(
        workflow.records()[&"double".into()].outputs,
        Some(json!({"out": 42}))
    );
    assert_eq!(
        *received.lock().unwrap(),
        vec![json!({"node": {"id": "double"}, "inputs": {"in": [21], "optional": []}})]
    );
}

#[test]
fn fanin_order_is_edge_id_order_independent_of_completion_and_node_id_order() {
    let release = Arc::new(AtomicBool::new(false));
    let first_gate = release.clone();
    let received = Arc::new(Mutex::new(None));
    let captures = received.clone();
    let mut graph = graph([
        source("a-fast", "fast", DataType::String),
        source("z-held", "held", DataType::String),
        transform("sink", "sink", DataType::String),
    ]);
    // Deliberately insert edges in reverse order; the held source owns the first edge ID.
    connect(&mut graph, "z-edge", "a-fast", "sink");
    connect(&mut graph, "a-edge", "z-held", "sink");
    let mut workflow = Workflow::new(
        graph,
        registry([
            ("fast", runner(|_, _| Ok(json!({"out": "completed-first"})))),
            (
                "held",
                runner(move |_, context| {
                    wait_flag(&first_gate, &context)?;
                    Ok(json!({"out": "edge-first"}))
                }),
            ),
            (
                "sink",
                runner(move |input, _| {
                    *captures.lock().unwrap() = Some(input["inputs"]["in"].clone());
                    Ok(json!({"out": "joined"}))
                }),
            ),
        ]),
        2,
    )
    .unwrap();
    poll_until(&mut workflow, |wf| {
        status(wf, "a-fast") == WorkflowStatus::Succeeded
    });
    assert_eq!(status(&workflow, "z-held"), WorkflowStatus::Running);
    assert_eq!(status(&workflow, "sink"), WorkflowStatus::Pending);
    release.store(true, Ordering::SeqCst);
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert_eq!(
        *received.lock().unwrap(),
        Some(json!(["edge-first", "completed-first"]))
    );
}

#[test]
fn parallel_ready_nodes_never_exceed_configured_concurrency() {
    let active = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let started = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(AtomicBool::new(false));
    let (running, high_water, calls, gate) = (
        active.clone(),
        peak.clone(),
        started.clone(),
        release.clone(),
    );
    let graph = graph((0..7).map(|i| node(&format!("job-{i}"), "gate")));
    let mut workflow = Workflow::new(
        graph,
        registry([(
            "gate",
            runner(move |_, context| {
                let concurrent = running.fetch_add(1, Ordering::SeqCst) + 1;
                high_water.fetch_max(concurrent, Ordering::SeqCst);
                calls.fetch_add(1, Ordering::SeqCst);
                let outcome = wait_flag(&gate, &context);
                running.fetch_sub(1, Ordering::SeqCst);
                outcome?;
                Ok(json!({}))
            }),
        )]),
        2,
    )
    .unwrap();
    poll_until(&mut workflow, |_| started.load(Ordering::SeqCst) == 2);
    for _ in 0..20 {
        workflow.poll();
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }
    assert_eq!(active.load(Ordering::SeqCst), 2);
    assert_eq!(
        workflow
            .records()
            .values()
            .filter(|r| r.status == WorkflowStatus::Pending)
            .count(),
        5
    );
    release.store(true, Ordering::SeqCst);
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert_eq!(started.load(Ordering::SeqCst), 7);
    assert_eq!(active.load(Ordering::SeqCst), 0);
    assert_eq!(peak.load(Ordering::SeqCst), 2);
}

#[test]
fn cyclic_graph_is_rejected_even_when_graph_options_allow_cycles() {
    let mut graph = graph([
        transform("a", "ok", DataType::Number),
        transform("b", "ok", DataType::Number),
    ]);
    assert!(graph.options.allow_cycles);
    connect(&mut graph, "ab", "a", "b");
    connect(&mut graph, "ba", "b", "a");
    graph.validate().unwrap();
    let error = Workflow::new(
        graph,
        registry([("ok", runner(|_, _| Ok(json!({"out": 1}))))]),
        2,
    )
    .err()
    .unwrap();
    assert!(error.contains("acyclic"), "{error}");
}

#[test]
fn self_cycle_is_rejected_even_if_model_allows_it() {
    let mut graph = graph([transform("a", "ok", DataType::Number)]);
    graph.options.allow_self_connections = true;
    connect(&mut graph, "self", "a", "a");
    let error = Workflow::new(
        graph,
        registry([("ok", runner(|_, _| Ok(json!({"out": 1}))))]),
        1,
    )
    .err()
    .unwrap();
    assert!(error.contains("acyclic"));
}

#[test]
fn cancellation_waits_for_cooperative_worker_cleanup_and_blocks_retry_until_terminal() {
    let entered = Arc::new(AtomicBool::new(false));
    let observed = Arc::new(AtomicBool::new(false));
    let allow_exit = Arc::new(AtomicBool::new(false));
    let downstream_calls = Arc::new(AtomicUsize::new(0));
    let (worker_entered, worker_observed, cleanup_gate) =
        (entered.clone(), observed.clone(), allow_exit.clone());
    let child_calls = downstream_calls.clone();
    let mut graph = graph([
        source("running", "cooperative", DataType::Number),
        transform("waiting", "child", DataType::Number),
    ]);
    connect(&mut graph, "edge", "running", "waiting");
    let mut workflow = Workflow::new(
        graph,
        registry([
            (
                "cooperative",
                runner(move |_, context| {
                    worker_entered.store(true, Ordering::SeqCst);
                    let deadline = Instant::now() + DEADLINE;
                    while !context.cancellation.is_cancelled() {
                        if Instant::now() >= deadline {
                            return Err("cancellation not received".into());
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    worker_observed.store(true, Ordering::SeqCst);
                    // The token has been seen, but cleanup deliberately has not completed.
                    while !cleanup_gate.load(Ordering::SeqCst) {
                        if Instant::now() >= deadline {
                            return Err("cleanup gate timed out".into());
                        }
                        thread::sleep(Duration::from_millis(1));
                    }
                    Ok(json!({"out": 9}))
                }),
            ),
            (
                "child",
                runner(move |_, _| {
                    child_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({"out": 0}))
                }),
            ),
        ]),
        1,
    )
    .unwrap();
    poll_until(&mut workflow, |_| entered.load(Ordering::SeqCst));
    workflow.cancel();
    workflow.cancel();
    poll_until(&mut workflow, |_| observed.load(Ordering::SeqCst));
    assert_eq!(status(&workflow, "waiting"), WorkflowStatus::Cancelled);
    assert_eq!(status(&workflow, "running"), WorkflowStatus::Running);
    assert!(!workflow.is_finished());
    assert!(workflow.retry(&"running".into()).is_err());
    allow_exit.store(true, Ordering::SeqCst);
    terminal(&mut workflow);
    assert!(!workflow.succeeded());
    assert_eq!(status(&workflow, "running"), WorkflowStatus::Cancelled);
    assert!(workflow.records()[&"running".into()].outputs.is_none());
    assert_eq!(downstream_calls.load(Ordering::SeqCst), 0);
    assert_eq!(workflow.records()[&"waiting".into()].attempt, 0);
}

#[test]
fn cancellation_before_first_poll_does_not_start_any_runner() {
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let mut workflow = Workflow::new(
        graph([node("job", "count")]),
        registry([(
            "count",
            runner(move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
                Ok(json!({}))
            }),
        )]),
        1,
    )
    .unwrap();
    workflow.cancel();
    terminal(&mut workflow);
    assert_eq!(status(&workflow, "job"), WorkflowStatus::Cancelled);
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(workflow.records()[&"job".into()].attempt, 0);
    workflow.retry(&"job".into()).unwrap();
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(workflow.records()[&"job".into()].attempt, 1);
}

#[test]
fn failure_skips_all_descendants_but_independent_work_succeeds() {
    let descendant_calls = Arc::new(AtomicUsize::new(0));
    let counter = descendant_calls.clone();
    let mut graph = graph([
        source("bad", "fail", DataType::Number),
        transform("child", "descendant", DataType::Number),
        transform("grandchild", "descendant", DataType::Number),
        source("independent", "ok", DataType::String),
    ]);
    connect(&mut graph, "bad-child", "bad", "child");
    connect(&mut graph, "child-grandchild", "child", "grandchild");
    let mut workflow = Workflow::new(
        graph,
        registry([
            (
                "fail",
                runner(|_, _| Err("expected offline failure".into())),
            ),
            (
                "descendant",
                runner(move |_, _| {
                    counter.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({"out": 1}))
                }),
            ),
            ("ok", runner(|_, _| Ok(json!({"out": "retained"})))),
        ]),
        3,
    )
    .unwrap();
    terminal(&mut workflow);
    assert_eq!(status(&workflow, "bad"), WorkflowStatus::Failed);
    assert_eq!(status(&workflow, "child"), WorkflowStatus::Skipped);
    assert_eq!(status(&workflow, "grandchild"), WorkflowStatus::Skipped);
    assert_eq!(status(&workflow, "independent"), WorkflowStatus::Succeeded);
    assert_eq!(descendant_calls.load(Ordering::SeqCst), 0);
    assert_eq!(workflow.records()[&"child".into()].attempt, 0);
    assert!(!workflow.succeeded());
}

#[test]
fn retry_failed_closure_reuses_unrelated_successful_outputs_and_increments_attempts() {
    let calls = Arc::new(Mutex::new(BTreeMap::<String, usize>::new()));
    let counter = calls.clone();
    let mut graph = graph([
        source("flaky", "work", DataType::Number),
        source("stable", "work", DataType::Number),
        transform("join", "work", DataType::Number),
        transform("leaf", "work", DataType::Number),
    ]);
    connect(&mut graph, "flaky-join", "flaky", "join");
    connect(&mut graph, "stable-join", "stable", "join");
    connect(&mut graph, "join-leaf", "join", "leaf");
    let mut workflow = Workflow::new(graph, registry([("work", runner(move |input, _| {
        let id = input["node"]["id"].as_str().unwrap();
        let call = {
            let mut counts = counter.lock().unwrap();
            let count = counts.entry(id.to_owned()).or_default();
            *count += 1;
            *count
        };
        match id {
            "flaky" if call == 1 => Err("try again".into()),
            "flaky" => Ok(json!({"out": 2})),
            "stable" => Ok(json!({"out": 40})),
            _ => Ok(json!({"out": input["inputs"]["in"].as_array().unwrap().iter().map(|v|v.as_i64().unwrap()).sum::<i64>()})),
        }
    }))]), 2).unwrap();
    terminal(&mut workflow);
    assert_eq!(status(&workflow, "flaky"), WorkflowStatus::Failed);
    assert_eq!(status(&workflow, "join"), WorkflowStatus::Skipped);
    let stable = workflow.records()[&"stable".into()].clone();
    assert!(
        workflow.retry(&"join".into()).is_err(),
        "an unsuccessful external predecessor must block retry"
    );
    assert_eq!(status(&workflow, "join"), WorkflowStatus::Skipped);
    workflow.retry(&"flaky".into()).unwrap();
    assert_eq!(status(&workflow, "flaky"), WorkflowStatus::Pending);
    assert_eq!(status(&workflow, "join"), WorkflowStatus::Pending);
    assert_eq!(status(&workflow, "leaf"), WorkflowStatus::Pending);
    assert_eq!(workflow.records()[&"stable".into()].outputs, stable.outputs);
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert_eq!(
        workflow.records()[&"leaf".into()].outputs,
        Some(json!({"out": 42}))
    );
    assert_eq!(workflow.records()[&"flaky".into()].attempt, 2);
    assert_eq!(workflow.records()[&"join".into()].attempt, 1);
    assert_eq!(workflow.records()[&"stable".into()].attempt, 1);
    assert_eq!(workflow.records()[&"stable".into()].outputs, stable.outputs);
    assert_eq!(
        *calls.lock().unwrap(),
        BTreeMap::from([
            ("flaky".into(), 2),
            ("stable".into(), 1),
            ("join".into(), 1),
            ("leaf".into(), 1),
        ])
    );
    assert!(workflow.retry(&"stable".into()).is_err());
    assert!(workflow.retry(&"does-not-exist".into()).is_err());
}

fn assert_bad_output(output: Value, output_type: DataType, target_type: DataType, expected: &str) {
    let downstream_calls = Arc::new(AtomicUsize::new(0));
    let calls = downstream_calls.clone();
    let mut graph = graph([
        source("bad", "bad", output_type),
        transform("downstream", "downstream", target_type),
    ]);
    connect(&mut graph, "edge", "bad", "downstream");
    let mut workflow = Workflow::new(
        graph,
        registry([
            ("bad", runner(move |_, _| Ok(output.clone()))),
            (
                "downstream",
                runner(move |_, _| {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(json!({}))
                }),
            ),
        ]),
        2,
    )
    .unwrap();
    terminal(&mut workflow);
    let record = &workflow.records()[&"bad".into()];
    assert_eq!(record.status, WorkflowStatus::Failed);
    assert!(
        record.message.contains(expected),
        "expected {expected:?}, got {:?}",
        record.message
    );
    assert!(record.outputs.is_none());
    assert_eq!(status(&workflow, "downstream"), WorkflowStatus::Skipped);
    assert_eq!(downstream_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn missing_connected_output_fails_before_downstream_execution() {
    assert_bad_output(
        json!({}),
        DataType::String,
        DataType::String,
        "missing connected output out",
    );
}

#[test]
fn non_object_runner_output_is_rejected() {
    assert_bad_output(
        json!([1, 2]),
        DataType::Any,
        DataType::Any,
        "must be an object",
    );
}

#[test]
fn unknown_output_port_is_rejected() {
    assert_bad_output(
        json!({"out": 1, "typo": 2}),
        DataType::Number,
        DataType::Number,
        "unknown output port typo",
    );
}

#[test]
fn wrong_primitive_type_is_rejected_at_source_port() {
    assert_bad_output(
        json!({"out": "not a number"}),
        DataType::Number,
        DataType::Number,
        "does not match Number",
    );
}

#[test]
fn any_output_is_runtime_checked_against_typed_target() {
    assert_bad_output(
        json!({"out": false}),
        DataType::Any,
        DataType::String,
        "incompatible with target downstream:in",
    );
}

#[test]
fn missing_runner_and_zero_concurrency_fail_at_construction() {
    let graph = graph([node("missing", "unregistered")]);
    let error = Workflow::new(graph.clone(), RunnerRegistry::new(), 1)
        .err()
        .unwrap();
    assert!(
        error.contains("unregistered") && error.contains("missing"),
        "{error}"
    );
    let error = Workflow::new(graph, RunnerRegistry::new(), 0)
        .err()
        .unwrap();
    assert!(error.contains("concurrency"), "{error}");
}

#[test]
fn invalid_graph_is_rejected_before_runner_execution() {
    let mut graph = graph([node("invalid", "ok")]);
    graph.nodes.get_mut(&"invalid".into()).unwrap().position.x = f32::NAN;
    let error = Workflow::new(
        graph,
        registry([("ok", runner(|_, _| panic!("must not run")))]),
        1,
    )
    .err()
    .unwrap();
    assert!(error.contains("finite"), "{error}");
}

#[test]
fn primitive_value_contracts_are_exact_and_named_contracts_are_host_defined() {
    for (kind, valid, invalid) in [
        (DataType::Bool, json!(true), json!(1)),
        (DataType::Number, json!(1.5), json!("1.5")),
        (DataType::String, json!("text"), json!(null)),
        (DataType::Object, json!({"a": 1}), json!([])),
    ] {
        assert!(value_matches(&kind, &valid));
        assert!(!value_matches(&kind, &invalid));
    }
    for value in [json!(null), json!([]), json!(false), json!(42)] {
        assert!(value_matches(&DataType::Any, &value));
        assert!(value_matches(&DataType::Named("image".into()), &value));
    }
}

#[test]
fn unconnected_outputs_may_be_omitted_and_empty_graph_is_finished() {
    let mut empty = Workflow::new(Graph::new(), RunnerRegistry::new(), 1).unwrap();
    assert!(empty.poll().is_empty());
    assert!(empty.is_finished());
    assert!(empty.succeeded());
    let mut workflow = Workflow::new(
        graph([source("unused", "ok", DataType::String)]),
        registry([("ok", runner(|_, _| Ok(json!({}))))]),
        1,
    )
    .unwrap();
    terminal(&mut workflow);
    assert!(workflow.succeeded());
}

#[test]
fn workflow_owns_frozen_graph_snapshot_and_runner_panics_become_failure() {
    let mut original = graph([source("job", "panic", DataType::Number)]);
    let mut workflow = Workflow::new(
        original.clone(),
        registry([(
            "panic",
            runner(|input, _| {
                assert_eq!(input["node"]["id"], "job");
                panic!("offline provider panic");
            }),
        )]),
        1,
    )
    .unwrap();
    original.nodes.get_mut(&"job".into()).unwrap().data = json!({"id": "edited"});
    assert_eq!(
        workflow.graph().nodes[&"job".into()].data,
        json!({"id": "job"})
    );
    terminal(&mut workflow);
    assert_eq!(status(&workflow, "job"), WorkflowStatus::Failed);
    assert_eq!(workflow.records()[&"job".into()].message, "runner panicked");
}

#[test]
fn cancelling_an_already_succeeded_workflow_preserves_success() {
    let mut workflow = Workflow::new(
        graph([node("finished", "ok")]),
        registry([("ok", runner(|_, _| Ok(json!({}))))]),
        1,
    )
    .unwrap();
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    workflow.cancel();
    workflow.poll();
    assert_eq!(status(&workflow, "finished"), WorkflowStatus::Succeeded);
    assert!(
        workflow.succeeded(),
        "late cancellation must not invalidate terminal success"
    );
}

#[test]
fn repeated_polls_do_not_restart_terminal_jobs_or_change_outputs() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mut workflow = Workflow::new(
        graph([source("once", "count", DataType::Number)]),
        registry([(
            "count",
            runner(move |_, _| {
                let call = count.fetch_add(1, Ordering::SeqCst) + 1;
                Ok(json!({"out": call}))
            }),
        )]),
        1,
    )
    .unwrap();
    terminal(&mut workflow);
    for _ in 0..50 {
        assert!(workflow.poll().is_empty());
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(workflow.records()[&"once".into()].attempt, 1);
    assert_eq!(
        workflow.records()[&"once".into()].outputs,
        Some(json!({"out": 1}))
    );
}

#[test]
fn output_contract_failure_can_be_retried_with_clean_records() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let mut graph = graph([
        source("source", "flaky-type", DataType::Number),
        transform("sink", "echo", DataType::Number),
    ]);
    connect(&mut graph, "edge", "source", "sink");
    let mut workflow = Workflow::new(
        graph,
        registry([
            (
                "flaky-type",
                runner(move |_, _| {
                    if count.fetch_add(1, Ordering::SeqCst) == 0 {
                        Ok(json!({"out": "invalid"}))
                    } else {
                        Ok(json!({"out": 7}))
                    }
                }),
            ),
            (
                "echo",
                runner(|input, _| Ok(json!({"out": input["inputs"]["in"][0]}))),
            ),
        ]),
        1,
    )
    .unwrap();
    terminal(&mut workflow);
    assert_eq!(status(&workflow, "source"), WorkflowStatus::Failed);
    assert_eq!(status(&workflow, "sink"), WorkflowStatus::Skipped);
    workflow.retry(&"source".into()).unwrap();
    assert!(workflow.records()[&"source".into()].outputs.is_none());
    assert_eq!(workflow.records()[&"source".into()].progress, 0.0);
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert_eq!(workflow.records()[&"source".into()].attempt, 2);
    assert_eq!(
        workflow.records()[&"sink".into()].outputs,
        Some(json!({"out": 7}))
    );
}

#[test]
fn serial_ready_node_order_is_deterministic() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let captures = order.clone();
    let mut workflow = Workflow::new(
        graph([
            node("z", "record"),
            node("a", "record"),
            node("m", "record"),
        ]),
        registry([(
            "record",
            runner(move |input, _| {
                captures
                    .lock()
                    .unwrap()
                    .push(input["node"]["id"].as_str().unwrap().to_owned());
                Ok(json!({}))
            }),
        )]),
        1,
    )
    .unwrap();
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert_eq!(*order.lock().unwrap(), vec!["a", "m", "z"]);
}

#[test]
fn late_progress_from_previous_attempt_cannot_mutate_retry_record() {
    let calls = Arc::new(AtomicUsize::new(0));
    let old_context = Arc::new(Mutex::new(None::<RunContext>));
    let release = Arc::new(AtomicBool::new(false));
    let second_entered = Arc::new(AtomicBool::new(false));
    let (counter, saved_context, gate, entered) = (
        calls.clone(),
        old_context.clone(),
        release.clone(),
        second_entered.clone(),
    );
    let mut workflow = Workflow::new(
        graph([node("job", "retry")]),
        registry([(
            "retry",
            runner(move |_, context| {
                if counter.fetch_add(1, Ordering::SeqCst) == 0 {
                    *saved_context.lock().unwrap() = Some(context);
                    return Err("first attempt failed".into());
                }
                entered.store(true, Ordering::SeqCst);
                wait_flag(&gate, &context)?;
                Ok(json!({}))
            }),
        )]),
        1,
    )
    .unwrap();
    terminal(&mut workflow);
    workflow.retry(&"job".into()).unwrap();
    poll_until(&mut workflow, |_| second_entered.load(Ordering::SeqCst));
    old_context
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .progress(0.99, "stale attempt");
    workflow.poll();
    let record = &workflow.records()[&"job".into()];
    assert_eq!(record.attempt, 2);
    assert_eq!(record.status, WorkflowStatus::Running);
    assert_eq!(record.progress, 0.0);
    assert_ne!(record.message, "stale attempt");
    release.store(true, Ordering::SeqCst);
    terminal(&mut workflow);
    assert!(workflow.succeeded());
}

#[test]
fn converging_failed_branches_retry_atomically_and_preserve_other_results() {
    let fail = Arc::new(AtomicBool::new(true));
    let flag = fail.clone();
    let mut g = graph([
        source("a", "flaky", DataType::Number),
        source("b", "flaky", DataType::Number),
        transform("join", "ok", DataType::Number),
        source("other", "ok", DataType::Number),
    ]);
    connect(&mut g, "a-join", "a", "join");
    connect(&mut g, "b-join", "b", "join");
    let mut workflow = Workflow::new(
        g,
        registry([
            (
                "flaky",
                runner(move |_, _| {
                    if flag.load(Ordering::SeqCst) {
                        Err("fail".into())
                    } else {
                        Ok(json!({"out":1}))
                    }
                }),
            ),
            ("ok", runner(|_, _| Ok(json!({"out":2})))),
        ]),
        2,
    )
    .unwrap();
    terminal(&mut workflow);
    assert!(workflow.retry(&"a".into()).is_err());
    assert!(workflow.retry(&"b".into()).is_err());
    assert_eq!(status(&workflow, "a"), WorkflowStatus::Failed);
    fail.store(false, Ordering::SeqCst);
    workflow.retry_many(&["a".into(), "b".into()]).unwrap();
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert_eq!(workflow.records()[&"other".into()].attempt, 1);
    assert_eq!(workflow.records()[&"a".into()].attempt, 2);
}

#[test]
fn cancelled_converging_branches_retry_all() {
    let mut g = graph([
        source("a", "ok", DataType::Number),
        source("b", "ok", DataType::Number),
        transform("join", "ok", DataType::Number),
    ]);
    connect(&mut g, "a-join", "a", "join");
    connect(&mut g, "b-join", "b", "join");
    let mut workflow = Workflow::new(
        g,
        registry([("ok", runner(|_, _| Ok(json!({"out":2}))))]),
        2,
    )
    .unwrap();
    workflow.cancel();
    workflow.retry_failed().unwrap();
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert!(workflow.retry_many(&[]).is_err());
    assert!(workflow.retry_failed().is_err());
}

#[test]
fn parallel_port_edges_keep_each_value_and_edge_id_fanin_order() {
    let mut graph = graph([
        source("source", "source", DataType::Number).with_port(Port::output(
            "other",
            "Other",
            DataType::Number,
        )),
        transform("sink", "sink", DataType::Number).with_port(Port::input(
            "separate",
            "Separate",
            DataType::Number,
        )),
    ]);
    connect(&mut graph, "z-edge", "source", "sink");
    graph
        .connect(
            "a-edge",
            PortRef::new("source", "other"),
            PortRef::new("sink", "in"),
        )
        .unwrap();
    graph
        .connect(
            "m-edge",
            PortRef::new("source", "out"),
            PortRef::new("sink", "separate"),
        )
        .unwrap();
    let mut workflow = Workflow::new(
        graph,
        registry([
            ("source", runner(|_, _| Ok(json!({"out": 10, "other": 20})))),
            (
                "sink",
                runner(|input, _| {
                    assert_eq!(input["inputs"], json!({"in": [20, 10], "separate": [10]}));
                    Ok(json!({"out": 40}))
                }),
            ),
        ]),
        1,
    )
    .unwrap();
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert_eq!(workflow.graph().edges.len(), 3);
    assert_eq!(workflow.records()[&"sink".into()].attempt, 1);
}

#[test]
fn newly_ready_child_competes_with_existing_roots_in_node_id_order() {
    let order = Arc::new(Mutex::new(Vec::new()));
    let capture = order.clone();
    let mut graph = graph([
        source("z-root", "record", DataType::Number),
        transform("a-child", "record", DataType::Number),
        source("b-root", "record", DataType::Number),
    ]);
    connect(&mut graph, "dependency", "b-root", "a-child");
    let mut workflow = Workflow::new(
        graph,
        registry([(
            "record",
            runner(move |input, _| {
                capture
                    .lock()
                    .unwrap()
                    .push(input["node"]["id"].as_str().unwrap().to_owned());
                Ok(json!({"out": 1}))
            }),
        )]),
        1,
    )
    .unwrap();
    terminal(&mut workflow);
    assert!(workflow.succeeded());
    assert_eq!(*order.lock().unwrap(), vec!["b-root", "a-child", "z-root"]);
}

#[test]
fn failure_reaches_reverse_id_chain_before_same_poll_schedules_more_work() {
    let ids: Vec<_> = (0..64)
        .rev()
        .map(|index| format!("node-{index:02}"))
        .collect();
    let mut graph = graph(ids.iter().map(|id| transform(id, "fail", DataType::Number)));
    for (index, pair) in ids.windows(2).enumerate() {
        connect(&mut graph, &format!("edge-{index:02}"), &pair[0], &pair[1]);
    }
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let mut workflow = Workflow::new(
        graph,
        registry([(
            "fail",
            runner(move |_, _| {
                counter.fetch_add(1, Ordering::SeqCst);
                Err("expected failure".into())
            }),
        )]),
        4,
    )
    .unwrap();
    poll_until(&mut workflow, |workflow| {
        status(workflow, &ids[0]) == WorkflowStatus::Failed
    });
    assert!(workflow.is_finished());
    for id in &ids[1..] {
        assert_eq!(status(&workflow, id), WorkflowStatus::Skipped);
        assert_eq!(workflow.records()[&NodeId::from(id.clone())].attempt, 0);
    }
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn retry_closure_handles_duplicate_and_overlapping_roots_without_unrelated_restarts() {
    let mut graph = graph([
        source("root", "ok", DataType::Number),
        transform("child", "ok", DataType::Number),
        transform("leaf", "ok", DataType::Number),
        source("unrelated", "ok", DataType::Number),
    ]);
    connect(&mut graph, "root-child", "root", "child");
    connect(&mut graph, "child-leaf", "child", "leaf");
    let mut workflow = Workflow::new(
        graph,
        registry([("ok", runner(|_, _| Ok(json!({"out": 1}))))]),
        2,
    )
    .unwrap();
    workflow.cancel();
    workflow
        .retry_many(&["child".into(), "root".into(), "root".into()])
        .unwrap();
    for id in ["root", "child", "leaf"] {
        assert_eq!(status(&workflow, id), WorkflowStatus::Pending);
    }
    assert_eq!(status(&workflow, "unrelated"), WorkflowStatus::Cancelled);
    terminal(&mut workflow);
    for id in ["root", "child", "leaf"] {
        assert_eq!(status(&workflow, id), WorkflowStatus::Succeeded);
        assert_eq!(workflow.records()[&NodeId::from(id)].attempt, 1);
    }
    assert_eq!(workflow.records()[&"unrelated".into()].attempt, 0);
    assert!(!workflow.succeeded());
}

#[test]
fn construction_error_precedence_remains_validation_concurrency_runner_then_cycle() {
    let mut graph = graph([
        transform("a", "missing", DataType::Number),
        transform("b", "missing", DataType::Number),
    ]);
    connect(&mut graph, "ab", "a", "b");
    connect(&mut graph, "ba", "b", "a");
    assert_eq!(
        Workflow::new(graph.clone(), RunnerRegistry::new(), 0)
            .err()
            .unwrap(),
        "concurrency must be positive"
    );
    assert_eq!(
        Workflow::new(graph.clone(), RunnerRegistry::new(), 1)
            .err()
            .unwrap(),
        "no workflow runner for kind missing (node a)"
    );
    assert_eq!(
        Workflow::new(
            graph.clone(),
            registry([("missing", runner(|_, _| Ok(json!({}))))]),
            1
        )
        .err()
        .unwrap(),
        "workflow requires an acyclic graph"
    );
    graph.nodes.get_mut(&"a".into()).unwrap().position.x = f32::NAN;
    assert!(
        Workflow::new(graph, RunnerRegistry::new(), 0)
            .err()
            .unwrap()
            .contains("finite")
    );
}
