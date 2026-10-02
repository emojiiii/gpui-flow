//! Runtime Image alpha picking. No decoder, pixel cache, or document properties.
use gpui_flow_scene::{Point, Size, display::Image};

/// Picking while an image has no usable current decoded frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ImageHitFallback {
    /// Keep the engine's geometric candidate (including a visible placeholder).
    #[default]
    Geometry,
    /// Let the next front-to-back candidate receive the pointer.
    PassThrough,
}

/// Optional per-instance Image alpha picking. The default SceneView remains
/// geometric; installing this policy opts one attachment into pixel filtering.
/// This samples source alpha, not final composited opacity, effects, or masks.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ImageAlphaHit {
    /// Accept only alpha strictly greater than this byte value. Zero accepts
    /// every nontransparent source pixel; 255 accepts no loaded source pixels.
    pub threshold: u8,
    /// Policy before the current generation has loaded. Defaults to Geometry.
    pub pending: ImageHitFallback,
    /// Policy for denied, cancelled, failed, or invalid frames. Defaults to Geometry.
    pub failed: ImageHitFallback,
}

/// A nearest source-pixel sample uses centers at (x + .5, y + .5): floor the
/// inverse-fit coordinate, with half-open source/box edges and ties choosing
/// the higher index. This deliberately does not emulate GPU bilinear filtering.
/// Coordinates are Image-local logical units; engine inverse transforms and
/// viewport conversion already remove world affine transforms/zoom. Window DPR
/// is not applied again: these dimensions describe decoded physical pixels.
/// None means invalid input; Some(0) includes letterbox/outside/rounded corners.
pub(crate) fn sample_alpha(
    image: &Image,
    local: Point,
    width: u32,
    height: u32,
    bgra: &[u8],
) -> Option<u8> {
    let bytes = usize::try_from(width)
        .ok()?
        .checked_mul(usize::try_from(height).ok()?)?
        .checked_mul(4)?;
    if width == 0
        || height == 0
        || bgra.len() != bytes
        || !local.is_finite()
        || !image.size.is_positive()
    {
        return None;
    }
    if local.x < 0.
        || local.y < 0.
        || local.x >= image.size.width
        || local.y >= image.size.height
        || !image.contains(local)
    {
        return Some(0);
    }
    // Match native object-fit and the CPU image compositor: actual decoded
    // dimensions override optional, possibly stale natural_size metadata.
    let mut fitted = image.clone();
    fitted.natural_size = Some(Size::new(width as f32, height as f32));
    let content = fitted.content_bounds();
    if !content.is_valid() || !content.size.is_positive() {
        return None;
    }
    let x = (f64::from(local.x) - f64::from(content.origin.x)) * f64::from(width)
        / f64::from(content.size.width);
    let y = (f64::from(local.y) - f64::from(content.origin.y)) * f64::from(height)
        / f64::from(content.size.height);
    if x < 0. || y < 0. || x >= f64::from(width) || y >= f64::from(height) {
        return Some(0);
    }
    let offset = (y.floor() as usize * width as usize + x.floor() as usize) * 4 + 3;
    bgra.get(offset).copied()
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_scene::display::ImageFit;

    fn source() -> Vec<u8> {
        [10, 20, 30, 40, 50, 60, 70, 80]
            .into_iter()
            .flat_map(|a| [91, 42, 17, a])
            .collect()
    }
    fn image(fit: ImageFit) -> Image {
        let mut image = Image::new("shared.png", Size::new(100., 100.));
        image.fit = fit;
        image
    }
    fn at(image: &Image, x: f32, y: f32) -> Option<u8> {
        sample_alpha(image, Point::new(x, y), 4, 2, &source())
    }
    #[test]
    fn contain_uses_centered_letterbox_and_decoded_dimensions() {
        let mut image = image(ImageFit::Contain);
        image.natural_size = Some(Size::new(1., 100.));
        assert_eq!(at(&image, 50., 24.99), Some(0));
        assert_eq!(at(&image, 0., 25.), Some(10));
        assert_eq!(at(&image, 25., 25.), Some(20));
        assert_eq!(at(&image, 99., 74.99), Some(80));
        assert_eq!(at(&image, 50., 75.), Some(0));
    }
    #[test]
    fn cover_maps_cropped_center_without_stretching() {
        let image = image(ImageFit::Cover);
        assert_eq!(at(&image, 0., 0.), Some(20));
        assert_eq!(at(&image, 49.99, 49.99), Some(20));
        assert_eq!(at(&image, 50., 50.), Some(70));
        assert_eq!(at(&image, 99.99, 99.99), Some(70));
    }
    #[test]
    fn fill_uses_full_source_and_half_open_pixel_edges() {
        let image = image(ImageFit::Fill);
        assert_eq!(at(&image, 24.99, 0.), Some(10));
        assert_eq!(at(&image, 25., 0.), Some(20));
        assert_eq!(at(&image, 99.99, 99.99), Some(80));
        for (x, y) in [(100., 50.), (50., 100.), (-0.01, 50.)] {
            assert_eq!(at(&image, x, y), Some(0));
        }
    }
    #[test]
    fn rounded_own_geometry_precedes_pixel_alpha() {
        let image = image(ImageFit::Fill).with_corners([40., 0., 40., 0.]);
        assert_eq!(at(&image, 1., 1.), Some(0));
        assert_eq!(at(&image, 99., 1.), Some(40));
        assert_eq!(at(&image, 99., 99.), Some(0));
        assert_eq!(at(&image, 1., 99.), Some(50));
    }
    #[test]
    fn invalid_frames_are_unavailable_not_transparent() {
        let image = image(ImageFit::Fill);
        assert_eq!(sample_alpha(&image, Point::ZERO, 0, 2, &[]), None);
        assert_eq!(sample_alpha(&image, Point::ZERO, 4, 2, &[0; 31]), None);
        assert_eq!(
            sample_alpha(&image, Point::ZERO, u32::MAX, u32::MAX, &[]),
            None
        );
        assert_eq!(at(&image, f32::NAN, 0.), None);
    }
    #[test]
    fn alpha_is_fourth_byte_and_color_order_does_not_affect_picking() {
        let image = image(ImageFit::Fill);
        assert_eq!(
            sample_alpha(&image, Point::ZERO, 1, 1, &[255, 255, 255, 0]),
            Some(0)
        );
        assert_eq!(
            sample_alpha(&image, Point::ZERO, 1, 1, &[0, 0, 0, 255]),
            Some(255)
        );
    }
}
