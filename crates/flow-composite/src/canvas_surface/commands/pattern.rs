//! Runtime-only immutable image patterns. Scene Paint stays serializable.
use super::{CurrentPath, State, clip, matrix};
use crate::{CompositeError, CompositeLimits, DomainNode, ImageInputs, StaticImage};
use gpui_flow_scene::{
    Affine, Color, DisplayId, DisplayObject, FillRule, Stroke, StrokeCap, StrokeJoin, Style,
    display::{Path, Rect},
};
use std::{borrow::Cow, sync::Arc};

/// Origin-anchored repetition in both directions, or one source-image extent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CanvasPatternRepeat {
    Repeat,
    NoRepeat,
}

/// An immutable source snapshot and independently replaceable placement matrix.
/// Clones share pixels but copy placement. Each submitted command owns its copy.
#[derive(Clone, Debug)]
pub struct CanvasPattern {
    image: Arc<StaticImage>,
    repetition: CanvasPatternRepeat,
    transform: Affine,
}
impl PartialEq for CanvasPattern {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.image, &other.image)
            && self.repetition == other.repetition
            && self.transform == other.transform
    }
}
impl CanvasPattern {
    /// Replace, never multiply, the source-pixel-to-local transform.
    /// Finite singular matrices are accepted and paint transparently. Invalid
    /// coefficients leave the previous transform intact.
    pub fn set_transform(&mut self, transform: Affine) -> Result<(), CompositeError> {
        bounded_matrix(transform)?;
        self.transform = transform;
        Ok(())
    }
    pub fn transform(&self) -> Affine {
        self.transform
    }
    pub fn repetition(&self) -> CanvasPatternRepeat {
        self.repetition
    }
}
fn bounded_matrix(value: Affine) -> Result<(), CompositeError> {
    if [value.a, value.b, value.c, value.d, value.tx, value.ty]
        .iter()
        .any(|v| !v.is_finite() || v.abs() > 1_000_000.)
    {
        Err(CompositeError::UnsupportedTransform)
    } else {
        Ok(())
    }
}
fn admit_image(image: &StaticImage, limits: CompositeLimits) -> Result<(), CompositeError> {
    if image.width() > limits.max_dimension
        || image.height() > limits.max_dimension
        || u64::from(image.width()) * u64::from(image.height()) > limits.max_pixels
    {
        return Err(CompositeError::TooLarge);
    }
    if image.byte_len() > limits.max_surface_bytes {
        return Err(CompositeError::WorkingSetLimit);
    }
    Ok(())
}
impl super::CanvasSurface {
    /// Resolve and capture decoded pixels now, independent of later input-key
    /// replacement/removal. No surface or tiled bitmap is allocated.
    pub fn create_pattern(
        &self,
        image: &DisplayId,
        inputs: &ImageInputs,
        repetition: CanvasPatternRepeat,
    ) -> Result<CanvasPattern, CompositeError> {
        if image.0.len() > 512 {
            return Err(CompositeError::InputLimit);
        }
        let image = inputs.get(image)?;
        admit_image(&image, self.limits)?;
        Ok(CanvasPattern {
            image,
            repetition,
            transform: Affine::IDENTITY,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum CanvasPathTarget {
    Current,
    Explicit(Path),
}
/// Centered local-space stroke geometry, without an unused scene paint field.
#[derive(Clone, Debug, PartialEq)]
pub struct CanvasStrokeOptions {
    pub width: f32,
    pub cap: StrokeCap,
    pub join: StrokeJoin,
    pub miter_limit: f32,
    pub dash_pattern: Vec<f32>,
    pub dash_offset: f32,
}
impl Default for CanvasStrokeOptions {
    fn default() -> Self {
        let stroke = Stroke::default();
        Self {
            width: stroke.width,
            cap: stroke.cap,
            join: stroke.join,
            miter_limit: stroke.miter_limit,
            dash_pattern: stroke.dash_pattern,
            dash_offset: stroke.dash_offset,
        }
    }
}
impl CanvasStrokeOptions {
    pub(super) fn stroke(&self) -> Result<Stroke, CompositeError> {
        if self.dash_pattern.len() > gpui_flow_scene::MAX_STROKE_DASH_ENTRIES {
            return Err(CompositeError::InputLimit);
        }
        Ok(Stroke {
            width: self.width,
            cap: self.cap,
            join: self.join,
            miter_limit: self.miter_limit,
            dash_pattern: self.dash_pattern.clone(),
            dash_offset: self.dash_offset,
            ..Stroke::new(Color::WHITE, 1.)
        })
    }
}

pub(super) struct Compiler<'a> {
    pub images: &'a mut ImageInputs,
    pub budget: &'a mut clip::Budget,
    pub smooth: bool,
    pub limits: CompositeLimits,
}
impl Compiler<'_> {
    #[allow(clippy::too_many_arguments)]
    pub fn compile(
        &mut self,
        target: &CanvasPathTarget,
        pattern: &CanvasPattern,
        rule: FillRule,
        stroke: Option<&CanvasStrokeOptions>,
        current: &CurrentPath,
        state: &State,
        index: usize,
    ) -> Result<DomainNode, CompositeError> {
        // Revalidate against this target's possibly reduced limits, including
        // patterns created by a different Canvas. Zero-opacity is no bypass.
        admit_image(&pattern.image, self.limits)?;
        let singular = pattern.transform.determinant() == 0.;
        let pattern_world = state.transform * pattern.transform;
        bounded_matrix(pattern_world)?;
        if !singular {
            matrix(pattern_world)?;
        }
        let path = match target {
            CanvasPathTarget::Current => {
                self.budget.input(current.path(), self.limits)?;
                Cow::Owned(current.local_snapshot(state.transform, self.budget, self.limits)?)
            }
            CanvasPathTarget::Explicit(path) => {
                self.budget.input(path, self.limits)?;
                Cow::Borrowed(path)
            }
        };
        let style = Style {
            fill: if stroke.is_none() {
                Some(Color::WHITE.into())
            } else {
                None
            },
            stroke: stroke.map(CanvasStrokeOptions::stroke).transpose()?,
            fill_rule: rule,
            opacity: if singular { 0. } else { state.opacity },
            ..Default::default()
        };
        let mut clips = state.clips.clone();
        if pattern.repetition == CanvasPatternRepeat::NoRepeat && !singular {
            // The extent and sampling are independent: even NoRepeat uses
            // wrapped filtering; clipping does not change edge sample colors.
            self.budget.input_count(5, self.limits)?;
            self.budget.path_allocation(5, self.limits)?;
            let rect = Rect::new(pattern.image.width() as f32, pattern.image.height() as f32)
                .to_path()
                .map_err(CompositeError::InvalidGeometry)?;
            clips.push(clip::Clip {
                path: Arc::new(rect),
                rule: FillRule::NonZero,
                transform: pattern_world,
            });
        }
        self.budget.draw(path.commands.len(), &clips, self.limits)?;
        let object = DisplayObject::new(format!("canvas-pattern-{index}"), path.into_owned())
            .with_transform(state.transform)
            .with_style(style);
        object.validate().map_err(CompositeError::InvalidGeometry)?;
        self.images.insert_pattern(
            object.id.clone(),
            pattern.image.clone(),
            pattern.transform,
            self.smooth,
        );
        if self.images.byte_len() > self.limits.max_surface_bytes {
            return Err(CompositeError::WorkingSetLimit);
        }
        Ok(clip::wrap(
            DomainNode {
                object,
                world_transform: state.transform,
                children: vec![],
            },
            &clips,
            index,
        ))
    }
}
