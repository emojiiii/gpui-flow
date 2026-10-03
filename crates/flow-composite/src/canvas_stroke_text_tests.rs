//! Supplied curved-outline protocol tests. Font identity/shaping are tested in
//! flow-text and the native host; these fixtures make no native-font claim.
use super::*;
use gpui_flow_scene::{
    Affine, Color, FillRule, Point, Size, Stroke, StrokeCap, StrokeJoin,
    display::{Canvas, Path, Text, TextDecoration},
};
use std::{
    cell::Cell,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
fn surface(limits: CompositeLimits) -> CanvasSurface {
    CanvasSurface::new(Canvas::new(48., 40.), limits).unwrap()
}
fn text() -> Text {
    Text::new("Outline", Size::new(24., 22.))
}
fn command() -> CanvasCommand {
    CanvasCommand::StrokeTextBox {
        text: text(),
        position: Point::new(3., 4.),
        color: Color::WHITE,
        options: CanvasStrokeOptions {
            width: 2.,
            ..Default::default()
        },
    }
}
fn curves() -> Path {
    Path::from_svg(
        "M 3 15 Q 2 2 10 3 C 18 2 22 15 15 18 L 3 15 Z M 8 12 Q 9 6 12 8 C 14 10 14 14 8 12 Z",
    )
    .unwrap()
}
fn output(r: CanvasFontRequest<'_>) -> Result<CanvasFontOutput, CompositeError> {
    assert!(r.fills.source.text_sources().is_empty());
    assert!(r.max_outline_commands >= curves().commands.len());
    assert!(r.max_outline_path_bytes > 0);
    Ok(CanvasFontOutput {
        outline_work: 0,
        fills: Default::default(),
        strokes: r
            .strokes
            .iter()
            .map(|job| CanvasStrokeTextOutput {
                id: job.id.clone(),
                path: curves(),
            })
            .collect(),
    })
}
fn path_start() -> Vec<CanvasCommand> {
    vec![
        CanvasCommand::MoveTo {
            to: Point::new(1., 1.),
        },
        CanvasCommand::LineTo {
            to: Point::new(10., 1.),
        },
        CanvasCommand::LineTo {
            to: Point::new(10., 9.),
        },
        CanvasCommand::ClosePath,
    ]
}
fn assert_unchanged(s: &CanvasSurface, before: &CanvasSurface) {
    assert!(Arc::ptr_eq(&s.snapshot(), &before.snapshot()));
    assert_eq!(
        s.snapshot().premultiplied_rgba(),
        before.snapshot().premultiplied_rgba()
    );
    assert_eq!(s.generation(), before.generation());
    assert_eq!(s.current_path_byte_len(), before.current_path_byte_len());
    if before.current_path_byte_len() == 0 {
        return;
    }
    // Compare persistent geometry by painting the next valid batch.
    let mut actual = s.clone();
    let mut expected = before.clone();
    let draw = [CanvasCommand::FillCurrentPath {
        paint: Color::WHITE.into(),
        rule: FillRule::NonZero,
    }];
    actual.draw_commands(&draw).unwrap();
    expected.draw_commands(&draw).unwrap();
    assert_eq!(
        actual.snapshot().premultiplied_rgba(),
        expected.snapshot().premultiplied_rgba()
    );
}
#[test]
fn canvas_stroke_text_typed_jobs_are_required_and_legacy_never_invokes_fill_producer() {
    let mut s = surface(Default::default());
    let before = s.clone();
    let called = Cell::new(false);
    assert!(
        s.draw_commands_with_text(&[command()], &Default::default(), |_| {
            called.set(true);
            Ok(Default::default())
        })
        .is_err()
    );
    assert!(!called.get());
    assert_unchanged(&s, &before);
    assert!(
        s.draw_commands_with_fonts(&[command()], &Default::default(), |r| {
            assert_eq!(r.strokes.len(), 1);
            assert_eq!(r.strokes[0].text, text());
            assert_eq!(r.strokes[0].world_transform, Affine::translation(3., 4.));
            output(r)
        })
        .unwrap()
    );
    assert!(s.snapshot().premultiplied_rgba().iter().any(|v| *v != 0));
}
#[test]
fn canvas_stroke_text_exact_ids_empty_results_duplicates_and_bad_geometry_fail_closed() {
    let mut s = surface(Default::default());
    s.draw_commands(&path_start()).unwrap();
    let before = s.clone();
    for failure in 0..5 {
        assert!(
            s.draw_commands_with_fonts(
                &[CanvasCommand::BeginPath, command(), command()],
                &Default::default(),
                |r| {
                    let mut result = output(r)?;
                    match failure {
                        0 => {
                            result.strokes.pop();
                        }
                        1 => result.strokes[0].id = "wrong".into(),
                        2 => result.strokes[0].id = result.strokes[1].id.clone(),
                        3 => {
                            result.strokes[0].path =
                                Path::new(vec![gpui_flow_scene::display::PathCommand::LineTo {
                                    to: Point::new(1., 1.),
                                }])
                        }
                        _ => result.strokes[0].path.commands.push(
                            gpui_flow_scene::display::PathCommand::LineTo {
                                to: Point::new(f32::NAN, 1.),
                            },
                        ),
                    }
                    Ok(result)
                }
            )
            .is_err()
        );
        assert_unchanged(&s, &before);
    }
    s.draw_commands_with_fonts(&[command()], &Default::default(), output)
        .unwrap();
}
#[test]
fn canvas_stroke_text_matches_single_curved_path_under_affine_dpr_clip_alpha_and_dash() {
    let mut config = Canvas::new(48., 40.);
    config.pixel_ratio = 2.;
    let mut s = CanvasSurface::new(config.clone(), Default::default()).unwrap();
    let mut expected = CanvasSurface::new(config, Default::default()).unwrap();
    let options = CanvasStrokeOptions {
        width: 2.5,
        cap: StrokeCap::Square,
        join: StrokeJoin::Miter,
        miter_limit: 3.,
        dash_pattern: vec![3., 2., 1.],
        dash_offset: -2.,
    };
    let affine = Affine {
        a: 1.1,
        b: 0.15,
        c: -0.2,
        d: 0.9,
        tx: 5.,
        ty: 2.,
    };
    let mut value = text();
    value.padding = [2., 3., 4., 1.];
    let prelude = vec![
        CanvasCommand::ClipPath {
            path: Path::from_svg("M 0 0 L 30 0 L 30 38 L 0 38 Z").unwrap(),
            rule: FillRule::NonZero,
        },
        CanvasCommand::SetTransform(affine),
        CanvasCommand::SetOpacity(0.5),
    ];
    let mut commands = prelude.clone();
    commands.push(CanvasCommand::StrokeTextBox {
        text: value.clone(),
        position: Point::new(3., 4.),
        color: Color::WHITE,
        options: options.clone(),
    });
    s.draw_commands_with_fonts(&commands, &Default::default(), output)
        .unwrap();
    let b = value.content_bounds();
    let mut reference = prelude;
    reference.extend([
        CanvasCommand::Transform(Affine::translation(3., 4.)),
        CanvasCommand::ClipPath {
            path: Path::from_svg(&format!(
                "M {} {} L {} {} L {} {} L {} {} Z",
                b.min_x(),
                b.min_y(),
                b.max_x(),
                b.min_y(),
                b.max_x(),
                b.max_y(),
                b.min_x(),
                b.max_y()
            ))
            .unwrap(),
            rule: FillRule::NonZero,
        },
        CanvasCommand::StrokePath {
            path: curves(),
            stroke: Stroke {
                width: options.width,
                cap: options.cap,
                join: options.join,
                miter_limit: options.miter_limit,
                dash_pattern: options.dash_pattern,
                dash_offset: options.dash_offset,
                ..Stroke::new(Color::WHITE, options.width)
            },
        },
    ]);
    expected.draw_commands(&reference).unwrap();
    assert_eq!(
        s.snapshot().premultiplied_rgba(),
        expected.snapshot().premultiplied_rgba()
    );
    assert!(
        s.snapshot()
            .premultiplied_rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] <= 128)
    );
}
#[test]
fn canvas_stroke_text_content_clip_rejects_outside_ink_and_preserves_holes() {
    let mut s = surface(Default::default());
    s.draw_commands_with_fonts(&[command()], &Default::default(), |r| {
        Ok(CanvasFontOutput {
            outline_work: 0,
            fills: Default::default(),
            strokes: vec![CanvasStrokeTextOutput {
                id: r.strokes[0].id.clone(),
                path: Path::from_svg("M -10 10 L 40 10 M 4 4 L 18 4 L 18 18 L 4 18 Z").unwrap(),
            }],
        })
    })
    .unwrap();
    let image = s.snapshot();
    let at =
        |x: usize, y: usize| image.premultiplied_rgba()[(y * image.width() as usize + x) * 4 + 3];
    assert_eq!(at(2, 14), 0);
    assert_eq!(at(27, 14), 0);
    assert_eq!(at(14, 12), 0);
    assert!(at(3, 14) > 0);
}
#[test]
fn canvas_stroke_text_overlapping_contours_form_one_alpha_union() {
    let mut s = surface(Default::default());
    s.draw_commands_with_fonts(
        &[CanvasCommand::SetOpacity(0.5), command()],
        &Default::default(),
        |r| {
            let mut path = curves();
            path.commands.extend(curves().commands);
            Ok(CanvasFontOutput {
                outline_work: 0,
                fills: Default::default(),
                strokes: vec![CanvasStrokeTextOutput {
                    id: r.strokes[0].id.clone(),
                    path,
                }],
            })
        },
    )
    .unwrap();
    let image = s.snapshot();
    assert!(
        image
            .premultiplied_rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .any(|p| p[3] == 128)
    );
    assert!(
        image
            .premultiplied_rgba()
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| p[3] <= 128)
    );
}
#[test]
fn canvas_stroke_text_late_producer_command_and_validator_failures_rollback_all_state() {
    let mut s = surface(Default::default());
    s.draw_commands(&path_start()).unwrap();
    let before = s.clone();
    let calls = Cell::new(0);
    assert!(
        s.draw_commands_with_fonts(
            &[command(), CanvasCommand::Restore],
            &Default::default(),
            |_| {
                calls.set(1);
                Ok(Default::default())
            }
        )
        .is_err()
    );
    assert_eq!(calls.get(), 0);
    assert_eq!(
        s.draw_commands_with_fonts(
            &[CanvasCommand::BeginPath, command()],
            &Default::default(),
            |_| Err(CompositeError::Superseded)
        ),
        Err(CompositeError::Superseded)
    );
    assert_unchanged(&s, &before);
    for reject in [1, 2, 3] {
        calls.set(0);
        assert_eq!(
            s.draw_commands_with_fonts_guarded(
                &[CanvasCommand::BeginPath, command()],
                &Default::default(),
                &AtomicBool::new(false),
                output,
                || {
                    calls.set(calls.get() + 1);
                    if calls.get() == reject {
                        Err(CompositeError::Superseded)
                    } else {
                        Ok(())
                    }
                }
            ),
            Err(CompositeError::Superseded)
        );
        assert_eq!(calls.get(), reject);
        assert_unchanged(&s, &before);
    }
    s.draw_commands_with_fonts(&[command()], &Default::default(), output)
        .unwrap();
}
#[test]
fn canvas_stroke_text_cancellation_and_geometry_only_final_guards_preserve_context() {
    let mut s = surface(Default::default());
    s.draw_commands(&path_start()).unwrap();
    let before = s.clone();
    let token = AtomicBool::new(false);
    assert_eq!(
        s.draw_commands_with_fonts_guarded(
            &[CanvasCommand::BeginPath, command()],
            &Default::default(),
            &token,
            |r| {
                let result = output(r);
                token.store(true, Ordering::Release);
                result
            },
            || Ok(())
        ),
        Err(CompositeError::Superseded)
    );
    assert_unchanged(&s, &before);
    token.store(false, Ordering::Release);
    let calls = Cell::new(0);
    assert_eq!(
        s.draw_commands_with_fonts_guarded(
            &[CanvasCommand::BeginPath],
            &Default::default(),
            &token,
            |_| panic!("no font work"),
            || {
                calls.set(calls.get() + 1);
                if calls.get() == 2 {
                    Err(CompositeError::Superseded)
                } else {
                    Ok(())
                }
            }
        ),
        Err(CompositeError::Superseded)
    );
    assert_unchanged(&s, &before);
}
#[test]
fn canvas_stroke_text_reduced_geometry_limits_reject_whole_batch_and_allow_retry() {
    for limits in [
        CompositeLimits {
            max_points: 4,
            ..Default::default()
        },
        CompositeLimits {
            max_total_points: 15,
            ..Default::default()
        },
        CompositeLimits {
            max_triangles: 1,
            ..Default::default()
        },
        CompositeLimits {
            max_path_bytes: 1024,
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
            max_mask_runs: 0,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_bytes: 16_000,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_writes: 2_000,
            ..Default::default()
        },
    ] {
        let mut s = surface(limits);
        let before = s.clone();
        assert!(
            s.draw_commands_with_fonts(&[command()], &Default::default(), |r| Ok(
                CanvasFontOutput {
                    outline_work: 0,
                    fills: Default::default(),
                    strokes: r
                        .strokes
                        .iter()
                        .map(|job| CanvasStrokeTextOutput {
                            id: job.id.clone(),
                            path: curves()
                        })
                        .collect()
                }
            ))
            .is_err()
        );
        assert_unchanged(&s, &before);
        s.draw_commands(&[CanvasCommand::BeginPath]).unwrap();
    }
}
#[test]
fn canvas_stroke_text_decoration_is_rejected_before_host_work_and_generic_text_stays_unsupported() {
    let mut s = surface(Default::default());
    let mut decorated = text();
    decorated.decoration = TextDecoration::Underline;
    assert!(
        s.draw_commands_with_fonts(
            &[CanvasCommand::StrokeTextBox {
                text: decorated,
                position: Point::ZERO,
                color: Color::WHITE,
                options: Default::default()
            }],
            &Default::default(),
            |_| panic!("not admitted")
        )
        .is_err()
    );
    let object =
        gpui_flow_scene::DisplayObject::new("generic", text()).with_style(gpui_flow_scene::Style {
            stroke: Some(Stroke::new(Color::WHITE, 2.)),
            ..Default::default()
        });
    assert!(object.validate().is_ok());
    let mut scene = gpui_flow_scene::Scene::new();
    scene.insert(object.clone(), None, None).unwrap();
    let source = capture_subtree(&scene, &"generic".into(), Default::default()).unwrap();
    assert!(matches!(
        s.draw(
            &source,
            Affine::IDENTITY,
            &Default::default(),
            &Default::default()
        ),
        Err(CompositeError::UnsupportedTextStyle(_))
    ));
    let mut advanced = object;
    advanced.style.stroke.as_mut().unwrap().cap = StrokeCap::Square;
    assert!(advanced.validate().is_err());
}

#[test]
fn canvas_stroke_text_empty_outline_is_valid_and_text_drawing_keeps_current_path() {
    let mut s = surface(Default::default());
    s.draw_commands(&path_start()).unwrap();
    let path_bytes = s.current_path_byte_len();
    s.draw_commands_with_fonts(&[command()], &Default::default(), |r| {
        Ok(CanvasFontOutput {
            outline_work: 0,
            fills: Default::default(),
            strokes: vec![CanvasStrokeTextOutput {
                id: r.strokes[0].id.clone(),
                path: Path::default(),
            }],
        })
    })
    .unwrap();
    assert_eq!(s.current_path_byte_len(), path_bytes);
    assert!(s.snapshot().premultiplied_rgba().iter().all(|p| *p == 0));
    assert!(
        s.draw_commands(&[CanvasCommand::FillCurrentPath {
            paint: Color::WHITE.into(),
            rule: FillRule::NonZero
        }])
        .unwrap()
    );
}
#[test]
fn canvas_stroke_text_shared_triangle_limit_cannot_reset_at_the_next_command() {
    let count = gpui_flow_scene::Shape::Path(curves())
        .stroke_mesh(&Stroke::new(Color::WHITE, 2.))
        .unwrap()
        .triangles()
        .len();
    let mut s = surface(CompositeLimits {
        max_triangles: count,
        ..Default::default()
    });
    let before = s.clone();
    assert!(
        s.draw_commands_with_fonts(&[command(), command()], &Default::default(), output)
            .is_err()
    );
    assert_unchanged(&s, &before);
    assert!(
        s.draw_commands_with_fonts(&[command()], &Default::default(), output)
            .unwrap()
    );
}

#[test]
fn canvas_stroke_text_fill_jobs_stay_separate_and_outline_work_is_charged() {
    let mut s = surface(Default::default());
    let before = s.clone();
    for overflow in [false, true] {
        assert_eq!(
            s.draw_commands_with_fonts(&[command()], &Default::default(), |r| {
                let mut result = output(r)?;
                result.outline_work = if overflow { u64::MAX } else { 32_000_001 };
                result.fills.pixel_work = 1;
                Ok(result)
            }),
            Err(CompositeError::WorkLimit)
        );
        assert_unchanged(&s, &before);
    }
    s.draw_commands_with_fonts(
        &[
            CanvasCommand::FillTextBox {
                text: text(),
                position: Point::ZERO,
                color: Color::WHITE,
            },
            command(),
        ],
        &Default::default(),
        |r| {
            assert_eq!(r.strokes.len(), 1);
            let mut fills = TextInputs::default();
            assert_eq!(r.fills.source.text_sources().len(), 1);
            for (object, _, _) in r.fills.source.text_sources() {
                fills.insert(object.id.clone(), Ok(None));
            }
            Ok(CanvasFontOutput {
                fills: CanvasTextOutput {
                    inputs: fills,
                    pixel_work: 0,
                },
                strokes: vec![CanvasStrokeTextOutput {
                    id: r.strokes[0].id.clone(),
                    path: curves(),
                }],
                outline_work: 100,
            })
        },
    )
    .unwrap();
    assert!(s.snapshot().premultiplied_rgba().iter().any(|p| *p != 0));
}
