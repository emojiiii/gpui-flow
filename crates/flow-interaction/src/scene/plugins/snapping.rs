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

/// Optional world-axis alignment to visible, unrelated scene objects. Install
/// after grid snapping; nearest edge/center wins deterministically in paint
/// order, with world-space guides and logical-pixel acceptance distance.
#[derive(Debug, Clone, Copy)]
pub struct SceneEdgeSnappingPlugin {
    pub threshold_pixels: f32,
    pub include_centers: bool,
}
impl Default for SceneEdgeSnappingPlugin {
    fn default() -> Self {
        Self {
            threshold_pixels: 6.0,
            include_centers: true,
        }
    }
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneEdgeSnappingPlugin {
    fn id(&self) -> &str {
        "edge-snapping"
    }
    fn priority(&self) -> i32 {
        60
    }
    fn filter_command(
        &mut self,
        command: &mut SceneCommand,
        cx: &PluginContext<'_, SceneEditor>,
        effects: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        let SceneCommand::MoveSelectionTo { anchor } = command else {
            return Ok(());
        };
        if !self.threshold_pixels.is_finite() || self.threshold_pixels < 0.0 {
            return Err("invalid edge snap tolerance".into());
        }
        let scene = cx.editor.scene();
        let Some(bounds) = crate::scene::selection_bounds(cx.editor).map_err(|e| e.to_string())?
        else {
            return Ok(());
        };
        let bounds = bounds.translated(*anchor - bounds.origin);
        let roots = cx.editor.selection.roots(scene);
        let paths = roots
            .iter()
            .map(|root| scene.ancestor_path(root))
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        let tolerance = self.threshold_pixels / cx.editor.viewport.zoom;
        let mut best_x: Option<(f32, f32)> = None;
        let mut best_y: Option<(f32, f32)> = None;
        for id in scene.paint_order().map_err(|e| e.to_string())? {
            let path = scene.ancestor_path(&id).map_err(|e| e.to_string())?;
            if roots.iter().any(|root| path.contains(root))
                || paths.iter().any(|path| path.contains(&id))
            {
                continue;
            }
            let Some(other) = scene
                .world_bounds_for_view(&id, cx.editor.viewport.zoom)
                .map_err(|e| e.to_string())?
            else {
                continue;
            };
            let count = if self.include_centers { 3 } else { 2 };
            let x = [bounds.min_x(), bounds.max_x(), bounds.center().x];
            let y = [bounds.min_y(), bounds.max_y(), bounds.center().y];
            let other_x = [other.min_x(), other.max_x(), other.center().x];
            let other_y = [other.min_y(), other.max_y(), other.center().y];
            for from in 0..count {
                for to in 0..count {
                    let dx = other_x[to] - x[from];
                    let dy = other_y[to] - y[from];
                    if dx.abs() <= tolerance && best_x.is_none_or(|(old, _)| dx.abs() < old.abs()) {
                        best_x = Some((dx, other_x[to]));
                    }
                    if dy.abs() <= tolerance && best_y.is_none_or(|(old, _)| dy.abs() < old.abs()) {
                        best_y = Some((dy, other_y[to]));
                    }
                }
            }
        }
        let mut guides = vec![];
        if let Some((delta, position)) = best_x {
            anchor.x += delta;
            guides.push(SnapGuide {
                axis: Axis::X,
                position,
            });
        }
        if let Some((delta, position)) = best_y {
            anchor.y += delta;
            guides.push(SnapGuide {
                axis: Axis::Y,
                position,
            });
        }
        // Edge snapping supersedes grid guides only on the axes it actually wins.
        let mut previous = vec![];
        effects.retain(|effect| {
            if let CanvasEffect::Guides(old) = effect {
                previous.extend(
                    old.iter()
                        .filter(|guide| !guides.iter().any(|new| new.axis == guide.axis))
                        .cloned(),
                );
                false
            } else {
                true
            }
        });
        previous.extend(guides);
        effects.push(CanvasEffect::Guides(previous));
        Ok(())
    }
}
