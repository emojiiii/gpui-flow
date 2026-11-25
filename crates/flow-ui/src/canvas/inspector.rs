use super::*;

impl FlowCanvas {
    pub(super) fn sync_input(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self
            .editor
            .selection
            .nodes
            .iter()
            .next()
            .and_then(|id| self.editor.graph().node(id))
            .and_then(|node| input_key(node).map(|key| (node, key)));
        let Some((node, key)) = selected else {
            self.input_binding = None;
            self.input = None;
            self.input_subscription = None;
            return;
        };
        let binding = (node.id.clone(), key);
        let value = node
            .data
            .get(key)
            .and_then(|value| value.as_str())
            .unwrap_or("")
            .to_owned();
        if self.input_binding.as_ref() == Some(&binding)
            && let Some(input) = &self.input
        {
            if input.read(cx).value().as_ref() != value {
                input.update(cx, |input, cx| input.set_value(value, window, cx));
            }
            return;
        }
        self.input_binding = Some(binding.clone());
        let (id, _) = binding;
        let input = cx.new(|cx| {
            InputState::new(window, cx)
                .default_value(value)
                .placeholder(key)
        });
        self.input_subscription = Some(cx.subscribe_in(
            &input,
            window,
            move |this, state, event: &InputEvent, _, cx| {
                if matches!(event, InputEvent::Change)
                    && this.editor.selection.nodes.iter().next() == Some(&id)
                    && let Some(node) = this.editor.graph().node(&id)
                    && input_key(node) == Some(key)
                    && let Some(mut data) = node.data.as_object().cloned()
                {
                    data.insert(key.into(), state.read(cx).value().to_string().into());
                    let _ = this.execute_command(
                        gpui_flow_interaction::CanvasCommand::SetNodeData {
                            node: id.clone(),
                            data: data.into(),
                        },
                        cx,
                    );
                }
            },
        ));
        self.input = Some(input);
    }
}

fn input_key(node: &Node) -> Option<&'static str> {
    if node.locked || !node.data.is_object() {
        return None;
    }
    match node.kind.as_str() {
        "image" | "audio" | "video" => Some("source"),
        "generation" | "agent" => Some("prompt"),
        kind if BUILTIN_KINDS.contains(&kind) => Some("text"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn inspector_binding_follows_kind_and_editability_of_the_current_node() {
        let mut node = builtin_node("text", "same", model::Point::ZERO);
        assert_eq!(input_key(&node), Some("text"));
        node.kind = "image".into();
        assert_eq!(input_key(&node), Some("source"));
        node.locked = true;
        assert_eq!(input_key(&node), None);
        node.locked = false;
        node.data = serde_json::Value::Null;
        assert_eq!(input_key(&node), None);
        node.data = serde_json::json!({});
        assert_eq!(input_key(&node), Some("source"));
        node.kind = "custom".into();
        assert_eq!(input_key(&node), None);
    }
}
