//! Text admission; this crate never measures or fabricates glyphs.
use super::{State, clip, matrix};
use crate::{CompositeError, CompositeLimits, DomainNode};
use gpui_flow_scene::{Affine, Color, DisplayObject, Point, Style, display::Text};
#[allow(clippy::too_many_arguments)]
pub(super) fn compile(
    text: &Text,
    position: Point,
    color: Color,
    state: &State,
    budget: &mut clip::Budget,
    total_bytes: &mut usize,
    index: usize,
    limits: CompositeLimits,
) -> Result<DomainNode, CompositeError> {
    *total_bytes = total_bytes
        .saturating_add(text.content.len())
        .saturating_add(text.font_family.len());
    if text.content.len() > 65_536 || text.font_family.len() > 512 || *total_bytes > 262_144 {
        return Err(CompositeError::InputLimit);
    }
    text.validate().map_err(CompositeError::InvalidGeometry)?;
    if [position.x, position.y, text.size.width, text.size.height]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
    {
        return Err(CompositeError::UnsupportedTransform);
    }
    let world = matrix(state.transform * Affine::translation(position.x, position.y))?;
    budget.draw(5, &state.clips, limits)?;
    let object = DisplayObject::new(format!("canvas-text-{index}"), text.clone())
        .with_transform(world)
        .with_style(Style {
            fill: Some(color.into()),
            opacity: state.opacity,
            ..Default::default()
        });
    object.validate().map_err(CompositeError::InvalidGeometry)?;
    Ok(clip::wrap(
        DomainNode {
            object,
            world_transform: world,
            children: vec![],
        },
        &state.clips,
        index,
    ))
}
