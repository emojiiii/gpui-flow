//! Frozen-DAG execution with bounded concurrency and explicit port-value envelopes.
//!
//! Runners receive `{"node": <node.data>, "inputs": {<port>: [values...]}}` and return
//! `{<output_port>: value}`. Fan-in follows edge-ID order. Credentials stay in runners.
use crate::{AgentRunner, Event, RunStatus, Runtime};
use gpui_flow_core::{DataType, Graph, NodeId, PortDirection};
use petgraph::{
    Incoming,
    algo::toposort,
    graph::{DiGraph, NodeIndex},
    visit::Dfs,
};
use serde_json::{Map, Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

pub type RunnerRegistry = BTreeMap<String, Arc<dyn AgentRunner>>;

/// Private index of the frozen, validated document. Parallel port edges are kept;
/// these indices never escape the workflow or replace document IDs/serialization.
struct WorkflowTopology {
    graph: DiGraph<NodeId, (), usize>,
    nodes: BTreeMap<NodeId, NodeIndex<usize>>,
    order: Vec<NodeIndex<usize>>,
}
impl WorkflowTopology {
    fn new(document: &Graph) -> Result<Self, String> {
        let mut graph = DiGraph::with_capacity(document.nodes.len(), document.edges.len());
        let nodes: BTreeMap<_, _> = document
            .nodes
            .keys()
            .map(|id| (id.clone(), graph.add_node(id.clone())))
            .collect();
        for edge in document.edges.values() {
            graph.add_edge(nodes[&edge.source.node], nodes[&edge.target.node], ());
        }
        let order =
            toposort(&graph, None).map_err(|_| "workflow requires an acyclic graph".to_string())?;
        Ok(Self {
            graph,
            nodes,
            order,
        })
    }

    fn predecessors(&self, node: &NodeId) -> impl Iterator<Item = &NodeId> {
        self.graph
            .neighbors_directed(self.nodes[node], Incoming)
            .map(move |index| &self.graph[index])
    }

    fn descendants_including(&self, roots: &[NodeId]) -> BTreeSet<NodeId> {
        let mut affected = BTreeSet::new();
        let mut dfs = Dfs::empty(&self.graph);
        for root in roots {
            // Retain the visitor map so converging or repeated roots are visited once.
            dfs.move_to(self.nodes[root]);
            while let Some(index) = dfs.next(&self.graph) {
                affected.insert(self.graph[index].clone());
            }
        }
        affected
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WorkflowStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Skipped,
    Cancelled,
}
#[derive(Clone, Debug)]
pub struct WorkflowRecord {
    pub status: WorkflowStatus,
    pub attempt: u32,
    pub progress: f32,
    pub message: String,
    pub outputs: Option<Value>,
}
impl Default for WorkflowRecord {
    fn default() -> Self {
        Self {
            status: WorkflowStatus::Pending,
            attempt: 0,
            progress: 0.,
            message: "Waiting for dependencies".into(),
            outputs: None,
        }
    }
}
/// Owns an immutable validated graph snapshot; edits to a canvas never change an active run.
/// Call `poll()` regularly. Cancellation is cooperative, so a provider ignoring its token
/// can delay completion. Retry is allowed only after all workers have reached terminal state.
pub struct Workflow {
    graph: Graph,
    topology: WorkflowTopology,
    runners: RunnerRegistry,
    runtime: Runtime,
    records: BTreeMap<NodeId, WorkflowRecord>,
    concurrency: usize,
    cancelled: bool,
}
impl Workflow {
    pub fn new(graph: Graph, runners: RunnerRegistry, concurrency: usize) -> Result<Self, String> {
        graph.validate().map_err(|e| e.to_string())?;
        if concurrency == 0 {
            return Err("concurrency must be positive".into());
        }
        for node in graph.nodes.values() {
            if !runners.contains_key(&node.kind) {
                return Err(format!(
                    "no workflow runner for kind {} (node {})",
                    node.kind, node.id
                ));
            }
        }
        let topology = WorkflowTopology::new(&graph)?;
        let records = graph
            .nodes
            .keys()
            .cloned()
            .map(|id| (id, WorkflowRecord::default()))
            .collect();
        Ok(Self {
            graph,
            topology,
            runners,
            runtime: Runtime::default(),
            records,
            concurrency,
            cancelled: false,
        })
    }
    pub fn graph(&self) -> &Graph {
        &self.graph
    }
    pub fn records(&self) -> &BTreeMap<NodeId, WorkflowRecord> {
        &self.records
    }
    pub fn is_finished(&self) -> bool {
        self.records
            .values()
            .all(|r| !matches!(r.status, WorkflowStatus::Pending | WorkflowStatus::Running))
    }
    pub fn succeeded(&self) -> bool {
        self.is_finished()
            && !self.cancelled
            && self
                .records
                .values()
                .all(|r| r.status == WorkflowStatus::Succeeded)
    }
    pub fn cancel(&mut self) {
        if self.is_finished() {
            return;
        }
        self.cancelled = true;
        for (id, record) in &mut self.records {
            match record.status {
                WorkflowStatus::Pending => {
                    record.status = WorkflowStatus::Cancelled;
                    record.message = "Cancelled before start".into();
                }
                WorkflowStatus::Running => {
                    self.runtime.cancel(id.as_str());
                    record.message = "Cancellation requested".into();
                }
                _ => {}
            }
        }
    }
    /// Re-execute a failed/cancelled/skipped node and its descendants, keeping unrelated
    /// successful outputs. Every predecessor outside that closure must already have succeeded.
    pub fn retry(&mut self, node: &NodeId) -> Result<(), String> {
        self.retry_many(std::slice::from_ref(node))
    }
    /// Retry several failed branches atomically, including converging joins. Validates
    /// the entire closure before changing any state. Select all unsuccessful prerequisites.
    pub fn retry_many(&mut self, nodes: &[NodeId]) -> Result<(), String> {
        if !self.is_finished() {
            return Err("wait for all active workers before retry".into());
        }
        if nodes.is_empty() {
            return Err("select at least one unsuccessful node".into());
        }
        for node in nodes {
            let record = self.records.get(node).ok_or("unknown workflow node")?;
            if !matches!(
                record.status,
                WorkflowStatus::Failed | WorkflowStatus::Cancelled | WorkflowStatus::Skipped
            ) {
                return Err("retry requires failed, cancelled or skipped nodes".into());
            }
        }
        let affected = self.topology.descendants_including(nodes);
        if affected.iter().any(|id| {
            self.topology.predecessors(id).any(|predecessor| {
                !affected.contains(predecessor)
                    && self.records[predecessor].status != WorkflowStatus::Succeeded
            })
        }) {
            return Err(
                "include all unsuccessful predecessors in retry_many or use retry_failed".into(),
            );
        }
        for id in affected {
            let attempt = self.records[&id].attempt;
            self.records.insert(
                id,
                WorkflowRecord {
                    attempt,
                    ..Default::default()
                },
            );
        }
        self.cancelled = false;
        Ok(())
    }
    /// Retry every unsuccessful branch, preserving successful independent results.
    pub fn retry_failed(&mut self) -> Result<(), String> {
        let nodes = self
            .records
            .iter()
            .filter(|(_, r)| {
                matches!(
                    r.status,
                    WorkflowStatus::Failed | WorkflowStatus::Cancelled | WorkflowStatus::Skipped
                )
            })
            .map(|(id, _)| id.clone())
            .collect::<Vec<_>>();
        self.retry_many(&nodes)
    }
    fn inputs(&self, id: &NodeId) -> Value {
        let mut inputs: Map<String, Value> = self.graph.nodes[id]
            .inputs()
            .map(|p| (p.id.0.clone(), Value::Array(vec![])))
            .collect();
        for edge in self.graph.edges.values().filter(|e| &e.target.node == id) {
            let value = self.records[&edge.source.node].outputs.as_ref().unwrap()
                [edge.source.port.as_str()]
            .clone();
            inputs
                .get_mut(edge.target.port.as_str())
                .unwrap()
                .as_array_mut()
                .unwrap()
                .push(value);
        }
        json!({"node":self.graph.nodes[id].data, "inputs":inputs})
    }
    fn validate_outputs(&self, id: &NodeId, outputs: &Value) -> Result<(), String> {
        let map = outputs
            .as_object()
            .ok_or("runner output must be an object keyed by output port ID")?;
        for (key, value) in map {
            let port = self.graph.nodes[id]
                .ports
                .iter()
                .find(|p| p.id.as_str() == key && p.direction == PortDirection::Output)
                .ok_or_else(|| format!("unknown output port {key}"))?;
            if !value_matches(&port.data_type, value) {
                return Err(format!("output {key} does not match {:?}", port.data_type));
            }
        }
        for edge in self.graph.edges.values().filter(|e| &e.source.node == id) {
            let value = map
                .get(edge.source.port.as_str())
                .ok_or_else(|| format!("missing connected output {}", edge.source.port))?;
            let target = self.graph.port(&edge.target).unwrap();
            if !value_matches(&target.data_type, value) {
                return Err(format!(
                    "value is incompatible with target {}:{}",
                    edge.target.node, edge.target.port
                ));
            }
        }
        Ok(())
    }
    /// Apply worker events, mark downstream failures, and start at most `concurrency` jobs.
    /// Returns applied worker events; inspect records for Pending/Skipped transitions too.
    pub fn poll(&mut self) -> Vec<Event> {
        let events = self.runtime.poll();
        for event in &events {
            let id = NodeId::from(event.node_id.clone());
            let run = &self.runtime.records[&event.node_id];
            let validation = if run.status == RunStatus::Succeeded {
                self.validate_outputs(&id, run.output.as_ref().unwrap())
            } else {
                Ok(())
            };
            let record = self.records.get_mut(&id).unwrap();
            record.attempt = run.attempt;
            record.progress = run.progress;
            record.message = run.message.clone();
            match run.status {
                RunStatus::Succeeded => match validation {
                    Ok(()) => {
                        record.status = WorkflowStatus::Succeeded;
                        record.outputs = run.output.clone();
                    }
                    Err(e) => {
                        record.status = WorkflowStatus::Failed;
                        record.message = e;
                    }
                },
                RunStatus::Failed => record.status = WorkflowStatus::Failed,
                RunStatus::Cancelled => record.status = WorkflowStatus::Cancelled,
                _ => {}
            }
        }
        if self.cancelled {
            return events;
        }
        // One topological pass propagates failures transitively before scheduling.
        // Its ordering is internal only: ready jobs still launch in NodeId order below.
        for &index in &self.topology.order {
            let id = &self.topology.graph[index];
            if self.records[id].status == WorkflowStatus::Pending
                && self.topology.predecessors(id).any(|predecessor| {
                    matches!(
                        self.records[predecessor].status,
                        WorkflowStatus::Failed
                            | WorkflowStatus::Skipped
                            | WorkflowStatus::Cancelled
                    )
                })
            {
                let r = self.records.get_mut(id).unwrap();
                r.status = WorkflowStatus::Skipped;
                r.message = "Upstream did not succeed".into();
            }
        }
        let active = self
            .records
            .values()
            .filter(|r| r.status == WorkflowStatus::Running)
            .count();
        let ready: Vec<_> = self
            .records
            .iter()
            .filter(|(id, r)| {
                r.status == WorkflowStatus::Pending
                    && self.topology.predecessors(id).all(|predecessor| {
                        self.records[predecessor].status == WorkflowStatus::Succeeded
                    })
            })
            .take(self.concurrency.saturating_sub(active))
            .map(|(id, _)| id.clone())
            .collect();
        for id in ready {
            let input = self.inputs(&id);
            let runner = self.runners[&self.graph.nodes[&id].kind].clone();
            let result = self.runtime.start(id.0.clone(), input, runner);
            let r = self.records.get_mut(&id).unwrap();
            match result {
                Ok(_) => {
                    r.status = WorkflowStatus::Running;
                    r.attempt = self.runtime.records[id.as_str()].attempt;
                    r.message = "Queued".into();
                }
                Err(e) => {
                    r.status = WorkflowStatus::Failed;
                    r.message = e;
                }
            }
        }
        events
    }
}
/// Named types are nominal application contracts: hosts validate their payload in runners.
/// JSON primitive types are checked before downstream execution, including Any -> typed edges.
pub fn value_matches(kind: &DataType, value: &Value) -> bool {
    match kind {
        DataType::Any | DataType::Named(_) => true,
        DataType::Bool => value.is_boolean(),
        DataType::Number => value.is_number(),
        DataType::String => value.is_string(),
        DataType::Object => value.is_object(),
    }
}
