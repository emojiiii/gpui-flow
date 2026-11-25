use super::*;
use gpui_flow_core::{AgentEditError, AgentEditRequest, AgentOperation, AgentSnapshot};
use serde_json::Value;

impl FlowCanvas {
    /// Register the assets this canvas exposes by ID to agent edits. Paths are
    /// provided by the host and are stored in the native workflow document.
    pub fn set_agent_assets(&mut self, assets: BTreeMap<String, String>) {
        self.agent_asset_paths = assets;
    }

    pub(super) fn project_agent_snapshot(&self) -> AgentSnapshot<'_> {
        let mut snapshot = self.editor.agent_snapshot();
        let graph = self.editor.graph();
        for node in graph.nodes.values() {
            for &key in asset_fields(&node.kind) {
                let Some(path) = asset_path(&node.data, key) else {
                    continue;
                };
                let asset_id = self
                    .agent_asset_paths
                    .iter()
                    .find_map(|(id, registered)| (registered == path).then_some(id));
                let data = snapshot
                    .graph
                    .to_mut()
                    .nodes
                    .get_mut(&node.id)
                    .expect("snapshot contains the projected node")
                    .data
                    .as_object_mut()
                    .expect("asset path belongs to object data");
                if let Some(id) = asset_id {
                    data.insert(key.into(), id.clone().into());
                } else {
                    data.remove(key);
                }
            }
        }
        snapshot
    }

    pub(super) fn resolve_agent_assets(
        &self,
        request: &mut AgentEditRequest,
    ) -> Result<(), AgentEditError> {
        let mut kinds = BTreeMap::new();
        for (index, operation) in request.operations.iter_mut().enumerate() {
            let (kind, data) = match operation {
                AgentOperation::AddNode { id, kind, data, .. } => {
                    let kind = kind.as_deref().unwrap_or("default");
                    kinds.insert(id.clone(), Some(kind));
                    (Some(kind), data)
                }
                AgentOperation::UpdateNode {
                    id,
                    kind: Some(kind),
                    ..
                } => {
                    kinds.insert(id.clone(), Some(kind.as_str()));
                    continue;
                }
                AgentOperation::RemoveNode { id } => {
                    kinds.insert(id.clone(), None);
                    continue;
                }
                AgentOperation::SetNodeData { id, data } => {
                    let kind = kinds.get(id).copied().unwrap_or_else(|| {
                        self.editor.graph().node(id).map(|node| node.kind.as_str())
                    });
                    (kind, data)
                }
                _ => continue,
            };
            if let Some(kind) = kind
                && let Err(message) = resolve_data(kind, data, &self.agent_asset_paths)
            {
                let mut error = AgentEditError::policy_denied(message);
                error.operation_index = Some(index);
                return Err(error);
            }
        }
        Ok(())
    }

    pub(super) fn validate_agent_assets(
        before: &Graph,
        candidate: &Graph,
        assets: &BTreeMap<String, String>,
    ) -> Result<(), String> {
        for node in candidate.nodes.values() {
            let previous = before.node(&node.id);
            if let Some(old) = previous
                && old.kind != node.kind
            {
                for &key in asset_fields(&old.kind) {
                    if !asset_fields(&node.kind).contains(&key)
                        && let Some(path) = asset_path(&old.data, key)
                        && asset_path(&node.data, key) == Some(path)
                    {
                        return Err(format!(
                            "clear node \"{}\" data.{key} when changing its kind",
                            node.id
                        ));
                    }
                }
            }
            let previous = previous.filter(|old| old.kind == node.kind);
            for &key in asset_fields(&node.kind) {
                if let Some(path) = asset_path(&node.data, key)
                    && !previous.is_some_and(|old| asset_path(&old.data, key) == Some(path))
                    && !assets.values().any(|registered| registered == path)
                {
                    return Err(format!(
                        "node \"{}\" data.{key} needs a registered asset ID",
                        node.id
                    ));
                }
            }
        }
        Ok(())
    }
}

fn asset_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "image" | "audio" => &["source"],
        "video" => &["source", "poster"],
        _ => &[],
    }
}

fn asset_path<'a>(data: &'a Value, key: &str) -> Option<&'a str> {
    data.get(key)
        .and_then(Value::as_str)
        .filter(|path| !path.is_empty())
}

fn resolve_data(
    kind: &str,
    data: &mut Value,
    assets: &BTreeMap<String, String>,
) -> Result<(), String> {
    for &key in asset_fields(kind) {
        if let Some(value) = data.get_mut(key) {
            match value {
                Value::Null => {}
                Value::String(id) if id.is_empty() => {}
                Value::String(id) => {
                    let path = assets
                        .get(id)
                        .ok_or_else(|| format!("data.{key} needs a registered asset ID"))?;
                    *id = path.clone();
                }
                _ => return Err(format!("data.{key} expects an asset ID string or null")),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[::core::prelude::v1::test]
    fn agent_assets_resolve_ids_and_reject_direct_paths() {
        let assets = BTreeMap::from([("cover".into(), "private/cover.png".into())]);
        let mut data = json!({"source": "cover", "caption": "Cover"});
        resolve_data("image", &mut data, &assets).unwrap();
        assert_eq!(
            data,
            json!({"source": "private/cover.png", "caption": "Cover"})
        );
        assert!(resolve_data("image", &mut data, &assets).is_err());
        assert!(
            resolve_data(
                "video",
                &mut json!({"poster": "private/other.png"}),
                &assets
            )
            .is_err()
        );
    }

    #[::core::prelude::v1::test]
    fn kind_change_cannot_activate_an_unregistered_path() {
        let mut graph = Graph::new();
        let mut node = Node::new("local", "Local", model::Point::ZERO);
        node.data = json!({"source": "private/cover.png"});
        graph.add_node(node).unwrap();
        let mut candidate = graph.clone();
        let id = NodeId::new("local");
        candidate.nodes.get_mut(&id).unwrap().kind = "image".into();
        assert!(FlowCanvas::validate_agent_assets(&graph, &candidate, &BTreeMap::new()).is_err());
        assert!(
            FlowCanvas::validate_agent_assets(
                &graph,
                &candidate,
                &BTreeMap::from([("cover".into(), "private/cover.png".into())])
            )
            .is_ok()
        );
        assert!(
            FlowCanvas::validate_agent_assets(&candidate, &candidate, &BTreeMap::new()).is_ok()
        );
        assert!(FlowCanvas::validate_agent_assets(&candidate, &graph, &BTreeMap::new()).is_err());
        graph.nodes.get_mut(&id).unwrap().data = json!({});
        assert!(FlowCanvas::validate_agent_assets(&candidate, &graph, &BTreeMap::new()).is_ok());
    }
}
