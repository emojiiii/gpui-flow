use super::*;
use gpui_flow_scene::{
    Affine, Bounds, Color, FillRule, GradientStop, Paint, Point, Size, Stroke, StrokeCap,
    display::{Canvas, Path, PathCommand, Text},
};
use std::{cell::Cell, sync::Arc};
fn surface() -> CanvasSurface {
    CanvasSurface::new(Canvas::new(32., 32.), Default::default()).unwrap()
}
fn mov(x: f32, y: f32) -> CanvasCommand {
    CanvasCommand::MoveTo {
        to: Point::new(x, y),
    }
}
fn line(x: f32, y: f32) -> CanvasCommand {
    CanvasCommand::LineTo {
        to: Point::new(x, y),
    }
}
fn square() -> Vec<CanvasCommand> {
    vec![
        mov(2., 2.),
        line(14., 2.),
        line(14., 14.),
        line(2., 14.),
        CanvasCommand::ClosePath,
    ]
}
fn fill() -> CanvasCommand {
    CanvasCommand::FillCurrentPath {
        paint: Color::WHITE.into(),
        rule: FillRule::NonZero,
    }
}
fn explicit(svg: &str) -> CanvasCommand {
    CanvasCommand::FillPath {
        path: Path::from_svg(svg).unwrap(),
        paint: Color::WHITE.into(),
        rule: FillRule::NonZero,
    }
}
fn pixel(s: &CanvasSurface, x: usize, y: usize) -> [u8; 4] {
    let p = s.snapshot();
    p.premultiplied_rgba()[(y * p.width() as usize + x) * 4..][..4]
        .try_into()
        .unwrap()
}
fn replay(s: &CanvasSurface) -> Vec<u8> {
    let mut s = s.clone();
    s.clear().unwrap();
    s.draw_commands(&[fill()]).unwrap();
    s.snapshot().premultiplied_rgba().to_vec()
}
fn unchanged(s: &CanvasSurface, before: &CanvasSurface) {
    assert_eq!(s.generation(), before.generation());
    assert!(Arc::ptr_eq(&s.snapshot(), &before.snapshot()));
    assert_eq!(replay(s), replay(before));
}
#[test]
fn canvas_current_path_cross_call_path_only_commit_and_reusable_draw() {
    let mut s = surface();
    let old = s.snapshot();
    let bytes = s.byte_len();
    assert!(!s.draw_commands(&square()[..2]).unwrap());
    assert!(!s.draw_commands(&square()[2..]).unwrap());
    assert_eq!(s.generation(), 1);
    assert_eq!(s.byte_len(), bytes);
    assert!(s.current_path_byte_len() > 0);
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
    assert!(s.draw_commands(&[fill()]).unwrap());
    assert_eq!(pixel(&s, 4, 4), [255; 4]);
    s.clear().unwrap();
    assert!(s.draw_commands(&[fill()]).unwrap());
    assert_eq!(pixel(&s, 4, 4), [255; 4]);
    let before = s.snapshot();
    assert!(!s.draw_commands(&[CanvasCommand::BeginPath]).unwrap());
    assert!(Arc::ptr_eq(&before, &s.snapshot()));
    s.clear().unwrap();
    assert!(!s.draw_commands(&[fill()]).unwrap());
    assert_eq!(pixel(&s, 4, 4), [0; 4]);
}
#[test]
fn canvas_current_path_save_restore_excludes_path_and_begin_preserves_state() {
    let mut s = surface();
    s.draw_commands(&[
        CanvasCommand::SetTransform(Affine::translation(10., 0.)),
        CanvasCommand::SetOpacity(0.5),
        CanvasCommand::Save,
        CanvasCommand::BeginPath,
        mov(1., 1.),
        line(8., 1.),
        CanvasCommand::Restore,
        line(8., 8.),
        line(1., 8.),
        CanvasCommand::ClosePath,
        fill(),
    ])
    .unwrap();
    assert_eq!(pixel(&s, 12, 3), [128; 4]);
    assert_eq!(pixel(&s, 2, 3), [0; 4]);
    s.clear().unwrap();
    s.draw_commands(&[fill()]).unwrap();
    assert_eq!(pixel(&s, 12, 3), [255; 4]);
}
#[test]
fn canvas_current_path_append_transform_is_frozen_and_draw_pen_uses_current_matrix() {
    for dpr in [1., 2.] {
        let mut config = Canvas::new(32., 32.);
        config.pixel_ratio = dpr;
        let mut s = CanvasSurface::new(config.clone(), Default::default()).unwrap();
        s.draw_commands(&[
            CanvasCommand::SetTransform(Affine::translation(2., 4.)),
            mov(2., 4.),
            line(24., 4.),
        ])
        .unwrap();
        let pen = Affine::translation(28., 0.) * Affine::scale(-2., 3.);
        let stroke = Stroke::new(Color::WHITE, 2.).with_cap(StrokeCap::Butt);
        s.draw_commands(&[
            CanvasCommand::SetTransform(pen),
            CanvasCommand::StrokeCurrentPath {
                stroke: stroke.clone(),
            },
        ])
        .unwrap();
        let mut expected = CanvasSurface::new(config, Default::default()).unwrap();
        expected
            .draw_commands(&[
                CanvasCommand::SetTransform(pen),
                CanvasCommand::StrokePath {
                    path: Path::from_svg("M 12 2.6666667 L 1 2.6666667").unwrap(),
                    stroke,
                },
            ])
            .unwrap();
        assert_eq!(
            s.snapshot().premultiplied_rgba(),
            expected.snapshot().premultiplied_rgba()
        );
        let scale = dpr as usize;
        assert_eq!(pixel(&s, 8 * scale, 6 * scale), [255; 4]);
        assert_eq!(pixel(&s, 8 * scale, 3 * scale), [0; 4]);
    }
}
#[test]
fn canvas_current_path_mixed_transforms_freeze_curve_controls_and_gradient_space() {
    let mut s = surface();
    s.draw_commands(&[
        mov(2., 16.),
        CanvasCommand::SetTransform(Affine::translation(2., 0.)),
        CanvasCommand::QuadraticCurveTo {
            control: Point::new(6., 6.),
            to: Point::new(12., 16.),
        },
        CanvasCommand::SetTransform(Affine::scale(2., 1.)),
        CanvasCommand::BezierCurveTo {
            control1: Point::new(9.5, 28.),
            control2: Point::new(12.5, 28.),
            to: Point::new(15., 16.),
        },
        CanvasCommand::SetTransform(Affine::IDENTITY),
        line(30., 30.),
        line(2., 30.),
        CanvasCommand::ClosePath,
    ])
    .unwrap();
    let paint = Paint::LinearGradient {
        start: Point::ZERO,
        end: Point::new(14., 0.),
        stops: vec![
            GradientStop {
                offset: 0.,
                color: Color::hex(0xff0000),
            },
            GradientStop {
                offset: 1.,
                color: Color::hex(0x0000ff),
            },
        ],
    };
    s.draw_commands(&[
        CanvasCommand::SetTransform(Affine::scale(2., 1.)),
        CanvasCommand::FillCurrentPath {
            paint: paint.clone(),
            rule: FillRule::NonZero,
        },
    ])
    .unwrap();
    let mut expected = surface();
    expected
        .draw_commands(&[
            CanvasCommand::SetTransform(Affine::scale(2., 1.)),
            CanvasCommand::FillPath {
                path: Path::from_svg("M 1 16 Q 4 6 7 16 C 9.5 28 12.5 28 15 16 L 15 30 L 1 30 Z")
                    .unwrap(),
                paint,
                rule: FillRule::NonZero,
            },
        ])
        .unwrap();
    assert_eq!(
        s.snapshot().premultiplied_rgba(),
        expected.snapshot().premultiplied_rgba()
    );
}
#[test]
fn canvas_current_path_implicit_starts_repeated_close_and_continuation_are_normalized() {
    let cases = vec![
        (
            vec![
                CanvasCommand::ClosePath,
                line(2., 2.),
                line(14., 2.),
                line(14., 14.),
                CanvasCommand::ClosePath,
                CanvasCommand::ClosePath,
                line(2., 14.),
                line(8., 24.),
                CanvasCommand::ClosePath,
            ],
            "M 2 2 L 14 2 L 14 14 Z M 2 2 L 2 14 L 8 24 Z",
        ),
        (
            vec![
                CanvasCommand::QuadraticCurveTo {
                    control: Point::new(2., 2.),
                    to: Point::new(14., 2.),
                },
                line(14., 14.),
                CanvasCommand::ClosePath,
            ],
            "M 2 2 Q 2 2 14 2 L 14 14 Z",
        ),
        (
            vec![
                CanvasCommand::BezierCurveTo {
                    control1: Point::new(2., 2.),
                    control2: Point::new(10., 2.),
                    to: Point::new(14., 2.),
                },
                line(14., 14.),
                CanvasCommand::ClosePath,
            ],
            "M 2 2 C 2 2 10 2 14 2 L 14 14 Z",
        ),
    ];
    for (commands, svg) in cases {
        let mut s = surface();
        s.draw_commands(&commands).unwrap();
        s.draw_commands(&[fill()]).unwrap();
        let mut e = surface();
        e.draw_commands(&[explicit(svg)]).unwrap();
        assert_eq!(
            s.snapshot().premultiplied_rgba(),
            e.snapshot().premultiplied_rgba()
        );
    }
}
#[test]
fn canvas_current_path_evenodd_holes_and_frozen_vector_clips() {
    let mut s = surface();
    s.draw_commands(&square()).unwrap();
    s.draw_commands(&[
        mov(5., 5.),
        line(11., 5.),
        line(11., 11.),
        line(5., 11.),
        CanvasCommand::ClosePath,
    ])
    .unwrap();
    s.draw_commands(&[
        CanvasCommand::SetTransform(Affine::translation(16., 0.)),
        CanvasCommand::ClipCurrentPath {
            rule: FillRule::EvenOdd,
        },
        CanvasCommand::BeginPath,
        CanvasCommand::SetTransform(Affine::IDENTITY),
        mov(20., 20.),
        line(30., 20.),
        line(30., 30.),
        line(20., 30.),
        CanvasCommand::ClosePath,
        CanvasCommand::SetTransform(Affine::IDENTITY),
        explicit("M 0 0 L 32 0 L 32 32 L 0 32 Z"),
    ])
    .unwrap();
    assert_eq!(pixel(&s, 3, 3), [255; 4]);
    assert_eq!(pixel(&s, 7, 7), [0; 4]);
    assert_eq!(pixel(&s, 22, 22), [0; 4]);
    s.clear().unwrap();
    s.draw_commands(&[fill()]).unwrap();
    assert_eq!(pixel(&s, 22, 22), [255; 4]);
    assert_eq!(pixel(&s, 3, 3), [0; 4]);
}
#[test]
fn canvas_current_path_clip_snapshot_survives_mutation_for_images_and_text() {
    for text in [false, true] {
        let mut s = surface();
        s.draw_commands(&square()).unwrap();
        let image = Arc::new(
            StaticImage::from_premultiplied_rgba(1, 1, vec![255; 4], Default::default()).unwrap(),
        );
        let mut images = ImageInputs::default();
        images.insert("input".into(), Ok(image.clone()));
        let draw = if text {
            CanvasCommand::FillTextBox {
                text: Text::new("ink", Size::new(32., 32.)),
                position: Point::ZERO,
                color: Color::WHITE,
            }
        } else {
            CanvasCommand::DrawImage {
                image: "input".into(),
                source: None,
                destination: Bounds::new(Point::ZERO, Size::new(32., 32.)),
            }
        };
        s.draw_commands_with_text(
            &[
                CanvasCommand::ClipCurrentPath {
                    rule: FillRule::NonZero,
                },
                line(30., 30.),
                CanvasCommand::BeginPath,
                draw,
            ],
            &images,
            |r| {
                let mut inputs = TextInputs::default();
                for (object, _, _) in r.source.text_sources() {
                    inputs.insert(
                        object.id.clone(),
                        Ok(Some(Arc::new(TextImage {
                            image: image.clone(),
                            local_bounds: Bounds::new(Point::ZERO, Size::new(32., 32.)),
                        }))),
                    );
                }
                Ok(CanvasTextOutput {
                    inputs,
                    pixel_work: 1,
                })
            },
        )
        .unwrap();
        assert_eq!(pixel(&s, 4, 4), [255; 4]);
        assert_eq!(pixel(&s, 20, 20), [0; 4]);
    }
}
#[test]
fn canvas_current_path_invalid_late_input_and_font_failure_roll_back_both_states() {
    let mut s = surface();
    s.draw_commands(&square()).unwrap();
    s.draw_commands(&[fill()]).unwrap();
    let old = s.clone();
    for bad in [
        vec![
            CanvasCommand::BeginPath,
            mov(20., 20.),
            fill(),
            CanvasCommand::Restore,
        ],
        vec![line(f32::NAN, 0.)],
        vec![
            CanvasCommand::SetTransform(Affine::scale(2., 2.)),
            line(f32::MAX, 0.),
        ],
        vec![
            CanvasCommand::BeginPath,
            mov(-f32::MAX, 0.),
            line(f32::MAX, 0.),
        ],
    ] {
        let called = Cell::new(false);
        assert!(
            s.draw_commands_with_text(&bad, &ImageInputs::default(), |_| {
                called.set(true);
                Ok(Default::default())
            })
            .is_err()
        );
        assert!(!called.get());
        unchanged(&s, &old);
    }
    assert_eq!(
        s.draw_commands_with_text(
            &[
                CanvasCommand::BeginPath,
                mov(20., 20.),
                explicit("M 1 1 L 4 1 L 4 4 Z")
            ],
            &ImageInputs::default(),
            |_| Err(CompositeError::Superseded)
        ),
        Err(CompositeError::Superseded)
    );
    unchanged(&s, &old);
    assert!(
        s.draw_commands(&[
            CanvasCommand::BeginPath,
            CanvasCommand::FillTextBox {
                text: Text::new("no backend", Size::new(20., 20.)),
                position: Point::ZERO,
                color: Color::WHITE
            }
        ])
        .is_err()
    );
    unchanged(&s, &old);
}
#[test]
fn canvas_current_path_point_and_work_budgets_cannot_be_reset_or_evaded_across_calls() {
    let limits = CompositeLimits {
        max_points: 5,
        max_total_points: 20,
        ..Default::default()
    };
    let mut s = CanvasSurface::new(Canvas::new(32., 32.), limits).unwrap();
    for command in square() {
        s.draw_commands(&[command]).unwrap();
    }
    let old = s.clone();
    assert_eq!(
        s.draw_commands(&[line(20., 20.)]),
        Err(CompositeError::InputLimit)
    );
    unchanged(&s, &old);
    let mut commands = Vec::new();
    for _ in 0..8 {
        commands.push(CanvasCommand::BeginPath);
        commands.extend([mov(1., 1.), line(2., 2.), line(3., 3.)]);
    }
    assert_eq!(s.draw_commands(&commands), Err(CompositeError::InputLimit));
    unchanged(&s, &old);
    assert_eq!(
        s.draw_commands(&vec![fill(); 5]),
        Err(CompositeError::InputLimit)
    );
    unchanged(&s, &old);
}
#[test]
fn canvas_current_path_snapshot_geometry_memory_and_pixel_work_fail_atomically() {
    let mut s = surface();
    s.draw_commands(&square()).unwrap();
    let old = s.clone();
    assert_eq!(
        s.with_current_path_budget(s.current_path_byte_len(), |s| s
            .draw_commands(&[line(20., 20.)])),
        Err(CompositeError::WorkingSetLimit)
    );
    unchanged(&s, &old);
    assert_eq!(
        s.with_current_path_budget(
            s.current_path_byte_len() + 4 * size_of::<PathCommand>() as u64,
            |s| s.draw_commands(&[fill()])
        ),
        Err(CompositeError::WorkingSetLimit)
    );
    unchanged(&s, &old);
    assert_eq!(
        s.draw_commands_with_text(
            &[CanvasCommand::BeginPath, explicit("M 1 1 L 4 1 L 4 4 Z")],
            &ImageInputs::default(),
            |r| Ok(CanvasTextOutput {
                inputs: Default::default(),
                pixel_work: r.limits.max_surface_writes + 1
            })
        ),
        Err(CompositeError::WorkLimit)
    );
    unchanged(&s, &old);
    let limits = CompositeLimits {
        max_surface_bytes: 8191,
        ..Default::default()
    };
    let mut limited = CanvasSurface::new(Canvas::new(32., 32.), limits).unwrap();
    limited.draw_commands(&square()).unwrap();
    let before = limited.snapshot();
    let path_bytes = limited.current_path_byte_len();
    assert_eq!(
        limited.draw_commands(&[CanvasCommand::BeginPath, explicit("M 1 1 L 4 1 L 4 4 Z")]),
        Err(CompositeError::WorkingSetLimit)
    );
    assert!(Arc::ptr_eq(&before, &limited.snapshot()));
    assert_eq!(limited.current_path_byte_len(), path_bytes);
}
#[test]
fn canvas_current_path_clone_cosmetics_clear_and_resize_lifecycle() {
    let mut s = surface();
    s.draw_commands(&square()).unwrap();
    let old = s.clone();
    s.draw_commands(&[CanvasCommand::BeginPath]).unwrap();
    assert!(replay(&old).iter().any(|x| *x != 0));
    assert!(replay(&s).iter().all(|x| *x == 0));
    s = old;
    let mut config = s.config().clone();
    config.smooth = false;
    s.reconfigure(config.clone()).unwrap();
    assert!(s.current_path_byte_len() > 0);
    s.clear().unwrap();
    assert!(s.current_path_byte_len() > 0);
    config.safe_resize = true;
    config.size.width = 24.;
    s.reconfigure(config.clone()).unwrap();
    assert_eq!(s.current_path_byte_len(), 0);
    s.draw_commands(&square()).unwrap();
    config.pixel_ratio = 2.;
    s.reconfigure(config).unwrap();
    assert_eq!(s.current_path_byte_len(), 0);
}
#[test]
fn canvas_current_path_scoped_budget_keeps_original_alive_across_multiple_calls() {
    let mut s = surface();
    s.draw_commands(&square()).unwrap();
    let original_bytes = s.current_path_byte_len();
    let mut expected = s.clone();
    expected.draw_commands(&[line(20., 20.)]).unwrap();
    let candidate_bytes = expected.current_path_byte_len();
    // Exactly enough for the first copy-on-write candidate. A second candidate
    // must still account for the adapter-owned original, even after self changes.
    let result = s.with_current_path_budget(original_bytes + candidate_bytes, |s| {
        s.draw_commands(&[line(20., 20.)])?;
        s.draw_commands(&[line(24., 24.)])
    });
    assert_eq!(result, Err(CompositeError::WorkingSetLimit));
    assert_eq!(s.current_path_byte_len(), expected.current_path_byte_len());
    assert_eq!(replay(&s), replay(&expected));
    // The temporary admission scope is gone; regular direct drawing still works.
    s.draw_commands(&[line(24., 24.)]).unwrap();
}
#[test]
fn canvas_current_path_escaping_clone_does_not_retain_temporary_budget_scope() {
    let mut s = surface();
    s.draw_commands(&square()).unwrap();
    let path_bytes = s.current_path_byte_len();
    let mut escaped = s
        .with_current_path_budget(path_bytes, |s| {
            s.with_current_path_budget(path_bytes, |s| {
                let mut clone = s.clone();
                assert_eq!(
                    clone.draw_commands(&[line(20., 20.)]),
                    Err(CompositeError::WorkingSetLimit)
                );
                Ok(clone)
            })
        })
        .unwrap();
    assert_eq!(escaped.additional_current_path_bytes(&s), 0);
    // A leaked lowered limit would reject this copy-on-write edit. A leaked
    // baseline handle would keep the previous allocation alive across scopes.
    escaped.draw_commands(&[line(20., 20.)]).unwrap();
    escaped.reset_current_path();
    escaped
        .with_current_path_budget(0, |s| s.draw_commands(&[CanvasCommand::BeginPath]))
        .unwrap();
    assert!(s.current_path_byte_len() > 0);
}
#[test]
fn canvas_current_path_inverse_overflow_rejects_before_geometry_or_pixels_change() {
    let mut s = surface();
    s.draw_commands(&[mov(f32::MAX / 4., 2.), line(f32::MAX / 2., 3.)])
        .unwrap();
    let before = s.snapshot();
    let path_bytes = s.current_path_byte_len();
    assert!(
        s.draw_commands(&[
            CanvasCommand::SetTransform(Affine::scale(1e-20, 1.)),
            fill()
        ])
        .is_err()
    );
    assert!(Arc::ptr_eq(&before, &s.snapshot()));
    assert_eq!(s.current_path_byte_len(), path_bytes);
    assert_eq!(s.generation(), 1);
}

#[test]
fn canvas_current_path_close_of_single_point_is_a_noop() {
    let mut s = CanvasSurface::new(
        Canvas::new(32., 32.),
        CompositeLimits {
            max_points: 1,
            ..Default::default()
        },
    )
    .unwrap();
    s.draw_commands(&[mov(4., 4.)]).unwrap();
    // Close adds no geometry/work for an empty or one-point subpath.
    assert!(
        !s.draw_commands(&[CanvasCommand::ClosePath, CanvasCommand::ClosePath])
            .unwrap()
    );
    let mut s = surface();
    s.draw_commands(&[mov(4., 4.), CanvasCommand::ClosePath])
        .unwrap();
    s.draw_commands(&[
        line(20., 4.),
        line(20., 20.),
        CanvasCommand::ClosePath,
        fill(),
    ])
    .unwrap();
    let mut expected = surface();
    expected
        .draw_commands(&[explicit("M 4 4 L 20 4 L 20 20 Z")])
        .unwrap();
    assert_eq!(
        s.snapshot().premultiplied_rgba(),
        expected.snapshot().premultiplied_rgba()
    );
}
