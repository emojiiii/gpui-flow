//! Non-separable W3C blend equations, on normalized encoded RGB (not linear light).
//! https://www.w3.org/TR/compositing-1/#blendingnonseparable
//! tiny-skia 0.11.4's highp clip_color uses max>=0 where the lower-gamut
//! condition needs min>=0: opaque red over blue returns red207, not red94.
//! Keep its rasterizer/separable modes, but narrowly adapt these four modes.
use gpui_flow_scene::BlendMode;

fn lum(c: [f64; 3]) -> f64 {
    c[0] * 0.3 + c[1] * 0.59 + c[2] * 0.11
}
fn sat(c: [f64; 3]) -> f64 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}
fn set_sat(c: [f64; 3], s: f64) -> [f64; 3] {
    let min = c[0].min(c[1]).min(c[2]);
    let span = sat(c);
    if span == 0. {
        [0.; 3]
    } else {
        c.map(|v| (v - min) * s / span)
    }
}
fn set_lum(mut c: [f64; 3], target: f64) -> [f64; 3] {
    let delta = target - lum(c);
    c = c.map(|v| v + delta);
    let min = c[0].min(c[1]).min(c[2]);
    let max = c[0].max(c[1]).max(c[2]);
    if min < 0. {
        c = c.map(|v| target + (v - target) * target / (target - min));
    }
    if max > 1. {
        c = c.map(|v| target + (v - target) * (1. - target) / (max - target));
    }
    c
}
pub(crate) fn is_nonseparable(mode: BlendMode) -> bool {
    matches!(
        mode,
        BlendMode::Hue | BlendMode::Saturation | BlendMode::Color | BlendMode::Luminosity
    )
}
/// Premultiplied RGBA8 source-over blend, then clip coverage interpolation.
/// Opacity/coverage are renderer-validated [0,1]. No allocation per pixel.
pub(crate) fn pixel(dst: &mut [u8], src: &[u8], opacity: f32, coverage: u8, mode: BlendMode) {
    if src[3] == 0 || coverage == 0 || opacity == 0. {
        return;
    }
    let sa = src[3] as f64 / 255. * opacity as f64;
    let da = dst[3] as f64 / 255.;
    let source = [src[0], src[1], src[2]].map(|v| v as f64 / src[3] as f64);
    let back = if dst[3] == 0 {
        [0.; 3]
    } else {
        [dst[0], dst[1], dst[2]].map(|v| v as f64 / dst[3] as f64)
    };
    let blend = match mode {
        BlendMode::Hue => set_lum(set_sat(source, sat(back)), lum(back)),
        BlendMode::Saturation => set_lum(set_sat(back, sat(source)), lum(back)),
        BlendMode::Color => set_lum(source, lum(back)),
        BlendMode::Luminosity => set_lum(back, lum(source)),
        _ => unreachable!("only non-separable modes use this adapter"),
    };
    let coverage = coverage as f64 / 255.;
    let alpha = sa + da - sa * da;
    for i in 0..3 {
        let value = sa * (1. - da) * source[i] + da * (1. - sa) * back[i] + sa * da * blend[i];
        dst[i] = ((value * coverage + dst[i] as f64 / 255. * (1. - coverage)).clamp(0., 1.) * 255.)
            .round() as u8;
    }
    dst[3] = ((alpha * coverage + da * (1. - coverage)).clamp(0., 1.) * 255.).round() as u8;
    for i in 0..3 {
        dst[i] = dst[i].min(dst[3]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn alpha_coverage_and_transparency_are_applied_once() {
        for mode in [
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ] {
            let mut clear = [0; 4];
            pixel(&mut clear, &[128, 0, 0, 128], 0.5, 255, mode);
            assert_eq!(clear, [64, 0, 0, 64]);
            let mut keep = [0, 0, 128, 128];
            pixel(&mut keep, &[0; 4], 1., 255, mode);
            assert_eq!(keep, [0, 0, 128, 128]);
            let mut keep = [0, 0, 255, 255];
            pixel(&mut keep, &[255, 0, 0, 255], 1., 0, mode);
            assert_eq!(keep, [0, 0, 255, 255]);
        }
        let mut half = [0, 0, 255, 255];
        pixel(&mut half, &[255, 0, 0, 255], 1., 128, BlendMode::Color);
        assert_eq!(half, [47, 0, 127, 255]);
    }
    #[test]
    fn grayscale_equal_channels_do_not_divide_by_zero_or_break_premultiplication() {
        for mode in [
            BlendMode::Hue,
            BlendMode::Saturation,
            BlendMode::Color,
            BlendMode::Luminosity,
        ] {
            for alpha in [1, 64, 128, 255] {
                let mut dst = [alpha, alpha, alpha, alpha];
                pixel(&mut dst, &[128, 128, 128, 128], 0.5, 128, mode);
                assert!(dst[..3].iter().all(|v| *v <= dst[3]));
                assert_eq!(dst[0], dst[1]);
                assert_eq!(dst[1], dst[2]);
            }
        }
    }
}
