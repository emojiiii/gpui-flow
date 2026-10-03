use super::*;
impl SceneView {
    pub fn resize_inner_tool(
        &mut self,
        token: gpui_flow_interaction::scene::ToolSessionToken,
        size: gpui_flow_scene::Size,
        anchor: gpui_flow_scene::Point,
        cx: &mut Context<Self>,
    ) -> Result<(), gpui_flow_interaction::scene::SceneInteractionError> {
        let before = self.editor.engine.revision();
        let result = self
            .tools
            .as_mut()
            .ok_or_else(|| {
                gpui_flow_interaction::scene::SceneInteractionError::InvalidInput(
                    "tool controller was released".into(),
                )
            })?
            .resize_inner(
                &mut self.editor,
                &mut self.interactions,
                token,
                size,
                anchor,
            );
        self.emit_tool_changes(before, cx);
        result
    }

    pub fn set_custom_resource_provider(
        &mut self,
        kind: impl Into<String>,
        provider: Option<crate::resources::SceneCustomResourceProvider>,
        cx: &mut Context<Self>,
    ) -> Result<(), gpui_flow_engine::ResourceError> {
        self.resources
            .as_mut()
            .ok_or(gpui_flow_engine::ResourceError::Disposed)?
            .set_custom_resource_provider(&self.editor.engine, kind.into(), provider)?;
        cx.notify();
        Ok(())
    }
}
