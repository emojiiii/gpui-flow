//! Keep native uniform metrics unchanged; affine text uses local raster space.
use super::*;
pub(super) struct RasterSpace {
    pub matrix: Affine,
    pub clip: Bounds,
    pub exact_local_clip: Option<Bounds>,
}
pub(super) fn space(
    matrix: Affine,
    content: Bounds,
    viewport: RasterViewport,
    full_source: bool,
) -> Result<Option<RasterSpace>, CompositeError> {
    let inverse = matrix
        .inverse()
        .ok_or(CompositeError::UnsupportedTransform)?;
    if positive_uniform(matrix) {
        let bounds = [
            matrix.tx + content.origin.x * matrix.a,
            matrix.ty + content.origin.y * matrix.d,
            content.size.width * matrix.a,
            content.size.height * matrix.d,
        ];
        return Ok(
            source_clip(full_source, bounds, viewport).map(|clip| RasterSpace {
                matrix,
                clip,
                exact_local_clip: None,
            }),
        );
    }
    // Existing Affine::max_scale is the largest singular value. Isotropic
    // local rasterization at this density avoids undersampling the stretched
    // axis; the compositor then maps actual glyph pixels through the affine.
    let scale = matrix.max_scale();
    if !scale.is_finite() || scale <= 0. || scale > 1_000_000. {
        return Err(CompositeError::UnsupportedTransform);
    }
    let raster = Affine::scale(scale, scale);
    let local_clip = if full_source {
        content
    } else {
        let Some(visible) = intersection(viewport.canvas_bounds, viewport.content_mask) else {
            return Ok(None);
        };
        let projected = inverse.transform_bounds(visible);
        if !projected.is_valid() {
            return Err(CompositeError::UnsupportedTransform);
        }
        let Some(clip) = intersection(content, projected) else {
            return Ok(None);
        };
        clip
    };
    let clip = raster.transform_bounds(local_clip);
    if !clip.is_valid() || !clip.size.is_positive() {
        return Err(CompositeError::UnsupportedTransform);
    }
    Ok(Some(RasterSpace {
        matrix: raster,
        clip,
        exact_local_clip: Some(content),
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn viewport() -> RasterViewport {
        let b = Bounds::new(Point::ZERO, Size::new(100., 100.));
        RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: b,
            content_mask: b,
            device_scale: 2.,
        }
    }
    #[test]
    fn affine_text_space_keeps_uniform_metrics_exact() {
        let content = Bounds::new(Point::new(2., 3.), Size::new(40., 20.));
        let m = Affine::translation(10., -5.) * Affine::scale(2., 2.);
        let p = space(m, content, viewport(), false).unwrap().unwrap();
        assert_eq!(p.matrix, m);
        assert_eq!(p.clip, viewport().content_mask);
        assert_eq!(p.exact_local_clip, None);
    }
    #[test]
    fn affine_text_space_uses_existing_maximum_scale_and_inverse_visibility() {
        let content = Bounds::new(Point::ZERO, Size::new(80., 50.));
        let m = Affine::new(0., 2., -3., 0., 90., 10.);
        let p = space(m, content, viewport(), false).unwrap().unwrap();
        assert_eq!(p.matrix, Affine::scale(3., 3.));
        assert_eq!(p.exact_local_clip, Some(content));
        assert_eq!(p.clip.origin, Point::ZERO);
        assert!((p.clip.size.width - 135.).abs() < 0.0001);
        assert!((p.clip.size.height - 90.).abs() < 0.0001);
        let p = space(m, content, viewport(), true).unwrap().unwrap();
        assert_eq!(p.clip, Bounds::new(Point::ZERO, Size::new(240., 150.)));
    }
    #[test]
    fn affine_text_space_rejects_singular_extreme_and_skips_offscreen_sources() {
        let b = Bounds::new(Point::ZERO, Size::new(20., 20.));
        for m in [
            Affine::scale(0., 1.),
            Affine::new(f32::NAN, 0., 0., 1., 0., 0.),
            Affine::scale(1_000_001., -1.),
        ] {
            assert!(space(m, b, viewport(), false).is_err());
        }
        assert!(
            space(
                Affine::translation(-100., -100.) * Affine::scale(-1., 1.),
                b,
                viewport(),
                false
            )
            .unwrap()
            .is_none()
        );
        assert!(
            space(
                Affine::translation(-100., -100.) * Affine::scale(-1., 1.),
                b,
                viewport(),
                true
            )
            .unwrap()
            .is_some()
        );
    }
}
