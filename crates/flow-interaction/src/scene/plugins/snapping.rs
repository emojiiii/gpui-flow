use crate::scene::SceneCommand;
use crate::*;
use gpui_flow_editor::SceneEditor;
/// Optional, independent scene grid middleware. Coordinates and guides are world-space;
/// acceptance tolerance remains in screen pixels at every viewport zoom level.
pub struct SceneSnappingPlugin {
    pub grid_size: f32,
    pub threshold_pixels: f32,
}
impl Default for SceneSnappingPlugin {
    fn default() -> Self {
        Self {
            grid_size: 20.0,
            threshold_pixels: 6.0,
        }
    }
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneSnappingPlugin {
    fn id(&self) -> &str {
        "snapping"
    }
    fn priority(&self) -> i32 {
        50
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        cx: &PluginContext<'_, SceneEditor>,
        effects: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let SceneCommand::MoveSelectionTo { anchor } = command {
            if !self.grid_size.is_finite()
                || self.grid_size <= 0.0
                || !self.threshold_pixels.is_finite()
                || self.threshold_pixels < 0.0
            {
                return Err("invalid scene snap grid or tolerance".into());
            }
            let snapped = anchor.snapped(self.grid_size);
            let tolerance = self.threshold_pixels / cx.editor.viewport.zoom;
            let mut guides = vec![];
            if (snapped.x - anchor.x).abs() <= tolerance {
                anchor.x = snapped.x;
                guides.push(SnapGuide {
                    axis: Axis::X,
                    position: anchor.x,
                });
            }
            if (snapped.y - anchor.y).abs() <= tolerance {
                anchor.y = snapped.y;
                guides.push(SnapGuide {
                    axis: Axis::Y,
                    position: anchor.y,
                });
            }
            effects.push(CanvasEffect::Guides(guides));
        }
        Ok(())
    }
}
