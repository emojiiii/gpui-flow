use super::*;
use gpui_flow_interaction::{
    HitTarget,
    scene::{
        GEOMETRY_TOOLS_PLUGIN, GeometryHandle, GeometryToolMode, SceneGeometryTools,
        SceneInteractionError, hit_geometry_handle, plugins::SceneGeometryToolsPlugin,
    },
};

impl SceneView {
    /// Opt into one Flow geometry tool, or return to ordinary outer handles.
    /// Switching modes cancels an unfinished drag and preserves completed edits.
    pub fn set_geometry_tool(
        &mut self,
        mode: Option<GeometryToolMode>,
        cx: &mut Context<Self>,
    ) -> Result<(), SceneInteractionError> {
        self.cancel_input(cx);
        self.close_image_crop_editor(cx);
        match mode {
            Some(mode) => self.replace_plugin(SceneGeometryToolsPlugin::new(mode), cx)?,
            None if self
                .interactions
                .plugin_ids()
                .contains(&GEOMETRY_TOOLS_PLUGIN) =>
            {
                self.uninstall_plugin(GEOMETRY_TOOLS_PLUGIN, cx)?
            }
            None => {}
        }
        Ok(())
    }
    pub fn geometry_tool(&self) -> Option<GeometryToolMode> {
        self.interactions.geometry_tool_mode()
    }
    pub(super) fn geometry_tools(&self) -> Option<SceneGeometryTools> {
        if self.inner_tool_active()
            || self.image_crop_editor_active()
            || !self.is_editable()
            || self.tools.as_ref().is_some_and(|tools| {
                !tools.outer_tool_active(gpui_flow_interaction::scene::DEFAULT_OUTER_TOOL)
            })
        {
            return None;
        }
        self.interactions
            .geometry_tools(&self.editor, self.presentation_hit_geometry()?)
    }
    pub(super) fn geometry_handles(&self) -> Option<(DisplayId, Vec<GeometryHandle>)> {
        let geometry = self.geometry_tools()?;
        let id = self.editor.engine.instance(geometry.target).ok()?.id;
        Some((id, geometry.screen_handles(&self.editor)?))
    }
    pub(super) fn geometry_hit(&self, point: WorldPoint) -> Option<HitTarget> {
        let (id, handles) = self.geometry_handles()?;
        let handle = hit_geometry_handle(&handles, point)?;
        Some(HitTarget::GeometryControl {
            id: id.to_string(),
            control: handle.control,
        })
    }
}
