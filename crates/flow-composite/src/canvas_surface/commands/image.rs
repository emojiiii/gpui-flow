//! Decoded image draws reuse the existing Canvas texture and clip compositor.
use super::{State, clip, matrix};
use crate::{CompositeError, CompositeLimits, DomainNode, ImageInputs};
use gpui_flow_scene::{
    Affine, Bounds, DisplayId, DisplayObject, FillRule, Point, Size, Style,
    display::{Canvas, Rect},
};
pub(super) struct Compiler<'a> {
    pub inputs: &'a ImageInputs,
    pub images: &'a mut ImageInputs,
    pub budget: &'a mut clip::Budget,
    pub smooth: bool,
    pub limits: CompositeLimits,
}
impl Compiler<'_> {
    pub fn compile(
        &mut self,
        key: &DisplayId,
        crop: Option<Bounds>,
        destination: Bounds,
        state: &State,
        index: usize,
    ) -> Result<DomainNode, CompositeError> {
        if key.0.len() > 512 {
            return Err(CompositeError::InputLimit);
        }
        super::super::validate_bounds(destination)?;
        let pixels = self.inputs.get(key)?;
        if pixels.width() > self.limits.max_dimension
            || pixels.height() > self.limits.max_dimension
            || u64::from(pixels.width()) * u64::from(pixels.height()) > self.limits.max_pixels
        {
            return Err(CompositeError::TooLarge);
        }
        let size = Size::new(pixels.width() as f32, pixels.height() as f32);
        let source = crop.unwrap_or(Bounds::new(Point::ZERO, size));
        super::super::validate_bounds(source)?;
        let transform = matrix(
            state.transform
                * Affine::translation(destination.origin.x, destination.origin.y)
                * Affine::scale(
                    destination.size.width / source.size.width,
                    destination.size.height / source.size.height,
                )
                * Affine::translation(-source.origin.x, -source.origin.y),
        )?;
        // The internal Canvas leaf carries only texture geometry/smoothing. It
        // allocates no surface; its decoded pixels remain shared immutable input.
        let mut canvas = Canvas::new(size.width, size.height);
        canvas.smooth = self.smooth;
        let object = DisplayObject::new(format!("canvas-image-{index}"), canvas)
            .with_transform(transform)
            .with_style(Style {
                fill: None,
                opacity: state.opacity,
                ..Default::default()
            });
        object.validate().map_err(CompositeError::InvalidGeometry)?;
        self.images.insert(object.id.clone(), Ok(pixels));
        if self.images.byte_len() > self.limits.max_surface_bytes {
            return Err(CompositeError::WorkingSetLimit);
        }
        // Crop without copying pixels. The destination clip is independent of
        // the frozen context clips and is discarded immediately after this draw.
        let rect = Rect::new(destination.size.width, destination.size.height)
            .to_path()
            .map_err(CompositeError::InvalidGeometry)?;
        let mut clips = state.clips.clone();
        if crop.is_some() {
            self.budget.input(&rect, self.limits)?;
            clips.push(clip::Clip {
                path: std::sync::Arc::new(rect),
                rule: FillRule::NonZero,
                transform: matrix(
                    state.transform
                        * Affine::translation(destination.origin.x, destination.origin.y),
                )?,
            });
        }
        self.budget.draw(5, &clips, self.limits)?;
        Ok(clip::wrap(
            DomainNode {
                stroke_scale: gpui_flow_scene::StrokeScaleData::IDENTITY,
                object,
                world_transform: transform,
                children: vec![],
            },
            &clips,
            index,
        ))
    }
}
