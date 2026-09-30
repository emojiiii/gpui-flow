use super::*;

impl FlowCanvas {
    pub(super) fn sync_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.editor.selection.nodes.iter().next().cloned();
        if selected.is_none() {
            self.input_node = None;
            self.input = None;
            self.input_subscription = None;
            return;
        }
        if selected == self.input_node {
            if let (Some(id), Some(input)) = (&selected, &self.input)
                && let Some(node) = self.editor.graph().node(id)
            {
                let key = if matches!(node.kind.as_str(), "image" | "audio" | "video") {
                    "source"
                } else if matches!(node.kind.as_str(), "generation" | "agent") {
                    "prompt"
                } else {
                    "text"
                };
                let value = node
                    .data
                    .get(key)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_owned();
                if input.read(cx).value().as_ref() != value {
                    input.update(cx, |input, cx| input.set_value(value, window, cx));
                }
            }
            return;
        }
        self.input_node = selected.clone();
        self.input = None;
        self.input_subscription = None;
        if let Some(node) = selected
            .and_then(|id| self.editor.graph().node(&id))
            .cloned()
        {
            if !BUILTIN_KINDS.contains(&node.kind.as_str()) || !node.data.is_object() || node.locked
            {
                return;
            }
            let key = if matches!(node.kind.as_str(), "image" | "audio" | "video") {
                "source"
            } else if matches!(node.kind.as_str(), "generation" | "agent") {
                "prompt"
            } else {
                "text"
            };
            let value = node
                .data
                .get(key)
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            let input = cx.new(|cx| {
                InputState::new(window, cx)
                    .default_value(value)
                    .placeholder(key)
            });
            self.input_subscription = Some(cx.subscribe_in(
                &input,
                window,
                move |this, state, event: &InputEvent, _, cx| {
                    if matches!(event, InputEvent::Change) {
                        let value = state.read(cx).value().to_string();
                        let id = node.id.clone();
                        let result = this.editor.transaction("Edit node payload", |g| {
                            let n = g
                                .nodes
                                .get_mut(&id)
                                .ok_or_else(|| model::GraphError::MissingNode(id.clone()))?;
                            if let Some(data) = n.data.as_object_mut() {
                                data.insert(key.into(), serde_json::Value::String(value));
                            }
                            Ok(())
                        });
                        this.report(result, cx);
                    }
                },
            ));
            self.input = Some(input);
        }
    }
}
