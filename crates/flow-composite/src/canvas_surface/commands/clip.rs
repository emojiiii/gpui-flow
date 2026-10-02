//! Compile clip descriptions per draw, preserving source-over edge coverage.
use crate::{CompositeError, CompositeLimits, DomainNode};
use gpui_flow_scene::{
    Affine, Color, DisplayObject, FillRule, MaskMode, Style,
    display::{Group, Path},
};
#[derive(Clone)]
pub(super) struct Clip<'a> {
    pub path: &'a Path,
    pub rule: FillRule,
    pub transform: Affine,
}
pub(super) struct Budget {
    input_points: usize,
    rendered_points: usize,
    nodes: usize,
    masks: usize,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            input_points: 0,
            rendered_points: 0,
            nodes: 1,
            masks: 0,
        }
    }
}
impl Budget {
    pub fn input(&mut self, path: &Path, limits: CompositeLimits) -> Result<(), CompositeError> {
        self.input_points = self.input_points.saturating_add(path.commands.len());
        if path.commands.len() > limits.max_points || self.input_points > limits.max_total_points {
            Err(CompositeError::InputLimit)
        } else {
            Ok(())
        }
    }
    pub fn draw(
        &mut self,
        path: &Path,
        clips: &[Clip<'_>],
        limits: CompositeLimits,
    ) -> Result<(), CompositeError> {
        self.nodes = self.nodes.saturating_add(1 + 2 * clips.len());
        self.masks = self.masks.saturating_add(clips.len());
        self.rendered_points = self.rendered_points.saturating_add(path.commands.len());
        for c in clips {
            self.rendered_points = self.rendered_points.saturating_add(c.path.commands.len());
        }
        if self.nodes > limits.max_nodes
            || self.masks > limits.max_mask_runs
            || self.rendered_points > limits.max_total_points
        {
            return Err(CompositeError::InputLimit);
        }
        if clips.len() + 1 > limits.max_depth {
            return Err(CompositeError::NestingLimit);
        }
        Ok(())
    }
}
pub(super) fn wrap(mut draw: DomainNode, clips: &[Clip<'_>], index: usize) -> DomainNode {
    for (depth, clip) in clips.iter().enumerate().rev() {
        let mut mask =
            DisplayObject::new(format!("canvas-clip-{index}-{depth}"), clip.path.clone())
                .with_transform(clip.transform)
                .with_style(Style {
                    fill: Some(Color::WHITE.into()),
                    fill_rule: clip.rule,
                    ..Default::default()
                });
        mask.mask = MaskMode::Alpha;
        draw = DomainNode {
            object: DisplayObject::new(format!("canvas-clip-scope-{index}-{depth}"), Group::new()),
            world_transform: Affine::IDENTITY,
            children: vec![
                DomainNode {
                    object: mask,
                    world_transform: clip.transform,
                    children: vec![],
                },
                draw,
            ],
        };
    }
    draw
}
