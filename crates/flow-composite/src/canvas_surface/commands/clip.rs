//! Compile clip descriptions per draw, preserving source-over edge coverage.
use crate::{CompositeError, CompositeLimits, DomainNode};
use gpui_flow_scene::{
    Affine, Color, DisplayObject, FillRule, MaskMode, Style,
    display::{Group, Path, PathCommand},
};
use std::sync::Arc;
#[derive(Clone)]
pub(super) struct Clip {
    pub path: Arc<Path>,
    pub rule: FillRule,
    pub transform: Affine,
}
pub(super) struct Budget {
    input_points: usize,
    rendered_points: usize,
    nodes: usize,
    masks: usize,
    path_bytes: u64,
}
impl Default for Budget {
    fn default() -> Self {
        Self {
            input_points: 0,
            rendered_points: 0,
            nodes: 1,
            masks: 0,
            path_bytes: 0,
        }
    }
}
impl Budget {
    pub fn remaining_points(&self, limits: CompositeLimits) -> usize {
        limits
            .max_total_points
            .saturating_sub(self.input_points.max(self.rendered_points))
    }
    pub fn remaining_path_bytes(&self, limits: CompositeLimits) -> u64 {
        limits.max_path_bytes.saturating_sub(self.path_bytes)
    }
    pub fn lowered_path(
        &mut self,
        path: &Path,
        limits: CompositeLimits,
    ) -> Result<(), CompositeError> {
        self.input(path, limits)?;
        self.rendered_points = self.rendered_points.saturating_add(path.commands.len());
        if self.rendered_points > limits.max_total_points {
            return Err(CompositeError::InputLimit);
        }
        self.path_allocation(path.commands.capacity(), limits)?;
        // Path validation computes analytic curve bounds into temporary points.
        // At most four extrema points per command, including Vec growth.
        self.retained_path_bytes(
            (path.commands.len() as u64)
                .saturating_mul(8 * size_of::<gpui_flow_scene::Point>() as u64)
                .saturating_add(3 * size_of::<gpui_flow_scene::Point>() as u64),
            limits,
        )
    }
    pub fn input(&mut self, path: &Path, limits: CompositeLimits) -> Result<(), CompositeError> {
        self.input_count(path.commands.len(), limits)
    }
    pub fn input_count(
        &mut self,
        points: usize,
        limits: CompositeLimits,
    ) -> Result<(), CompositeError> {
        self.input_points = self.input_points.saturating_add(points);
        if points > limits.max_points || self.input_points > limits.max_total_points {
            Err(CompositeError::InputLimit)
        } else {
            Ok(())
        }
    }
    pub fn retained_path_bytes(
        &mut self,
        bytes: u64,
        limits: CompositeLimits,
    ) -> Result<(), CompositeError> {
        self.path_bytes = self.path_bytes.saturating_add(bytes);
        if self.path_bytes > limits.max_path_bytes {
            Err(CompositeError::WorkingSetLimit)
        } else {
            Ok(())
        }
    }
    pub fn path_allocation(
        &mut self,
        points: usize,
        limits: CompositeLimits,
    ) -> Result<(), CompositeError> {
        self.retained_path_bytes(
            (points as u64).saturating_mul(size_of::<PathCommand>() as u64),
            limits,
        )
    }
    pub fn draw(
        &mut self,
        points: usize,
        clips: &[Clip],
        limits: CompositeLimits,
    ) -> Result<(), CompositeError> {
        self.path_allocation(points, limits)?;
        for clip in clips {
            self.path_allocation(clip.path.commands.len(), limits)?;
        }
        self.nodes = self.nodes.saturating_add(1 + 2 * clips.len());
        self.masks = self.masks.saturating_add(clips.len());
        self.rendered_points = self.rendered_points.saturating_add(points);
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
pub(super) fn wrap(mut draw: DomainNode, clips: &[Clip], index: usize) -> DomainNode {
    for (depth, clip) in clips.iter().enumerate().rev() {
        let mut mask = DisplayObject::new(
            format!("canvas-clip-{index}-{depth}"),
            clip.path.as_ref().clone(),
        )
        .with_transform(clip.transform)
        .with_style(Style {
            fill: Some(Color::WHITE.into()),
            fill_rule: clip.rule,
            ..Default::default()
        });
        mask.mask = MaskMode::Alpha;
        draw = DomainNode {
            stroke_scale: gpui_flow_scene::StrokeScaleData::IDENTITY,
            object: DisplayObject::new(format!("canvas-clip-scope-{index}-{depth}"), Group::new()),
            world_transform: Affine::IDENTITY,
            children: vec![
                DomainNode {
                    stroke_scale: gpui_flow_scene::StrokeScaleData::IDENTITY,
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
