use crate::*;
use gpui_flow_core::{ConnectionDrag, Point, PortRef};
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SnapConfig {
    pub grid_size: Option<f32>,
    pub node_alignment: bool,
    pub threshold_pixels: f32,
    pub port_radius_pixels: f32,
}
impl Default for SnapConfig {
    fn default() -> Self {
        Self {
            grid_size: None,
            node_alignment: true,
            threshold_pixels: 6.0,
            port_radius_pixels: 20.0,
        }
    }
}
#[derive(Default)]
pub struct SnappingPlugin {
    pub config: SnapConfig,
}
impl SnappingPlugin {
    pub fn new(config: SnapConfig) -> Self {
        Self { config }
    }
    pub fn with_grid(mut self, size: f32) -> Self {
        self.config.grid_size = Some(size);
        self
    }
}
impl CanvasPlugin for SnappingPlugin {
    fn id(&self) -> &str {
        "snapping"
    }
    fn priority(&self) -> i32 {
        50
    }
    fn on_event(&mut self, event: &CanvasEvent, _: &PluginContext<'_>, out: &mut PluginOutput) {
        if matches!(event, CanvasEvent::PointerUp(_)) {
            out.effect(CanvasEffect::Guides(vec![]));
        }
    }
    fn filter_command(
        &mut self,
        command: &mut CanvasCommand,
        cx: &PluginContext<'_>,
        effects: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if !self.config.threshold_pixels.is_finite()
            || self.config.threshold_pixels < 0.0
            || !self.config.port_radius_pixels.is_finite()
            || self.config.port_radius_pixels < 0.0
            || self
                .config
                .grid_size
                .is_some_and(|g| !g.is_finite() || g <= 0.0)
        {
            return Err(
                "snap settings must be finite and non-negative; grid size must be positive".into(),
            );
        }
        let hidden = gpui_flow_core::hidden_node_ids(cx.editor.graph());
        match command {
            CanvasCommand::PreviewConnection {
                from,
                position,
                hovered,
            } if hovered.is_none() => {
                let mut drag = ConnectionDrag::default();
                if drag
                    .begin(cx.editor.graph(), from.clone(), *position)
                    .is_err()
                {
                    return Ok(());
                }
                let mut closest = self.config.port_radius_pixels;
                let mut closest_invalid = self.config.port_radius_pixels;
                let mut invalid = None;
                for node in cx
                    .editor
                    .graph()
                    .nodes
                    .values()
                    .filter(|node| !hidden.contains(&node.id))
                {
                    for port in &node.ports {
                        let reference = PortRef::new(node.id.clone(), port.id.clone());
                        if &reference == from {
                            continue;
                        }
                        let Some(anchor) = cx.editor.graph().port_position(&reference) else {
                            continue;
                        };
                        let distance = cx
                            .editor
                            .viewport
                            .world_to_screen(anchor)
                            .distance(*position);
                        if distance <= closest_invalid {
                            closest_invalid = distance;
                            invalid = Some(reference.clone());
                        }
                        if distance <= closest
                            && drag.validate_target(cx.editor.graph(), &reference).is_ok()
                        {
                            closest = distance;
                            *hovered = Some(reference);
                        }
                    }
                }
                if hovered.is_none() {
                    *hovered = invalid;
                }
            }
            CanvasCommand::SetPositions { nodes, groups } => {
                let Some((anchor_id, anchor)) = nodes
                    .iter()
                    .find(|(id, _)| !hidden.contains(*id))
                    .map(|(id, p)| (id.clone(), *p))
                else {
                    return Ok(());
                };
                let Some(node) = cx.editor.graph().nodes.get(&anchor_id) else {
                    return Ok(());
                };
                let tolerance = self.config.threshold_pixels / cx.editor.viewport.zoom;
                let mut dx: Option<(f32, f32)> = None;
                let mut dy: Option<(f32, f32)> = None;
                let mut consider = |axis: Axis, difference: f32, line: f32| {
                    if difference.abs() > tolerance {
                        return;
                    }
                    let target = match axis {
                        Axis::X => &mut dx,
                        Axis::Y => &mut dy,
                    };
                    if target.is_none_or(|(old, _)| difference.abs() < old.abs()) {
                        *target = Some((difference, line));
                    }
                };
                if let Some(grid) = self.config.grid_size {
                    let snapped = anchor.snapped(grid);
                    consider(Axis::X, snapped.x - anchor.x, snapped.x);
                    consider(Axis::Y, snapped.y - anchor.y, snapped.y);
                }
                if self.config.node_alignment {
                    for other in cx.editor.graph().nodes.values() {
                        if nodes.contains_key(&other.id) || hidden.contains(&other.id) {
                            continue;
                        }
                        for a in [
                            anchor.x,
                            anchor.x + node.size.width * 0.5,
                            anchor.x + node.size.width,
                        ] {
                            for b in [
                                other.position.x,
                                other.position.x + other.size.width * 0.5,
                                other.position.x + other.size.width,
                            ] {
                                consider(Axis::X, b - a, b);
                            }
                        }
                        for a in [
                            anchor.y,
                            anchor.y + node.size.height * 0.5,
                            anchor.y + node.size.height,
                        ] {
                            for b in [
                                other.position.y,
                                other.position.y + other.size.height * 0.5,
                                other.position.y + other.size.height,
                            ] {
                                consider(Axis::Y, b - a, b);
                            }
                        }
                    }
                }
                let correction = Point::new(dx.map_or(0.0, |(d, _)| d), dy.map_or(0.0, |(d, _)| d));
                for p in nodes.values_mut() {
                    *p += correction;
                }
                for p in groups.values_mut() {
                    *p += correction;
                }
                let mut guides = vec![];
                if let Some((_, position)) = dx {
                    guides.push(SnapGuide {
                        axis: Axis::X,
                        position,
                    });
                }
                if let Some((_, position)) = dy {
                    guides.push(SnapGuide {
                        axis: Axis::Y,
                        position,
                    });
                }
                effects.push(CanvasEffect::Guides(guides));
            }
            _ => {}
        }
        Ok(())
    }
}
