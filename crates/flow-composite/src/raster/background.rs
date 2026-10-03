use super::*;

/// Premultiplied encoded-sRGB bytes. Interpolate from original B once; never
/// source-over the captured backdrop onto itself. F has no own final opacity.
fn replacement_pixel(b: &mut [u8; 4], g: &[u8; 4], f: &[u8; 4], m: u8, q: f64) {
    if q == 0. || (m == 0 && f[3] == 0) {
        return;
    }
    let mask = f64::from(m) / 255.;
    let remain = 1. - f64::from(f[3]) / 255.;
    let mut out = [0; 4];
    for c in 0..4 {
        let original = f64::from(b[c]);
        let filtered = (1. - mask) * original + mask * f64::from(g[c]);
        let composed = f64::from(f[c]) + remain * filtered;
        out[c] = ((1. - q) * original + q * composed).round().clamp(0., 255.) as u8;
    }
    for c in 0..3 {
        out[c] = out[c].min(out[3]);
    }
    *b = out;
}

/// Shared by detached-prefix sampling and final crop. No source mutation.
pub(super) fn copy_window(
    source: &Pixmap,
    target: &mut Pixmap,
    offset: [usize; 2],
    mut check: impl FnMut() -> Result<(), CompositeError>,
) -> Result<(), CompositeError> {
    let width = target.width() as usize;
    for (y, row) in target.data_mut().chunks_exact_mut(width * 4).enumerate() {
        check()?;
        let from = ((offset[1] + y) * source.width() as usize + offset[0]) * 4;
        row.copy_from_slice(&source.data()[from..from + row.len()]);
    }
    Ok(())
}
#[allow(clippy::too_many_arguments)]
fn replacement_pixels(
    target: &mut Pixmap,
    filtered: &Pixmap,
    foreground: &Pixmap,
    mask: &Mask,
    clip: Option<&Mask>,
    offset: [i64; 2],
    opacity: f32,
    mut check: impl FnMut() -> Result<(), CompositeError>,
) -> Result<(), CompositeError> {
    let width = target.width();
    for (y, row) in target
        .data_mut()
        .chunks_exact_mut(width as usize * 4)
        .enumerate()
    {
        check()?;
        for (x, pixel) in row.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let index = y * width as usize + x;
            let q = f64::from(opacity) * f64::from(clip.map_or(255, |c| c.data()[index])) / 255.;
            let f = foreground.data()[index * 4..index * 4 + 4]
                .try_into()
                .expect("pixel");
            let m = mask.data()[index];
            let sx = x as i64 - offset[0];
            let sy = y as i64 - offset[1];
            let g = if sx >= 0
                && sy >= 0
                && sx < filtered.width() as i64
                && sy < filtered.height() as i64
            {
                let offset = (sy as usize * filtered.width() as usize + sx as usize) * 4;
                filtered.data()[offset..offset + 4]
                    .try_into()
                    .expect("pixel")
            } else {
                // The contour lies inside S. Its exterior still receives F.
                debug_assert_eq!(m, 0);
                &[0; 4]
            };
            replacement_pixel(pixel, g, f, m, q);
        }
    }
    Ok(())
}

impl Raster<'_> {
    pub(super) fn background_blur(
        &mut self,
        node: &PreparedNode,
        plan: &crate::blur::BlurPlan,
        target: &mut Pixmap,
        depth: usize,
        clip: Option<&Mask>,
    ) -> Result<(), CompositeError> {
        if depth >= self.budget.limits.max_depth {
            return Err(CompositeError::NestingLimit);
        }
        let background = node.background.as_ref().expect("planned background");
        if self.owner.as_ref() != Some(&background.site.owner) {
            return Err(CompositeError::UnsupportedExternalBackdrop);
        }
        debug_assert!(background.site.entry < self.budget.limits.max_nodes);
        let coverage = background.coverage.as_ref().expect("planned contour");
        let p = &plan.surface;
        let ox = p.origin[0] - self.origin[0];
        let oy = p.origin[1] - self.origin[1];
        if ox < 0.
            || oy < 0.
            || ox + p.width as f64 > self.width as f64
            || oy + p.height as f64 > self.height as f64
        {
            return Err(CompositeError::InvalidGeometry(
                "incomplete backdrop sample".into(),
            ));
        }
        self.check()?;
        self.budget.reserve(plan.bytes())?;
        self.budget.write(plan.work())?;
        self.budget.write(p.pixels * 2)?; // detached sample initialization/copy
        let mut sample = Pixmap::from_vec(
            bytes(p.pixels * 4)?,
            tiny_skia::IntSize::from_wh(p.width, p.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        copy_window(target, &mut sample, [ox as usize, oy as usize], || {
            self.check()
        })?;
        let mut foreground = self.surface()?;
        self.budget.reserve(self.pixels)?;
        self.budget.write(self.pixels * 2)?; // A8 clear and contour fill
        let mut mask = Mask::from_vec(
            bytes(self.pixels)?,
            tiny_skia::IntSize::from_wh(self.width, self.height).ok_or(CompositeError::TooLarge)?,
        )
        .ok_or(CompositeError::AllocationFailed)?;
        let previous_owner = self
            .owner
            .replace(crate::background::Owner::Transparent(node.id.clone()));
        if let Some(foreground_plan) = node.device.as_ref().and_then(|d| d.blur.as_ref()) {
            self.blur(node, foreground_plan, &mut foreground, depth, None, true)?;
        } else {
            self.contents(node, &mut foreground, depth + 1, None, true)?;
            if node.grayscale > 0. {
                self.grayscale(&mut foreground, node.grayscale)?;
            }
        }
        self.owner = previous_owner;
        let a = self.local_to_layer(coverage.world);
        if a.inverse().is_none() || transform(a).invert().is_none_or(|t| !t.is_finite()) {
            return Err(CompositeError::UnsupportedTransform);
        }
        for point in coverage.path.points() {
            let point = a.transform_point(Point::new(point.x, point.y));
            if !point.is_finite() || point.x.abs() > 1_000_000. || point.y.abs() > 1_000_000. {
                return Err(CompositeError::UnsupportedTransform);
            }
        }
        self.check()?;
        mask.fill_path(&coverage.path, coverage.rule, true, transform(a));
        let filtered = blur_pixels(sample, plan.sigma, || self.check())?;
        self.budget.write(self.pixels)?;
        replacement_pixels(
            target,
            &filtered,
            &foreground,
            &mask,
            clip,
            [ox as i64, oy as i64],
            node.opacity,
            || self.check(),
        )?;
        self.release(foreground);
        self.budget.in_use -= self.pixels + plan.bytes();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn background_prefix_crop_and_replacement_cancel_at_row_boundaries() {
        let mut source = Pixmap::new(8, 6).unwrap();
        source.fill(tiny_skia::Color::from_rgba8(200, 0, 0, 255));
        let immutable = source.data().to_vec();
        let mut copied = Pixmap::new(4, 4).unwrap();
        let mut rows = 0;
        assert_eq!(
            copy_window(&source, &mut copied, [2, 1], || {
                rows += 1;
                if rows == 3 {
                    Err(CompositeError::Superseded)
                } else {
                    Ok(())
                }
            }),
            Err(CompositeError::Superseded)
        );
        assert_eq!(source.data(), immutable);
        assert!(
            copied.data()[..32]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [200, 0, 0, 255])
        );
        assert!(copied.data()[32..].iter().all(|c| *c == 0));
        let mut target = Pixmap::new(4, 4).unwrap();
        target.fill(tiny_skia::Color::from_rgba8(200, 0, 0, 255));
        let original = target.data().to_vec();
        let mut filtered = Pixmap::new(4, 4).unwrap();
        filtered.fill(tiny_skia::Color::from_rgba8(0, 0, 200, 255));
        let foreground = Pixmap::new(4, 4).unwrap();
        let mask =
            Mask::from_vec(vec![255; 16], tiny_skia::IntSize::from_wh(4, 4).unwrap()).unwrap();
        let mut rows = 0;
        assert_eq!(
            replacement_pixels(
                &mut target,
                &filtered,
                &foreground,
                &mask,
                None,
                [0, 0],
                1.,
                || {
                    rows += 1;
                    if rows == 3 {
                        Err(CompositeError::Superseded)
                    } else {
                        Ok(())
                    }
                }
            ),
            Err(CompositeError::Superseded)
        );
        assert!(
            target.data()[..32]
                .as_chunks::<4>()
                .0
                .iter()
                .all(|p| *p == [0, 0, 200, 255])
        );
        assert_eq!(&target.data()[32..], &original[32..]);
    }

    #[test]
    fn replacement_is_independent_premultiplied_interpolation() {
        for b in [[32, 64, 16, 128], [0; 4], [255, 20, 3, 255]] {
            for g in [[16, 32, 64, 128], b] {
                for f in [[0; 4], [20, 2, 1, 30], [3, 4, 5, 255]] {
                    for m in [0, 37, 128, 255] {
                        for (qn, qd) in [(0u64, 1u64), (1, 4), (613, 1000), (1, 1)] {
                            let q = qn as f64 / qd as f64;
                            let mut actual = b;
                            replacement_pixel(&mut actual, &g, &f, m, q);
                            let mut expected = [0; 4];
                            for c in 0..4 {
                                // Independent exact rational oracle avoids cancellation
                                // around half-byte ties in a rearranged f64 expression.
                                let b1 = u64::from(b[c]) * (255 - u64::from(m))
                                    + u64::from(g[c]) * u64::from(m);
                                let r = u64::from(f[c]) * 65_025 + (255 - u64::from(f[3])) * b1;
                                let numerator = u64::from(b[c]) * 65_025 * (qd - qn) + qn * r;
                                let denominator = 65_025 * qd;
                                expected[c] = ((numerator + denominator / 2) / denominator) as u8;
                            }
                            for c in 0..3 {
                                expected[c] = expected[c].min(expected[3]);
                            }
                            assert_eq!(actual, expected, "B={b:?}, G={g:?}, F={f:?}, m={m}, q={q}");
                        }
                    }
                }
            }
        }
        let b = [32, 64, 16, 128];
        let mut actual = b;
        replacement_pixel(&mut actual, &b, &[0; 4], 255, 1.);
        assert_eq!(
            actual, b,
            "a captured translucent backdrop must not gain alpha"
        );
    }
}
