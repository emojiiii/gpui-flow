//! Canvas patterns capture pixels, while geometry, pens and texture matrices
//! retain the same transactional and bounded behavior as other Canvas draws.
use super::*;
use gpui_flow_scene::{
    Affine, Bounds, Color, FillRule, Point, Size, Stroke, StrokeCap, StrokeJoin,
    display::{Canvas, Path, PathCommand, Text},
};
use std::{cell::Cell, sync::Arc};

fn surface(smooth: bool, dpr: f32) -> CanvasSurface {
    let mut canvas = Canvas::new(32., 32.);
    canvas.smooth = smooth;
    canvas.pixel_ratio = dpr;
    CanvasSurface::new(canvas, Default::default()).unwrap()
}
fn bounds(x: f32, y: f32, width: f32, height: f32) -> Bounds {
    Bounds::new(Point::new(x, y), Size::new(width, height))
}
fn image(width: u32, height: u32, rgba: Vec<u8>) -> Arc<StaticImage> {
    Arc::new(StaticImage::from_premultiplied_rgba(width, height, rgba, Default::default()).unwrap())
}
fn inputs() -> ImageInputs {
    let mut inputs = ImageInputs::default();
    // A transparent fourth texel catches pad/clamp sampling and accidental
    // opaque rectangle fallbacks as well as source-alpha handling.
    inputs.insert(
        "tile".into(),
        Ok(image(
            2,
            2,
            vec![255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255, 0, 0, 0, 0],
        )),
    );
    inputs
}
fn solid_inputs(rgba: [u8; 4]) -> ImageInputs {
    let mut inputs = ImageInputs::default();
    inputs.insert("tile".into(), Ok(image(1, 1, rgba.to_vec())));
    inputs
}
fn pattern(surface: &CanvasSurface, repeat: CanvasPatternRepeat) -> CanvasPattern {
    surface
        .create_pattern(&"tile".into(), &inputs(), repeat)
        .unwrap()
}
fn solid_pattern(surface: &CanvasSurface, rgba: [u8; 4]) -> CanvasPattern {
    surface
        .create_pattern(
            &"tile".into(),
            &solid_inputs(rgba),
            CanvasPatternRepeat::Repeat,
        )
        .unwrap()
}
fn rect(x: f32, y: f32, width: f32, height: f32) -> Path {
    Path::new(vec![
        PathCommand::MoveTo {
            to: Point::new(x, y),
        },
        PathCommand::LineTo {
            to: Point::new(x + width, y),
        },
        PathCommand::LineTo {
            to: Point::new(x + width, y + height),
        },
        PathCommand::LineTo {
            to: Point::new(x, y + height),
        },
        PathCommand::Close,
    ])
}
fn append(path: &Path) -> Vec<CanvasCommand> {
    path.commands
        .iter()
        .map(|command| match *command {
            PathCommand::MoveTo { to } => CanvasCommand::MoveTo { to },
            PathCommand::LineTo { to } => CanvasCommand::LineTo { to },
            PathCommand::QuadraticTo { control, to } => {
                CanvasCommand::QuadraticCurveTo { control, to }
            }
            PathCommand::CubicTo {
                control1,
                control2,
                to,
            } => CanvasCommand::BezierCurveTo {
                control1,
                control2,
                to,
            },
            PathCommand::Close => CanvasCommand::ClosePath,
        })
        .collect()
}
fn fill(path: CanvasPathTarget, pattern: CanvasPattern) -> CanvasCommand {
    CanvasCommand::FillPattern {
        path,
        pattern,
        rule: FillRule::NonZero,
    }
}
fn full(pattern: CanvasPattern) -> CanvasCommand {
    fill(CanvasPathTarget::Explicit(rect(0., 0., 32., 32.)), pattern)
}
fn stroke(
    path: CanvasPathTarget,
    pattern: CanvasPattern,
    options: CanvasStrokeOptions,
) -> CanvasCommand {
    CanvasCommand::StrokePattern {
        path,
        pattern,
        options,
    }
}
fn pixel(surface: &CanvasSurface, x: usize, y: usize) -> [u8; 4] {
    let pixels = surface.snapshot();
    pixels.premultiplied_rgba()[(y * pixels.width() as usize + x) * 4..][..4]
        .try_into()
        .unwrap()
}
fn assert_pixels_equal(actual: &CanvasSurface, expected: &CanvasSurface) {
    assert_eq!(actual.snapshot().width(), expected.snapshot().width());
    assert_eq!(actual.snapshot().height(), expected.snapshot().height());
    assert_eq!(
        actual.snapshot().premultiplied_rgba(),
        expected.snapshot().premultiplied_rgba()
    );
}
fn replay_current(surface: &CanvasSurface) -> Vec<u8> {
    let mut copy = surface.clone();
    copy.clear().unwrap();
    copy.draw_commands(&[CanvasCommand::FillCurrentPath {
        paint: Color::WHITE.into(),
        rule: FillRule::NonZero,
    }])
    .unwrap();
    copy.snapshot().premultiplied_rgba().to_vec()
}
fn unchanged(actual: &CanvasSurface, before: &CanvasSurface) {
    assert_eq!(actual.generation(), before.generation());
    assert!(Arc::ptr_eq(&actual.snapshot(), &before.snapshot()));
    assert_eq!(
        actual.current_path_byte_len(),
        before.current_path_byte_len()
    );
    assert_eq!(replay_current(actual), replay_current(before));
}
fn ordinary_stroke(options: &CanvasStrokeOptions) -> Stroke {
    Stroke {
        width: options.width,
        cap: options.cap,
        join: options.join,
        miter_limit: options.miter_limit,
        dash_pattern: options.dash_pattern.clone(),
        dash_offset: options.dash_offset,
        ..Stroke::new(Color::WHITE, 1.)
    }
}

#[test]
fn canvas_pattern_repeat_target_draw_kind_and_dpr_matrix() {
    for repetition in [CanvasPatternRepeat::Repeat, CanvasPatternRepeat::NoRepeat] {
        for current in [false, true] {
            for is_stroke in [false, true] {
                for dpr in [1., 2.] {
                    let mut s = surface(false, dpr);
                    let mut tile = pattern(&s, repetition);
                    tile.set_transform(Affine::scale(4., 4.)).unwrap();
                    let path = rect(2., 2., 24., 24.);
                    let target = if current {
                        s.draw_commands(&append(&path)).unwrap();
                        CanvasPathTarget::Current
                    } else {
                        CanvasPathTarget::Explicit(path)
                    };
                    let command = if is_stroke {
                        stroke(
                            target,
                            tile,
                            CanvasStrokeOptions {
                                width: 4.,
                                cap: StrokeCap::Butt,
                                join: StrokeJoin::Bevel,
                                ..Default::default()
                            },
                        )
                    } else {
                        fill(target, tile)
                    };
                    assert!(s.draw_commands(&[command]).unwrap());
                    let d = dpr as usize;
                    assert_eq!(pixel(&s, 3 * d, 3 * d), [255, 0, 0, 255]);
                    assert_eq!(pixel(&s, 5 * d, 3 * d), [0, 255, 0, 255]);
                    assert_eq!(pixel(&s, 3 * d, 5 * d), [0, 0, 255, 255]);
                    assert_eq!(
                        pixel(&s, 11 * d, 3 * d),
                        if repetition == CanvasPatternRepeat::Repeat {
                            [255, 0, 0, 255]
                        } else {
                            [0; 4]
                        }
                    );
                    if !is_stroke {
                        assert_eq!(pixel(&s, 5 * d, 5 * d), [0; 4]);
                    }
                    assert_eq!(s.generation(), 2);
                }
            }
        }
    }
}

#[test]
fn canvas_pattern_transform_replaces_and_clone_and_command_keep_matrix_snapshots() {
    let mut s = surface(false, 1.);
    let mut tile = pattern(&s, CanvasPatternRepeat::Repeat);
    assert_eq!(tile.transform(), Affine::IDENTITY);
    assert_eq!(tile.repetition(), CanvasPatternRepeat::Repeat);
    tile.set_transform(Affine::translation(7., 9.)).unwrap();
    tile.set_transform(Affine::scale(4., 4.)).unwrap();
    assert_eq!(tile.transform(), Affine::scale(4., 4.));
    let copied = tile.clone();
    assert_eq!(copied, tile);
    let submitted = full(tile.clone());
    tile.set_transform(Affine::translation(4., 0.) * Affine::scale(4., 4.))
        .unwrap();
    assert_ne!(copied, tile);
    assert_eq!(copied.transform(), Affine::scale(4., 4.));
    s.draw_commands(&[submitted]).unwrap();
    assert_eq!(pixel(&s, 1, 1), [255, 0, 0, 255]);
    s.clear().unwrap();
    s.draw_commands(&[full(tile)]).unwrap();
    assert_eq!(pixel(&s, 1, 1), [0, 255, 0, 255]);
    assert!(!format!("{copied:?}").is_empty());
}

#[test]
fn canvas_pattern_invalid_matrix_is_rejected_without_replacing_the_previous_matrix() {
    let s = surface(false, 1.);
    let mut tile = pattern(&s, CanvasPatternRepeat::NoRepeat);
    let good = Affine::translation(3., 5.) * Affine::scale(4., 2.);
    tile.set_transform(good).unwrap();
    for field in 0..6 {
        for value in [
            f32::NAN,
            f32::INFINITY,
            f32::NEG_INFINITY,
            1_000_001.,
            -1_000_001.,
        ] {
            let mut coefficients = [1., 0., 0., 1., 0., 0.];
            coefficients[field] = value;
            let [a, b, c, d, tx, ty] = coefficients;
            assert_eq!(
                tile.set_transform(Affine::new(a, b, c, d, tx, ty)),
                Err(CompositeError::UnsupportedTransform)
            );
            assert_eq!(tile.transform(), good);
            assert_eq!(tile.repetition(), CanvasPatternRepeat::NoRepeat);
        }
    }
    tile.set_transform(Affine::translation(1_000_000., -1_000_000.))
        .unwrap();
    assert_eq!(tile.transform().tx, 1_000_000.);
}

#[test]
fn canvas_pattern_origin_is_not_reset_to_path_bounds_and_negative_coordinates_wrap() {
    for dpr in [1., 2.] {
        let mut s = surface(false, dpr);
        let mut tile = pattern(&s, CanvasPatternRepeat::Repeat);
        tile.set_transform(Affine::translation(12., 12.) * Affine::scale(4., 4.))
            .unwrap();
        s.draw_commands(&[fill(
            CanvasPathTarget::Explicit(rect(1., 1., 30., 30.)),
            tile,
        )])
        .unwrap();
        let d = dpr as usize;
        // Texture coordinates here are negative: (-2.625,-2.625),
        // (-1.625,-2.625), and (-2.625,-1.625).
        assert_eq!(pixel(&s, d, d), [0; 4]);
        assert_eq!(pixel(&s, 5 * d, d), [0, 0, 255, 255]);
        assert_eq!(pixel(&s, d, 5 * d), [0, 255, 0, 255]);
        assert_eq!(pixel(&s, 5 * d, 5 * d), [255, 0, 0, 255]);
        assert_eq!(pixel(&s, 13 * d, 13 * d), [255, 0, 0, 255]);
    }
}

fn wrapped_reference(
    source: &StaticImage,
    width: u32,
    height: u32,
    mapping: Affine,
    repetition: CanvasPatternRepeat,
    smooth: bool,
) -> tiny_skia::Pixmap {
    let mut expected = tiny_skia::Pixmap::new(width, height).unwrap();
    let paint = tiny_skia::Paint {
        shader: tiny_skia::Pattern::new(
            source.pixmap.as_ref(),
            tiny_skia::SpreadMode::Repeat,
            if smooth {
                tiny_skia::FilterQuality::Bilinear
            } else {
                tiny_skia::FilterQuality::Nearest
            },
            1.,
            crate::vector::transform(mapping),
        ),
        ..Default::default()
    };
    let path = if repetition == CanvasPatternRepeat::Repeat {
        tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(0., 0., width as f32, height as f32).unwrap(),
        )
    } else {
        tiny_skia::PathBuilder::from_rect(
            tiny_skia::Rect::from_xywh(0., 0., source.width() as f32, source.height() as f32)
                .unwrap(),
        )
        .transform(crate::vector::transform(mapping))
        .unwrap()
    };
    expected.fill_path(
        &path,
        &paint,
        tiny_skia::FillRule::Winding,
        tiny_skia::Transform::identity(),
        None,
    );
    expected
}

#[test]
fn canvas_pattern_mapping_is_dpr_times_draw_ctm_times_pattern_matrix() {
    let inputs = inputs();
    let source = inputs.get(&"tile".into()).unwrap();
    let matrices = [
        (
            Affine::translation(3., 5.),
            Affine::translation(2., 1.) * Affine::scale(4., 3.),
        ),
        (
            Affine::translation(24., 1.) * Affine::scale(-2., 1.),
            Affine::scale(3., 4.),
        ),
        (Affine::new(0., 1., -1., 0., 24., 4.), Affine::scale(4., 3.)),
        (Affine::new(1., 0., 0.5, 1., 2., 3.), Affine::scale(3., 4.)),
    ];
    for repeat in [CanvasPatternRepeat::Repeat, CanvasPatternRepeat::NoRepeat] {
        for smooth in [false, true] {
            for dpr in [1., 2.] {
                for (ctm, local) in matrices {
                    let mut s = surface(smooth, dpr);
                    let mut tile = s.create_pattern(&"tile".into(), &inputs, repeat).unwrap();
                    tile.set_transform(local).unwrap();
                    // Current geometry was frozen at identity, independently
                    // of the draw-time coordinate space used for sampling.
                    s.draw_commands(&append(&rect(0., 0., 32., 32.))).unwrap();
                    s.draw_commands(&[
                        CanvasCommand::SetTransform(ctm),
                        fill(CanvasPathTarget::Current, tile),
                    ])
                    .unwrap();
                    let mapping = Affine::scale(dpr, dpr) * ctm * local;
                    let expected = wrapped_reference(
                        &source,
                        (32. * dpr) as u32,
                        (32. * dpr) as u32,
                        mapping,
                        repeat,
                        smooth,
                    );
                    // Integer transforms compare exactly. At a slanted clip
                    // edge either pipeline may round coverage by one unit.
                    for (index, (actual, expected)) in s
                        .snapshot()
                        .premultiplied_rgba()
                        .iter()
                        .zip(expected.data())
                        .enumerate()
                    {
                        assert!(
                            actual.abs_diff(*expected) <= 2,
                            "repeat={repeat:?}, smooth={smooth}, dpr={dpr}, ctm={ctm:?}, byte={index}: {actual} != {expected}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn canvas_pattern_bilinear_repeat_seams_and_no_repeat_edges_use_wrapped_texels() {
    let mut inputs = ImageInputs::default();
    let source = image(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255]);
    inputs.insert("tile".into(), Ok(source.clone()));
    for repeat in [CanvasPatternRepeat::Repeat, CanvasPatternRepeat::NoRepeat] {
        for dpr in [1., 2.] {
            let mut s = surface(true, dpr);
            let mut tile = s.create_pattern(&"tile".into(), &inputs, repeat).unwrap();
            let local = Affine::translation(4., 4.) * Affine::scale(4., 8.);
            tile.set_transform(local).unwrap();
            s.draw_commands(&[full(tile)]).unwrap();
            let expected = wrapped_reference(
                &source,
                (32. * dpr) as u32,
                (32. * dpr) as u32,
                Affine::scale(dpr, dpr) * local,
                repeat,
                true,
            );
            assert_eq!(s.snapshot().premultiplied_rgba(), expected.data());
            let d = dpr as usize;
            let left = pixel(&s, 4 * d, 6 * d);
            let right = pixel(&s, 12 * d - 1, 6 * d);
            assert!(left[0] > 0 && left[1] > 0);
            assert!(right[0] > 0 && right[1] > 0);
            assert_eq!(left[3], 255);
            assert_eq!(right[3], 255);
            if repeat == CanvasPatternRepeat::NoRepeat {
                assert_eq!(pixel(&s, 4 * d - 1, 6 * d), [0; 4]);
                assert_eq!(pixel(&s, 12 * d, 6 * d), [0; 4]);
                assert_eq!(pixel(&s, 6 * d, 12 * d), [0; 4]);
            }
        }
    }
}

#[test]
fn canvas_pattern_transparent_texels_and_context_alpha_are_applied_once_per_draw() {
    for repeat in [CanvasPatternRepeat::Repeat, CanvasPatternRepeat::NoRepeat] {
        let mut s = surface(false, 1.);
        s.fill_rect(bounds(0., 0., 32., 32.), Color::WHITE).unwrap();
        let mut tile = s
            .create_pattern(&"tile".into(), &solid_inputs([128, 0, 0, 128]), repeat)
            .unwrap();
        tile.set_transform(Affine::scale(32., 32.)).unwrap();
        s.draw_commands(&[CanvasCommand::SetOpacity(0.5), full(tile.clone())])
            .unwrap();
        assert_eq!(pixel(&s, 8, 8), [255, 191, 191, 255]);
        s.draw_commands(&[CanvasCommand::SetOpacity(0.5), full(tile)])
            .unwrap();
        assert_eq!(pixel(&s, 8, 8), [255, 143, 143, 255]);
        let mut transparent = pattern(&s, repeat);
        transparent.set_transform(Affine::scale(8., 8.)).unwrap();
        let before = pixel(&s, 12, 12);
        s.draw_commands(&[full(transparent)]).unwrap();
        assert_eq!(pixel(&s, 12, 12), before);
    }
}

#[test]
fn canvas_pattern_current_geometry_is_frozen_but_pen_and_pattern_follow_draw_ctm() {
    for dpr in [1., 2.] {
        for repeat in [CanvasPatternRepeat::Repeat, CanvasPatternRepeat::NoRepeat] {
            for is_stroke in [false, true] {
                let mut actual = surface(false, dpr);
                let mut expected = surface(false, dpr);
                let frozen = Path::from_svg("M 4 8 L 26 8 L 26 20 L 4 20 Z").unwrap();
                actual
                    .draw_commands(&[
                        CanvasCommand::SetTransform(Affine::translation(2., 4.)),
                        CanvasCommand::MoveTo {
                            to: Point::new(2., 4.),
                        },
                        CanvasCommand::LineTo {
                            to: Point::new(24., 4.),
                        },
                        CanvasCommand::LineTo {
                            to: Point::new(24., 16.),
                        },
                        CanvasCommand::LineTo {
                            to: Point::new(2., 16.),
                        },
                        CanvasCommand::ClosePath,
                    ])
                    .unwrap();
                let ctm = Affine::translation(28., 2.) * Affine::scale(-2., 2.);
                let local = Path::from_svg("M 12 3 L 1 3 L 1 9 L 12 9 Z").unwrap();
                let mut tile = pattern(&actual, repeat);
                tile.set_transform(Affine::scale(8., 8.)).unwrap();
                let options = CanvasStrokeOptions {
                    width: 2.,
                    cap: StrokeCap::Butt,
                    join: StrokeJoin::Miter,
                    ..Default::default()
                };
                let actual_draw = if is_stroke {
                    stroke(CanvasPathTarget::Current, tile.clone(), options.clone())
                } else {
                    fill(CanvasPathTarget::Current, tile.clone())
                };
                let expected_draw = if is_stroke {
                    stroke(CanvasPathTarget::Explicit(local), tile, options)
                } else {
                    fill(CanvasPathTarget::Explicit(local), tile)
                };
                actual
                    .draw_commands(&[CanvasCommand::SetTransform(ctm), actual_draw])
                    .unwrap();
                expected
                    .draw_commands(&[CanvasCommand::SetTransform(ctm), expected_draw])
                    .unwrap();
                assert_pixels_equal(&actual, &expected);
                // Reusing the path after the patterned draw must retain its
                // original append-time coordinates, not the inverse-CTM copy.
                let mut geometry = surface(false, dpr);
                geometry
                    .draw_commands(&[CanvasCommand::FillPath {
                        path: frozen,
                        paint: Color::WHITE.into(),
                        rule: FillRule::NonZero,
                    }])
                    .unwrap();
                assert_eq!(
                    replay_current(&actual),
                    geometry.snapshot().premultiplied_rgba()
                );
            }
        }
    }
}

#[test]
fn canvas_pattern_stroke_options_default_and_cap_join_dash_geometry_match_real_strokes() {
    let options = CanvasStrokeOptions::default();
    let default = Stroke::default();
    assert_eq!(options.width, default.width);
    assert_eq!(options.cap, default.cap);
    assert_eq!(options.join, default.join);
    assert_eq!(options.miter_limit, default.miter_limit);
    assert_eq!(options.dash_pattern, default.dash_pattern);
    assert_eq!(options.dash_offset, default.dash_offset);
    let path = Path::from_svg("M 5 22 L 14 6 L 24 22 M 5 27 Q 14 18 26 27").unwrap();
    for cap in [StrokeCap::Butt, StrokeCap::Square, StrokeCap::Round] {
        for join in [StrokeJoin::Bevel, StrokeJoin::Miter, StrokeJoin::Round] {
            for dash in [vec![], vec![4., 3.], vec![3., 2., 1.]] {
                for current in [false, true] {
                    let mut s = surface(false, 2.);
                    let mut expected = surface(false, 2.);
                    let options = CanvasStrokeOptions {
                        width: 3.,
                        cap,
                        join,
                        miter_limit: 2.,
                        dash_pattern: dash.clone(),
                        dash_offset: -2.,
                    };
                    let target = if current {
                        s.draw_commands(&append(&path)).unwrap();
                        CanvasPathTarget::Current
                    } else {
                        CanvasPathTarget::Explicit(path.clone())
                    };
                    let tile = solid_pattern(&s, [255; 4]);
                    s.draw_commands(&[stroke(target, tile, options.clone())])
                        .unwrap();
                    expected
                        .draw_commands(&[CanvasCommand::StrokePath {
                            path: path.clone(),
                            stroke: ordinary_stroke(&options),
                        }])
                        .unwrap();
                    assert_pixels_equal(&s, &expected);
                    assert_eq!(pixel(&s, 28, 28), [0; 4]);
                    assert!(s.snapshot().premultiplied_rgba().iter().any(|v| *v != 0));
                }
            }
        }
    }
}

#[test]
fn canvas_pattern_evenodd_curves_nested_frozen_clips_and_save_restore() {
    let path = Path::from_svg(
        "M 2 16 Q 2 2 16 2 C 28 2 30 12 30 16 L 30 30 L 2 30 Z M 10 10 L 22 10 L 22 22 L 10 22 Z",
    )
    .unwrap();
    for current in [false, true] {
        for rule in [FillRule::NonZero, FillRule::EvenOdd] {
            for dpr in [1., 2.] {
                let mut actual = surface(false, dpr);
                let mut expected = surface(false, dpr);
                let mut prefix = vec![
                    CanvasCommand::Save,
                    CanvasCommand::SetTransform(Affine::translation(2., 0.)),
                    CanvasCommand::ClipPath {
                        path: rect(0., 0., 24., 32.),
                        rule: FillRule::NonZero,
                    },
                    CanvasCommand::SetTransform(Affine::IDENTITY),
                    CanvasCommand::BeginPath,
                ];
                prefix.extend(append(&rect(0., 4., 32., 22.)));
                prefix.push(CanvasCommand::ClipCurrentPath {
                    rule: FillRule::NonZero,
                });
                prefix.push(CanvasCommand::BeginPath);
                if current {
                    prefix.extend(append(&path));
                }
                prefix.push(CanvasCommand::SetOpacity(0.5));
                let tile = solid_pattern(&actual, [255; 4]);
                let mut commands = prefix.clone();
                commands.push(CanvasCommand::FillPattern {
                    path: if current {
                        CanvasPathTarget::Current
                    } else {
                        CanvasPathTarget::Explicit(path.clone())
                    },
                    pattern: tile.clone(),
                    rule,
                });
                commands.push(CanvasCommand::Restore);
                commands.push(fill(
                    CanvasPathTarget::Explicit(rect(27., 1., 4., 4.)),
                    tile,
                ));
                actual.draw_commands(&commands).unwrap();
                let mut reference = prefix;
                reference.push(if current {
                    CanvasCommand::FillCurrentPath {
                        paint: Color::WHITE.into(),
                        rule,
                    }
                } else {
                    CanvasCommand::FillPath {
                        path: path.clone(),
                        paint: Color::WHITE.into(),
                        rule,
                    }
                });
                reference.push(CanvasCommand::Restore);
                reference.push(CanvasCommand::FillPath {
                    path: rect(27., 1., 4., 4.),
                    paint: Color::WHITE.into(),
                    rule: FillRule::NonZero,
                });
                expected.draw_commands(&reference).unwrap();
                assert_pixels_equal(&actual, &expected);
                let d = dpr as usize;
                assert_eq!(pixel(&actual, 28 * d, 2 * d), [255; 4]);
                assert_eq!(
                    pixel(&actual, 15 * d, 15 * d),
                    if rule == FillRule::EvenOdd {
                        [0; 4]
                    } else {
                        [128; 4]
                    }
                );
                assert_eq!(pixel(&actual, 28 * d, 15 * d), [0; 4]);
            }
        }
    }
}

#[test]
fn canvas_pattern_capture_survives_key_replacement_input_drop_and_cross_surface_use() {
    let source_surface = surface(false, 1.);
    let mut inputs = solid_inputs([255, 0, 0, 255]);
    let source = inputs.get(&"tile".into()).unwrap();
    let weak = Arc::downgrade(&source);
    let tile = source_surface
        .create_pattern(&"tile".into(), &inputs, CanvasPatternRepeat::Repeat)
        .unwrap();
    assert!(Arc::strong_count(&source) >= 3);
    let command = full(tile.clone());
    inputs.insert("tile".into(), Ok(image(1, 1, vec![0, 255, 0, 255])));
    let replacement = source_surface
        .create_pattern(&"tile".into(), &inputs, CanvasPatternRepeat::Repeat)
        .unwrap();
    drop(inputs);
    drop(source);
    drop(source_surface);
    drop(tile);
    assert!(weak.upgrade().is_some());
    let mut destination = surface(false, 2.);
    destination
        .draw_commands(std::slice::from_ref(&command))
        .unwrap();
    assert_eq!(pixel(&destination, 20, 20), [255, 0, 0, 255]);
    drop(command);
    // A destination stores only rendered pixels, never a submitted command.
    assert!(weak.upgrade().is_none());
    destination.draw_commands(&[full(replacement)]).unwrap();
    assert_eq!(pixel(&destination, 20, 20), [0, 255, 0, 255]);
}

#[test]
fn canvas_pattern_command_clones_share_one_pixel_allocation_until_last_owner_drops() {
    let s = surface(false, 1.);
    let inputs = inputs();
    let source = inputs.get(&"tile".into()).unwrap();
    let weak = Arc::downgrade(&source);
    let tile = s
        .create_pattern(&"tile".into(), &inputs, CanvasPatternRepeat::Repeat)
        .unwrap();
    let count = Arc::strong_count(&source);
    let copy = tile.clone();
    assert_eq!(Arc::strong_count(&source), count + 1);
    let command = full(copy);
    let copied_command = command.clone();
    assert_eq!(Arc::strong_count(&source), count + 2);
    drop(source);
    drop(inputs);
    drop(tile);
    drop(command);
    assert!(weak.upgrade().is_some());
    drop(copied_command);
    assert!(weak.upgrade().is_none());
}

#[test]
fn canvas_pattern_can_capture_own_surface_without_read_write_feedback() {
    let mut s = surface(false, 1.);
    s.fill_rect(bounds(0., 0., 4., 4.), Color::hex(0xff0000))
        .unwrap();
    let before = s.snapshot();
    let mut inputs = ImageInputs::default();
    inputs.insert("tile".into(), Ok(before.clone()));
    let mut tile = s
        .create_pattern(&"tile".into(), &inputs, CanvasPatternRepeat::Repeat)
        .unwrap();
    tile.set_transform(Affine::translation(4., 4.)).unwrap();
    drop(inputs);
    s.draw_commands(&[full(tile.clone())]).unwrap();
    assert_eq!(pixel(&s, 5, 5), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 10, 10), [0; 4]);
    assert_eq!(
        &before.premultiplied_rgba()[(5 * 32 + 5) * 4..][..4],
        &[0; 4]
    );
    s.clear().unwrap();
    s.draw_commands(&[full(tile)]).unwrap();
    assert_eq!(pixel(&s, 1, 1), [0; 4]);
    assert_eq!(pixel(&s, 5, 5), [255, 0, 0, 255]);
}

#[test]
fn canvas_pattern_factory_missing_failed_keys_and_key_length_boundary() {
    let s = surface(false, 1.);
    let before = s.snapshot();
    let generation = s.generation();
    let mut inputs = inputs();
    inputs.insert("failed".into(), Err(CompositeError::InvalidImage));
    assert_eq!(
        s.create_pattern(&"missing".into(), &inputs, CanvasPatternRepeat::Repeat),
        Err(CompositeError::ImagePending("missing".into()))
    );
    assert_eq!(
        s.create_pattern(&"failed".into(), &inputs, CanvasPatternRepeat::Repeat),
        Err(CompositeError::InvalidImage)
    );
    let shared = inputs.get(&"tile".into()).unwrap();
    inputs.insert("x".repeat(512).into(), Ok(shared.clone()));
    inputs.insert("x".repeat(513).into(), Ok(shared));
    assert!(
        s.create_pattern(
            &"x".repeat(512).into(),
            &inputs,
            CanvasPatternRepeat::Repeat
        )
        .is_ok()
    );
    assert_eq!(
        s.create_pattern(
            &"x".repeat(513).into(),
            &inputs,
            CanvasPatternRepeat::Repeat
        ),
        Err(CompositeError::InputLimit)
    );
    assert_eq!(s.generation(), generation);
    assert!(Arc::ptr_eq(&before, &s.snapshot()));
}

#[test]
fn canvas_pattern_factory_honors_reduced_source_dimensions_pixels_and_bytes() {
    let mut inputs = ImageInputs::default();
    inputs.insert("tile".into(), Ok(image(8, 8, vec![255; 8 * 8 * 4])));
    for limits in [
        CompositeLimits {
            max_dimension: 4,
            ..Default::default()
        },
        CompositeLimits {
            max_pixels: 63,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_bytes: 255,
            ..Default::default()
        },
    ] {
        let s = CanvasSurface::new(Canvas::new(1., 1.), limits).unwrap();
        let before = s.snapshot();
        assert!(
            s.create_pattern(&"tile".into(), &inputs, CanvasPatternRepeat::Repeat)
                .is_err(),
            "{limits:?}"
        );
        assert!(Arc::ptr_eq(&before, &s.snapshot()));
        assert_eq!(s.generation(), 1);
    }
}

#[test]
fn canvas_pattern_singular_local_matrix_draws_nothing_but_ctm_rules_stay_strict() {
    for repeat in [CanvasPatternRepeat::Repeat, CanvasPatternRepeat::NoRepeat] {
        for singular in [
            Affine::scale(0., 1.),
            Affine::new(1., 2., 2., 4., 3., 5.),
            Affine::scale(0., 0.),
        ] {
            for smooth in [false, true] {
                for dpr in [1., 2.] {
                    let mut s = surface(smooth, dpr);
                    s.fill_rect(bounds(0., 0., 32., 32.), Color::WHITE).unwrap();
                    s.draw_commands(&append(&rect(2., 2., 10., 10.))).unwrap();
                    let mut tile = pattern(&s, repeat);
                    tile.set_transform(singular).unwrap();
                    assert_eq!(tile.transform(), singular);
                    let before_pixels = s.snapshot().premultiplied_rgba().to_vec();
                    for current in [false, true] {
                        for is_stroke in [false, true] {
                            let target = if current {
                                CanvasPathTarget::Current
                            } else {
                                CanvasPathTarget::Explicit(rect(2., 2., 10., 10.))
                            };
                            let command = if is_stroke {
                                stroke(target, tile.clone(), Default::default())
                            } else {
                                fill(target, tile.clone())
                            };
                            s.draw_commands(&[command]).unwrap();
                            assert_eq!(s.snapshot().premultiplied_rgba(), before_pixels);
                        }
                    }
                    let before = s.clone();
                    assert_eq!(
                        s.draw_commands(&[CanvasCommand::SetTransform(singular), full(tile)]),
                        Err(CompositeError::UnsupportedTransform)
                    );
                    unchanged(&s, &before);
                }
            }
        }
    }
}

#[test]
fn canvas_pattern_tiny_two_axis_scale_rejects_atomically_instead_of_painting_transparent() {
    let transform = Affine::scale(1e-30, 1e-30);
    // This is genuinely nonsingular, but its finite inverse exceeds the
    // rasterizer's bounded source-sampling domain with either filter.
    assert!(transform.determinant() > 0.);
    for smooth in [false, true] {
        for dpr in [1., 2.] {
            for current in [false, true] {
                for is_stroke in [false, true] {
                    let mut s = surface(smooth, dpr);
                    s.fill_rect(bounds(0., 0., 32., 32.), Color::WHITE).unwrap();
                    s.draw_commands(&append(&rect(2., 2., 10., 10.))).unwrap();
                    let mut tile = solid_pattern(&s, [37, 149, 223, 255]);
                    tile.set_transform(transform).unwrap();
                    let before = s.clone();
                    let target = if current {
                        CanvasPathTarget::Current
                    } else {
                        CanvasPathTarget::Explicit(rect(4., 4., 24., 24.))
                    };
                    let command = if is_stroke {
                        stroke(target, tile, Default::default())
                    } else {
                        fill(target, tile)
                    };
                    let mut commands = vec![CanvasCommand::BeginPath];
                    commands.extend(append(&rect(4., 4., 24., 24.)));
                    commands.push(command);
                    assert_eq!(
                        s.draw_commands(&commands),
                        Err(CompositeError::UnsupportedTransform),
                        "smooth={smooth}, dpr={dpr}, current={current}, stroke={is_stroke}"
                    );
                    unchanged(&s, &before);
                }
            }
        }
    }
}

#[test]
fn canvas_pattern_source_sampling_domain_accepts_boundary_and_rejects_next_scale() {
    for smooth in [false, true] {
        for dpr in [1., 2.] {
            let mut s = surface(smooth, dpr);
            // The raster guard includes 16 device pixels beyond the target.
            let scale = (32. + 16. / dpr) / 16_777_216.;
            let rgba = [37, 149, 223, 255];
            let mut tile = solid_pattern(&s, rgba);
            tile.set_transform(Affine::scale(scale, scale)).unwrap();
            s.draw_commands(&[full(tile.clone())]).unwrap();
            assert!(
                s.snapshot()
                    .premultiplied_rgba()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .all(|p| *p == rgba)
            );
            let before = s.clone();
            let smaller = f32::from_bits(scale.to_bits() - 1);
            tile.set_transform(Affine::scale(smaller, smaller)).unwrap();
            assert_eq!(
                s.draw_commands(&[CanvasCommand::BeginPath, full(tile)]),
                Err(CompositeError::UnsupportedTransform),
                "smooth={smooth}, dpr={dpr}"
            );
            unchanged(&s, &before);
        }
    }
}

#[test]
fn canvas_pattern_extreme_composition_and_tiny_inverse_are_atomic() {
    let mut s = surface(false, 2.);
    s.draw_commands(&append(&rect(2., 2., 10., 10.))).unwrap();
    let before = s.clone();
    for (ctm, local) in [
        (Affine::scale(1_000_000., 1.), Affine::scale(1_000_000., 1.)),
        (
            Affine::translation(1_000_000., 0.),
            Affine::translation(1_000_000., 0.),
        ),
        (Affine::IDENTITY, Affine::scale(f32::MIN_POSITIVE, 1.)),
        (Affine::IDENTITY, Affine::scale(f32::from_bits(1), 1.)),
    ] {
        let mut tile = pattern(&s, CanvasPatternRepeat::Repeat);
        tile.set_transform(local).unwrap();
        assert!(
            s.draw_commands(&[
                CanvasCommand::BeginPath,
                CanvasCommand::SetTransform(ctm),
                full(tile),
            ])
            .is_err(),
            "ctm={ctm:?}, pattern={local:?}"
        );
        unchanged(&s, &before);
    }
}

#[test]
fn canvas_pattern_late_invalid_image_or_font_error_rolls_back_pixels_and_current_path() {
    let mut s = surface(false, 1.);
    s.draw_commands(&append(&rect(2., 2., 10., 10.))).unwrap();
    let tile = pattern(&s, CanvasPatternRepeat::Repeat);
    s.draw_commands(&[full(tile.clone())]).unwrap();
    let before = s.clone();
    let invalid_path = Path::new(vec![PathCommand::MoveTo {
        to: Point::new(f32::NAN, 1.),
    }]);
    let bad_stroke = CanvasStrokeOptions {
        width: f32::NAN,
        ..Default::default()
    };
    let late_commands = [
        CanvasCommand::Restore,
        CanvasCommand::SetOpacity(f32::NAN),
        fill(CanvasPathTarget::Explicit(invalid_path), tile.clone()),
        stroke(CanvasPathTarget::Current, tile.clone(), bad_stroke),
        CanvasCommand::DrawImage {
            image: "missing".into(),
            source: None,
            destination: bounds(0., 0., 4., 4.),
        },
    ];
    for late in late_commands {
        let called = Cell::new(false);
        assert!(
            s.draw_commands_with_text(
                &[
                    CanvasCommand::BeginPath,
                    CanvasCommand::MoveTo {
                        to: Point::new(20., 20.)
                    },
                    full(tile.clone()),
                    late,
                ],
                &ImageInputs::default(),
                |_| {
                    called.set(true);
                    Ok(Default::default())
                }
            )
            .is_err()
        );
        assert!(!called.get());
        unchanged(&s, &before);
    }
    let mut failed = ImageInputs::default();
    failed.insert("failed".into(), Err(CompositeError::InvalidImage));
    assert_eq!(
        s.draw_commands_with_images(
            &[
                CanvasCommand::BeginPath,
                full(tile.clone()),
                CanvasCommand::DrawImage {
                    image: "failed".into(),
                    source: None,
                    destination: bounds(0., 0., 4., 4.)
                },
            ],
            &failed
        ),
        Err(CompositeError::InvalidImage)
    );
    unchanged(&s, &before);
    let text = CanvasCommand::FillTextBox {
        text: Text::new("host font required", Size::new(20., 8.)),
        position: Point::ZERO,
        color: Color::WHITE,
    };
    assert!(matches!(
        s.draw_commands(&[CanvasCommand::BeginPath, full(tile.clone()), text.clone()]),
        Err(CompositeError::TextBackendRequired(_))
    ));
    unchanged(&s, &before);
    assert_eq!(
        s.draw_commands_with_text(
            &[CanvasCommand::BeginPath, full(tile), text],
            &ImageInputs::default(),
            |_| Err(CompositeError::Superseded)
        ),
        Err(CompositeError::Superseded)
    );
    unchanged(&s, &before);
}

#[test]
fn canvas_pattern_invalid_stroke_options_reject_even_a_singular_or_empty_draw() {
    let mut s = surface(false, 1.);
    let mut tile = pattern(&s, CanvasPatternRepeat::Repeat);
    tile.set_transform(Affine::scale(0., 1.)).unwrap();
    let before = s.clone();
    for options in [
        CanvasStrokeOptions {
            width: -1.,
            ..Default::default()
        },
        CanvasStrokeOptions {
            width: f32::INFINITY,
            ..Default::default()
        },
        CanvasStrokeOptions {
            miter_limit: 0.,
            ..Default::default()
        },
        CanvasStrokeOptions {
            miter_limit: f32::NAN,
            ..Default::default()
        },
        CanvasStrokeOptions {
            dash_pattern: vec![1., 0.],
            ..Default::default()
        },
        CanvasStrokeOptions {
            dash_pattern: vec![1., -1.],
            ..Default::default()
        },
        CanvasStrokeOptions {
            dash_pattern: vec![f32::NAN],
            ..Default::default()
        },
        CanvasStrokeOptions {
            dash_pattern: vec![1.; gpui_flow_scene::MAX_STROKE_DASH_ENTRIES + 1],
            ..Default::default()
        },
        CanvasStrokeOptions {
            dash_offset: f32::INFINITY,
            ..Default::default()
        },
    ] {
        assert!(
            s.draw_commands(&[stroke(CanvasPathTarget::Current, tile.clone(), options)])
                .is_err()
        );
        unchanged(&s, &before);
    }
}

#[test]
fn canvas_pattern_reduced_geometry_and_working_budgets_reject_before_publish() {
    let factory = surface(false, 1.);
    let tile = solid_pattern(&factory, [255; 4]);
    for limits in [
        CompositeLimits {
            max_points: 4,
            ..Default::default()
        },
        CompositeLimits {
            max_total_points: 4,
            ..Default::default()
        },
        CompositeLimits {
            max_path_bytes: 4 * size_of::<PathCommand>() as u64,
            ..Default::default()
        },
        CompositeLimits {
            max_nodes: 1,
            ..Default::default()
        },
        CompositeLimits {
            max_depth: 0,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_bytes: 2047,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_writes: 256,
            ..Default::default()
        },
    ] {
        let mut s = CanvasSurface::new(Canvas::new(16., 16.), limits).unwrap();
        let before = s.snapshot();
        assert!(
            s.draw_commands(&[fill(
                CanvasPathTarget::Explicit(rect(1., 1., 10., 10.)),
                tile.clone()
            )])
            .is_err(),
            "{limits:?}"
        );
        assert_eq!(s.generation(), 1);
        assert!(Arc::ptr_eq(&before, &s.snapshot()));
        assert_eq!(s.current_path_byte_len(), 0);
    }
}

#[test]
fn canvas_pattern_nested_clips_charge_masks_depth_nodes_and_path_snapshots() {
    let factory = surface(false, 1.);
    let tile = solid_pattern(&factory, [255; 4]);
    for limits in [
        CompositeLimits {
            max_mask_runs: 0,
            ..Default::default()
        },
        CompositeLimits {
            max_mask_runs: 1,
            ..Default::default()
        },
        CompositeLimits {
            max_nodes: 3,
            ..Default::default()
        },
        CompositeLimits {
            max_depth: 1,
            ..Default::default()
        },
        CompositeLimits {
            max_total_points: 14,
            ..Default::default()
        },
        CompositeLimits {
            max_path_bytes: 14 * size_of::<PathCommand>() as u64,
            ..Default::default()
        },
    ] {
        let mut s = CanvasSurface::new(Canvas::new(16., 16.), limits).unwrap();
        let before = s.snapshot();
        let commands = [
            CanvasCommand::ClipPath {
                path: rect(0., 0., 15., 15.),
                rule: FillRule::NonZero,
            },
            CanvasCommand::ClipPath {
                path: rect(1., 1., 12., 12.),
                rule: FillRule::NonZero,
            },
            fill(
                CanvasPathTarget::Explicit(rect(2., 2., 8., 8.)),
                tile.clone(),
            ),
        ];
        assert!(s.draw_commands(&commands).is_err(), "{limits:?}");
        assert!(Arc::ptr_eq(&before, &s.snapshot()));
        assert_eq!(s.generation(), 1);
    }
}

#[test]
fn canvas_pattern_cross_surface_use_rechecks_source_limits_and_charges_retained_pixels() {
    let factory = surface(false, 1.);
    let mut inputs = ImageInputs::default();
    inputs.insert("tile".into(), Ok(image(32, 32, vec![255; 32 * 32 * 4])));
    let tile = factory
        .create_pattern(&"tile".into(), &inputs, CanvasPatternRepeat::Repeat)
        .unwrap();
    for limits in [
        CompositeLimits {
            max_dimension: 16,
            ..Default::default()
        },
        CompositeLimits {
            max_pixels: 256,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_bytes: 4095,
            ..Default::default()
        },
        // The source fits by itself, but source plus old/new destination does not.
        CompositeLimits {
            max_surface_bytes: 5000,
            ..Default::default()
        },
    ] {
        let mut s = CanvasSurface::new(Canvas::new(16., 16.), limits).unwrap();
        let before = s.snapshot();
        assert!(
            s.draw_commands(&[fill(
                CanvasPathTarget::Explicit(rect(0., 0., 16., 16.)),
                tile.clone()
            )])
            .is_err(),
            "{limits:?}"
        );
        assert!(Arc::ptr_eq(&before, &s.snapshot()));
    }
}

#[test]
fn canvas_pattern_current_path_scoped_budget_and_font_work_fail_atomically() {
    let mut s = surface(false, 1.);
    s.draw_commands(&append(&rect(2., 2., 10., 10.))).unwrap();
    let tile = pattern(&s, CanvasPatternRepeat::Repeat);
    let before = s.clone();
    assert!(
        s.with_current_path_budget(s.current_path_byte_len(), |s| {
            s.draw_commands(&[fill(CanvasPathTarget::Current, tile.clone())])
        })
        .is_err()
    );
    unchanged(&s, &before);
    assert_eq!(
        s.draw_commands_with_text(
            &[CanvasCommand::BeginPath, full(tile)],
            &ImageInputs::default(),
            |request| {
                Ok(CanvasTextOutput {
                    inputs: Default::default(),
                    pixel_work: request.limits.max_surface_writes + 1,
                })
            }
        ),
        Err(CompositeError::WorkLimit)
    );
    unchanged(&s, &before);
}

#[test]
fn canvas_pattern_tiny_tile_uses_bounded_shader_work_instead_of_expanding_tiles() {
    let limits = CompositeLimits {
        max_nodes: 16,
        max_points: 64,
        max_total_points: 128,
        max_path_bytes: 4096,
        max_surface_writes: 2_000_000,
        ..Default::default()
    };
    let mut canvas = Canvas::new(256., 256.);
    canvas.smooth = false;
    let mut s = CanvasSurface::new(canvas, limits).unwrap();
    let mut tile = solid_pattern(&s, [255, 0, 0, 255]);
    // Over a trillion mathematical repetitions cover the viewport. Admission
    // and rendering must scale with viewport/geometry, not repetition count.
    tile.set_transform(Affine::scale(0.0001, 0.0001)).unwrap();
    s.draw_commands(&[fill(
        CanvasPathTarget::Explicit(rect(0., 0., 256., 256.)),
        tile,
    )])
    .unwrap();
    assert_eq!(pixel(&s, 0, 0), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 127, 133), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 255, 255), [255, 0, 0, 255]);
}

#[test]
fn canvas_pattern_far_offscreen_no_repeat_is_bounded_and_transparent() {
    let mut s = surface(true, 1.);
    let mut tile = pattern(&s, CanvasPatternRepeat::NoRepeat);
    tile.set_transform(Affine::translation(900_000., -900_000.))
        .unwrap();
    s.draw_commands(&[full(tile)]).unwrap();
    assert!(s.snapshot().premultiplied_rgba().iter().all(|v| *v == 0));
    let mut tile = pattern(&s, CanvasPatternRepeat::Repeat);
    tile.set_transform(Affine::translation(900_000., -900_000.))
        .unwrap();
    s.draw_commands(&[full(tile)]).unwrap();
    assert!(s.snapshot().premultiplied_rgba().iter().any(|v| *v != 0));
}

#[test]
fn canvas_pattern_negative_repeat_bilinear_seams_have_independently_calculated_texels() {
    let mut inputs = ImageInputs::default();
    inputs.insert(
        "tile".into(),
        Ok(image(2, 1, vec![255, 0, 0, 255, 0, 255, 0, 255])),
    );
    for dpr in [1., 2.] {
        let mut s = surface(true, dpr);
        let mut tile = s
            .create_pattern(&"tile".into(), &inputs, CanvasPatternRepeat::Repeat)
            .unwrap();
        tile.set_transform(Affine::translation(12.25, 0.) * Affine::scale(4., 4.))
            .unwrap();
        s.draw_commands(&[full(tile)]).unwrap();
        for x in 0..(32. * dpr) as usize {
            // Bilinear texel centers are n+0.5. Use Euclidean remainder,
            // including negative n, instead of relying on either renderer.
            let source_x = ((x as f32 + 0.5) / dpr - 12.25) / 4. - 0.5;
            let left = source_x.floor() as i32;
            let weight = source_x - source_x.floor();
            let left_red = if left.rem_euclid(2) == 0 { 255. } else { 0. };
            let right_red = 255. - left_red;
            let red = (left_red * (1. - weight) + right_red * weight).round() as u8;
            let actual = pixel(&s, x, (6. * dpr) as usize);
            assert!(
                actual[0].abs_diff(red) <= 1,
                "dpr={dpr}, x={x}, actual={actual:?}, expected red={red}"
            );
            assert!(actual[1].abs_diff(255 - red) <= 1);
            assert_eq!(actual[2], 0);
            assert_eq!(actual[3], 255);
        }
    }
}

#[test]
fn canvas_pattern_fractional_no_repeat_boundary_has_calculated_coverage_and_alpha() {
    for dpr in [1., 2.] {
        let mut s = surface(true, dpr);
        let mut tile = s
            .create_pattern(
                &"tile".into(),
                &solid_inputs([128, 0, 0, 128]),
                CanvasPatternRepeat::NoRepeat,
            )
            .unwrap();
        tile.set_transform(Affine::translation(4.25, 4.25) * Affine::scale(4.5, 4.5))
            .unwrap();
        s.draw_commands(&[CanvasCommand::SetOpacity(0.5), full(tile)])
            .unwrap();
        let lo = 4.25 * dpr;
        let hi = 8.75 * dpr;
        for y in 0..(16. * dpr) as usize {
            for x in 0..(16. * dpr) as usize {
                // Exact pixel-square intersection with the transformed source
                // extent. Source alpha 128 and context alpha 1/2 give 64,
                // before multiplying the two independent edge coverages.
                let coverage_x = ((x as f32 + 1.).min(hi) - (x as f32).max(lo)).clamp(0., 1.);
                let coverage_y = ((y as f32 + 1.).min(hi) - (y as f32).max(lo)).clamp(0., 1.);
                let expected = (64. * coverage_x * coverage_y).round() as u8;
                let actual = pixel(&s, x, y);
                assert!(
                    actual[0].abs_diff(expected) <= 1,
                    "dpr={dpr}, ({x},{y}): {actual:?}, expected alpha={expected}"
                );
                assert_eq!(actual[1], 0);
                assert_eq!(actual[2], 0);
                assert!(actual[3].abs_diff(expected) <= 1);
            }
        }
    }
}

#[test]
fn canvas_pattern_and_image_caller_keys_cannot_collide_with_private_command_ids() {
    for pattern_first in [false, true] {
        for caller_key in [
            "canvas-pattern-1",
            "canvas-pattern-2",
            "canvas-image-1",
            "canvas-image-2",
        ] {
            let mut s = surface(false, 1.);
            let mut inputs = solid_inputs([255, 0, 0, 255]);
            let tile = s
                .create_pattern(&"tile".into(), &inputs, CanvasPatternRepeat::Repeat)
                .unwrap();
            inputs.insert(caller_key.into(), Ok(image(1, 1, vec![0, 0, 255, 255])));
            let patterned = fill(CanvasPathTarget::Explicit(rect(0., 0., 8., 8.)), tile);
            let image = CanvasCommand::DrawImage {
                image: caller_key.into(),
                source: None,
                destination: bounds(12., 0., 8., 8.),
            };
            let commands = if pattern_first {
                vec![patterned, image]
            } else {
                vec![image, patterned]
            };
            s.draw_commands_with_images(&commands, &inputs).unwrap();
            assert_eq!(
                pixel(&s, 3, 3),
                [255, 0, 0, 255],
                "pattern_first={pattern_first}, caller_key={caller_key}"
            );
            assert_eq!(
                pixel(&s, 15, 3),
                [0, 0, 255, 255],
                "pattern_first={pattern_first}, caller_key={caller_key}"
            );
        }
    }
}

#[test]
fn canvas_pattern_and_image_shared_source_is_charged_once_and_unused_errors_are_ignored() {
    let mut s = surface(false, 1.);
    let mut inputs = inputs();
    let shared = inputs.get(&"tile".into()).unwrap();
    inputs.insert("alias".into(), Ok(shared));
    inputs.insert("unused".into(), Err(CompositeError::InvalidImage));
    let one = s
        .create_pattern(&"tile".into(), &inputs, CanvasPatternRepeat::Repeat)
        .unwrap();
    let two = s
        .create_pattern(&"alias".into(), &inputs, CanvasPatternRepeat::NoRepeat)
        .unwrap();
    let retained = s.byte_len() * 2 + 16;
    let called = Cell::new(false);
    s.draw_commands_with_text(
        &[
            full(one),
            full(two),
            CanvasCommand::DrawImage {
                image: "alias".into(),
                source: None,
                destination: bounds(16., 16., 8., 8.),
            },
        ],
        &inputs,
        |request| {
            called.set(true);
            assert_eq!(request.retained_bytes, retained);
            Ok(Default::default())
        },
    )
    .unwrap();
    assert!(called.get());
    assert_eq!(pixel(&s, 17, 17), [255, 0, 0, 255]);
}

#[test]
fn canvas_pattern_no_repeat_source_extent_itself_consumes_clip_budgets() {
    let factory = surface(false, 1.);
    let mut tile = factory
        .create_pattern(
            &"tile".into(),
            &solid_inputs([255; 4]),
            CanvasPatternRepeat::NoRepeat,
        )
        .unwrap();
    tile.set_transform(Affine::scale(8., 8.)).unwrap();
    for limits in [
        CompositeLimits {
            max_mask_runs: 0,
            ..Default::default()
        },
        CompositeLimits {
            max_depth: 1,
            ..Default::default()
        },
        CompositeLimits {
            max_nodes: 3,
            ..Default::default()
        },
        CompositeLimits {
            max_total_points: 9,
            ..Default::default()
        },
        CompositeLimits {
            max_path_bytes: 9 * size_of::<PathCommand>() as u64,
            ..Default::default()
        },
    ] {
        let mut s = CanvasSurface::new(Canvas::new(16., 16.), limits).unwrap();
        let before = s.snapshot();
        assert!(
            s.draw_commands(&[fill(
                CanvasPathTarget::Explicit(rect(0., 0., 16., 16.)),
                tile.clone()
            )])
            .is_err(),
            "{limits:?}"
        );
        assert!(Arc::ptr_eq(&before, &s.snapshot()));
        assert_eq!(s.generation(), 1);
    }
}

#[test]
fn canvas_pattern_singular_invalid_geometry_and_late_commands_still_roll_back_path() {
    let mut s = surface(false, 1.);
    s.draw_commands(&append(&rect(2., 2., 10., 10.))).unwrap();
    let mut tile = pattern(&s, CanvasPatternRepeat::Repeat);
    tile.set_transform(Affine::scale(0., 1.)).unwrap();
    let before = s.clone();
    let invalid_path = Path::new(vec![PathCommand::MoveTo {
        to: Point::new(f32::INFINITY, 0.),
    }]);
    for commands in [
        vec![
            CanvasCommand::BeginPath,
            fill(CanvasPathTarget::Explicit(invalid_path), tile.clone()),
        ],
        vec![
            CanvasCommand::BeginPath,
            full(tile.clone()),
            CanvasCommand::Restore,
        ],
        vec![
            CanvasCommand::BeginPath,
            stroke(
                CanvasPathTarget::Current,
                tile,
                CanvasStrokeOptions {
                    width: -1.,
                    ..Default::default()
                },
            ),
        ],
    ] {
        assert!(s.draw_commands(&commands).is_err());
        unchanged(&s, &before);
    }
}

#[test]
fn canvas_pattern_advanced_stroke_triangle_budget_is_enforced_atomically() {
    let mut s = CanvasSurface::new(
        Canvas::new(32., 32.),
        CompositeLimits {
            max_triangles: 0,
            ..Default::default()
        },
    )
    .unwrap();
    let tile = solid_pattern(&s, [255; 4]);
    let before = s.snapshot();
    let path = Path::from_svg("M 3 26 L 14 5 L 27 26").unwrap();
    let options = CanvasStrokeOptions {
        width: 4.,
        cap: StrokeCap::Square,
        join: StrokeJoin::Miter,
        dash_pattern: vec![3., 2.],
        ..Default::default()
    };
    assert!(
        s.draw_commands(&[stroke(CanvasPathTarget::Explicit(path), tile, options)])
            .is_err()
    );
    assert!(Arc::ptr_eq(&before, &s.snapshot()));
    assert_eq!(s.generation(), 1);
}

#[test]
fn canvas_pattern_dpr_snapshot_samples_physical_pixels_until_explicitly_scaled() {
    let mut canvas = Canvas::new(8., 8.);
    canvas.pixel_ratio = 2.;
    let mut source = CanvasSurface::new(canvas, Default::default()).unwrap();
    source
        .fill_rect(bounds(0., 0., 8., 8.), Color::hex(0x00ff00))
        .unwrap();
    source
        .fill_rect(bounds(0., 0., 2., 8.), Color::hex(0xff0000))
        .unwrap();
    assert_eq!(source.snapshot().width(), 16);
    let mut inputs = ImageInputs::default();
    inputs.insert("tile".into(), Ok(source.snapshot()));
    for dpr in [1., 2.] {
        let mut destination = surface(false, dpr);
        let mut tile = destination
            .create_pattern(&"tile".into(), &inputs, CanvasPatternRepeat::NoRepeat)
            .unwrap();
        destination.draw_commands(&[full(tile.clone())]).unwrap();
        let d = dpr as usize;
        // Four red physical source pixels initially span four logical units.
        assert_eq!(pixel(&destination, 3 * d, 3 * d), [255, 0, 0, 255]);
        assert_eq!(pixel(&destination, 6 * d, 3 * d), [0, 255, 0, 255]);
        assert_eq!(pixel(&destination, 12 * d, 3 * d), [0, 255, 0, 255]);
        assert_eq!(pixel(&destination, 17 * d, 3 * d), [0; 4]);
        destination.clear().unwrap();
        tile.set_transform(Affine::scale(0.5, 0.5)).unwrap();
        destination.draw_commands(&[full(tile)]).unwrap();
        assert_eq!(pixel(&destination, d, 3 * d), [255, 0, 0, 255]);
        assert_eq!(pixel(&destination, 3 * d, 3 * d), [0, 255, 0, 255]);
        assert_eq!(pixel(&destination, 7 * d, 3 * d), [0, 255, 0, 255]);
        assert_eq!(pixel(&destination, 9 * d, 3 * d), [0; 4]);
    }
}

#[test]
fn canvas_pattern_zero_opacity_does_not_bypass_composed_transform_validation() {
    let mut s = surface(false, 1.);
    s.draw_commands(&append(&rect(2., 2., 10., 10.))).unwrap();
    let before = s.clone();
    let mut tile = pattern(&s, CanvasPatternRepeat::Repeat);
    tile.set_transform(Affine::scale(1_000_000., 1.)).unwrap();
    assert_eq!(
        s.draw_commands(&[
            CanvasCommand::BeginPath,
            CanvasCommand::SetOpacity(0.),
            CanvasCommand::SetTransform(Affine::scale(2., 1.)),
            full(tile),
        ]),
        Err(CompositeError::UnsupportedTransform)
    );
    unchanged(&s, &before);
}
