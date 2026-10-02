use super::*;
use gpui_flow_scene::{
    Affine, Color, FillRule, GradientStop, Paint, Point, Stroke, StrokeAlignment, StrokeCap,
    StrokeJoin,
    display::{Canvas, Path},
};
use std::sync::Arc;
fn surface() -> CanvasSurface {
    CanvasSurface::new(Canvas::new(32., 32.), CompositeLimits::default()).unwrap()
}
fn path(s: &str) -> Path {
    Path::from_svg(s).unwrap()
}
fn square() -> Path {
    path("M 0 0 L 6 0 L 6 6 L 0 6 Z")
}
fn fill(p: Path, c: Color) -> CanvasCommand {
    CanvasCommand::FillPath {
        path: p,
        paint: c.into(),
        rule: FillRule::NonZero,
    }
}
fn pixel(s: &CanvasSurface, x: usize, y: usize) -> [u8; 4] {
    let p = s.snapshot();
    p.premultiplied_rgba()[(y * p.width() as usize + x) * 4..][..4]
        .try_into()
        .unwrap()
}
#[test]
fn canvas_commands_fill_curves_and_evenodd_holes_publish_once() {
    let mut s = surface();
    let old = s.snapshot();
    let curves = path("M 2 16 Q 8 6 14 16 C 19 28 25 28 30 16 L 30 30 L 2 30 Z");
    let hole = path("M 1 1 L 15 1 L 15 15 L 1 15 Z M 5 5 L 11 5 L 11 11 L 5 11 Z");
    assert!(
        s.draw_commands(&[
            fill(curves, Color::WHITE),
            CanvasCommand::FillPath {
                path: hole,
                paint: Color::hex(0xff0000).into(),
                rule: FillRule::EvenOdd
            }
        ])
        .unwrap()
    );
    assert_eq!(s.generation(), 2);
    assert_eq!(pixel(&s, 2, 2), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 8, 8), [0; 4]);
    assert_eq!(pixel(&s, 8, 26), [255; 4]);
    assert!(old.premultiplied_rgba().iter().all(|v| *v == 0));
}
#[test]
fn canvas_commands_transform_order_restore_and_next_batch_state_are_explicit() {
    let mut s = surface();
    s.draw_commands(&[
        CanvasCommand::Transform(Affine::translation(10., 2.)),
        CanvasCommand::Save,
        CanvasCommand::Transform(Affine::scale(2., 2.)),
        CanvasCommand::SetOpacity(0.5),
        fill(square(), Color::hex(0xff0000)),
        CanvasCommand::Restore,
        CanvasCommand::Transform(Affine::translation(0., 16.)),
        fill(square(), Color::hex(0x00ff00)),
    ])
    .unwrap();
    assert_eq!(pixel(&s, 11, 3), [128, 0, 0, 128]);
    assert_eq!(pixel(&s, 20, 3), [128, 0, 0, 128]);
    assert_eq!(pixel(&s, 9, 3), [0; 4]);
    assert_eq!(pixel(&s, 11, 19), [0, 255, 0, 255]);
    s.draw_commands(&[fill(square(), Color::WHITE)]).unwrap();
    assert_eq!(pixel(&s, 1, 1), [255; 4]);
}
#[test]
fn canvas_commands_opacity_is_per_draw_not_an_isolated_group() {
    let mut s = surface();
    s.draw_commands(&[
        CanvasCommand::SetOpacity(0.5),
        fill(square(), Color::hex(0xff0000)),
        fill(square(), Color::hex(0x0000ff)),
    ])
    .unwrap();
    assert_eq!(pixel(&s, 2, 2), [64, 0, 128, 192]);
}
#[test]
fn canvas_commands_gradients_follow_local_transform_and_dpr() {
    let mut c = Canvas::new(16., 16.);
    c.pixel_ratio = 2.;
    let mut s = CanvasSurface::new(c, CompositeLimits::default()).unwrap();
    let paint = Paint::LinearGradient {
        start: Point::new(0., 0.),
        end: Point::new(6., 0.),
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
        CanvasCommand::SetTransform(Affine::translation(2., 3.)),
        CanvasCommand::FillPath {
            path: square(),
            paint,
            rule: FillRule::NonZero,
        },
    ])
    .unwrap();
    assert_eq!(pixel(&s, 3, 7), [0; 4]);
    let left = pixel(&s, 5, 7);
    let right = pixel(&s, 14, 7);
    assert!(left[0] > left[2]);
    assert!(right[2] > right[0]);
    assert_eq!(left[3], 255);
    assert_eq!(pixel(&s, 5, 5), [0; 4]);
}
#[test]
fn canvas_commands_strokes_reuse_dash_caps_joins_and_single_alpha_union() {
    let mut s = surface();
    let stroke = Stroke::new(Color::rgba(255, 255, 255, 128), 4.)
        .with_cap(StrokeCap::Butt)
        .with_join(StrokeJoin::Miter)
        .with_dashes(vec![4., 4.], 0.);
    s.draw_commands(&[CanvasCommand::StrokePath {
        path: path("M 4 8 L 28 8 L 28 26"),
        stroke,
    }])
    .unwrap();
    assert_eq!(pixel(&s, 5, 8), [128; 4]);
    assert_eq!(pixel(&s, 10, 8), [0; 4]);
    assert_eq!(pixel(&s, 3, 8), [0; 4]);
    assert!(
        s.snapshot()
            .premultiplied_rgba()
            .chunks(4)
            .all(|p| p[3] <= 128)
    );
    let mut round = surface();
    round
        .draw_commands(&[CanvasCommand::StrokePath {
            path: path("M 6 16 L 20 16"),
            stroke: Stroke::new(Color::WHITE, 6.).with_cap(StrokeCap::Round),
        }])
        .unwrap();
    assert_eq!(pixel(&round, 4, 16), [255; 4]);
}
fn rejected(commands: &[CanvasCommand]) {
    let mut s = surface();
    s.draw_commands(&[fill(square(), Color::WHITE)]).unwrap();
    let old = s.snapshot();
    let generation = s.generation();
    assert!(s.draw_commands(commands).is_err());
    assert_eq!(s.generation(), generation);
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
}
#[test]
fn canvas_commands_late_invalid_geometry_state_or_stack_rolls_back_all_pixels() {
    let first = fill(square(), Color::BLACK);
    for bad in [
        CanvasCommand::Restore,
        CanvasCommand::Save,
        CanvasCommand::SetOpacity(f32::NAN),
        CanvasCommand::SetOpacity(-0.1),
        CanvasCommand::Transform(Affine::scale(0., 1.)),
        CanvasCommand::SetTransform(Affine::translation(f32::INFINITY, 0.)),
        CanvasCommand::Transform(Affine::scale(1_000_001., 1.)),
        fill(
            Path::new(vec![gpui_flow_scene::display::PathCommand::LineTo {
                to: Point::ZERO,
            }]),
            Color::WHITE,
        ),
    ] {
        rejected(&[first.clone(), bad]);
    }
    rejected(&[
        CanvasCommand::Save,
        CanvasCommand::Transform(Affine::scale(1_000_000., 1.)),
        CanvasCommand::Transform(Affine::scale(2., 1.)),
        CanvasCommand::Restore,
    ]);
}
#[test]
fn canvas_commands_empty_state_only_and_invisible_batches_do_not_publish() {
    let mut s = surface();
    let old = s.snapshot();
    assert!(!s.draw_commands(&[]).unwrap());
    assert!(
        !s.draw_commands(&[
            CanvasCommand::Save,
            CanvasCommand::SetOpacity(0.5),
            CanvasCommand::Restore
        ])
        .unwrap()
    );
    assert!(
        !s.draw_commands(&[fill(Path::default(), Color::WHITE)])
            .unwrap()
    );
    assert!(
        !s.draw_commands(&[CanvasCommand::SetOpacity(0.), fill(square(), Color::WHITE)])
            .unwrap()
    );
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
    assert_eq!(s.generation(), 1);
}
#[test]
fn canvas_commands_input_stack_and_paint_limits_precede_output() {
    rejected(&vec![
        CanvasCommand::SetOpacity(1.);
        MAX_CANVAS_COMMANDS + 1
    ]);
    rejected(&vec![CanvasCommand::Save; MAX_CANVAS_STATE_DEPTH + 1]);
    rejected(&[CanvasCommand::StrokePath {
        path: square(),
        stroke: Stroke::new(Color::WHITE, 1.).with_alignment(StrokeAlignment::Inside),
    }]);
    rejected(&[CanvasCommand::FillPath {
        path: square(),
        paint: Paint::LinearGradient {
            start: Point::ZERO,
            end: Point::new(1., 1.),
            stops: vec![
                GradientStop {
                    offset: 0.,
                    color: Color::WHITE
                };
                257
            ],
        },
        rule: FillRule::NonZero,
    }]);
    for limits in [
        CompositeLimits {
            max_points: 4,
            ..Default::default()
        },
        CompositeLimits {
            max_total_points: 9,
            ..Default::default()
        },
        CompositeLimits {
            max_nodes: 2,
            ..Default::default()
        },
        CompositeLimits {
            max_depth: 0,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_bytes: 8191,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_writes: 2000,
            ..Default::default()
        },
    ] {
        let mut s = CanvasSurface::new(Canvas::new(32., 32.), limits).unwrap();
        let old = s.snapshot();
        assert!(
            s.draw_commands(&[fill(square(), Color::WHITE), fill(square(), Color::WHITE)])
                .is_err()
        );
        assert!(Arc::ptr_eq(&old, &s.snapshot()));
        assert_eq!(s.generation(), 1);
    }
}
#[test]
fn canvas_commands_resize_resets_or_preserves_pixels_but_never_retains_context() {
    let mut s = surface();
    s.draw_commands(&[
        CanvasCommand::Transform(Affine::translation(10., 10.)),
        fill(square(), Color::WHITE),
    ])
    .unwrap();
    let mut c = Canvas::new(40., 40.);
    c.safe_resize = true;
    s.reconfigure(c.clone()).unwrap();
    assert_eq!(pixel(&s, 11, 11), [255; 4]);
    s.draw_commands(&[fill(square(), Color::hex(0xff0000))])
        .unwrap();
    assert_eq!(pixel(&s, 1, 1), [255, 0, 0, 255]);
    c.safe_resize = false;
    c.size.width = 41.;
    s.reconfigure(c).unwrap();
    assert_eq!(pixel(&s, 11, 11), [0; 4]);
}

fn clip(p: Path, rule: FillRule) -> CanvasCommand {
    CanvasCommand::ClipPath { path: p, rule }
}
#[test]
fn canvas_clips_freeze_transform_ignore_opacity_and_apply_dpr_once() {
    let mut c = Canvas::new(16., 16.);
    c.pixel_ratio = 2.;
    let mut s = CanvasSurface::new(c, CompositeLimits::default()).unwrap();
    s.draw_commands(&[
        CanvasCommand::SetOpacity(0.),
        CanvasCommand::Transform(Affine::translation(5., 3.)),
        clip(square(), FillRule::NonZero),
        CanvasCommand::SetTransform(Affine::IDENTITY),
        CanvasCommand::SetOpacity(1.),
        fill(path("M 0 0 L 16 0 L 16 16 L 0 16 Z"), Color::WHITE),
    ])
    .unwrap();
    assert_eq!(pixel(&s, 11, 7), [255; 4]);
    assert_eq!(pixel(&s, 9, 7), [0; 4]);
    assert_eq!(pixel(&s, 22, 7), [0; 4]);
    assert_eq!(pixel(&s, 11, 18), [0; 4]);
}
#[test]
fn canvas_clips_nested_intersection_and_restore_preserve_draw_order() {
    let mut s = surface();
    s.draw_commands(&[
        CanvasCommand::Save,
        clip(path("M 2 2 L 20 2 L 20 20 L 2 20 Z"), FillRule::NonZero),
        CanvasCommand::Save,
        clip(path("M 10 10 L 28 10 L 28 28 L 10 28 Z"), FillRule::NonZero),
        fill(path("M 0 0 L 32 0 L 32 32 L 0 32 Z"), Color::hex(0xff0000)),
        CanvasCommand::Restore,
        fill(square(), Color::hex(0x00ff00)),
        CanvasCommand::Restore,
        CanvasCommand::Transform(Affine::translation(24., 24.)),
        fill(square(), Color::hex(0x0000ff)),
    ])
    .unwrap();
    assert_eq!(pixel(&s, 15, 15), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 22, 15), [0; 4]);
    assert_eq!(pixel(&s, 3, 3), [0, 255, 0, 255]);
    assert_eq!(pixel(&s, 1, 1), [0; 4]);
    assert_eq!(pixel(&s, 26, 26), [0, 0, 255, 255]);
}
#[test]
fn canvas_clips_evenodd_holes_and_empty_clips_are_real_geometry() {
    let mut s = surface();
    let hole = path("M 1 1 L 15 1 L 15 15 L 1 15 Z M 5 5 L 11 5 L 11 11 L 5 11 Z");
    s.draw_commands(&[
        clip(hole, FillRule::EvenOdd),
        fill(path("M 0 0 L 32 0 L 32 32 L 0 32 Z"), Color::WHITE),
    ])
    .unwrap();
    assert_eq!(pixel(&s, 2, 2), [255; 4]);
    assert_eq!(pixel(&s, 8, 8), [0; 4]);
    assert_eq!(pixel(&s, 20, 20), [0; 4]);
    let mut empty = surface();
    empty
        .draw_commands(&[
            CanvasCommand::Save,
            clip(Path::default(), FillRule::NonZero),
            fill(square(), Color::WHITE),
            CanvasCommand::Restore,
            CanvasCommand::Transform(Affine::translation(10., 10.)),
            fill(square(), Color::WHITE),
        ])
        .unwrap();
    assert_eq!(pixel(&empty, 2, 2), [0; 4]);
    assert_eq!(pixel(&empty, 12, 12), [255; 4]);
}
#[test]
fn canvas_clips_apply_coverage_per_draw_before_source_over() {
    let curve = path("M 1.3 16 Q 14.7 -9 30.4 16 Q 16 39 1.3 16 Z");
    let paint = fill(path("M 0 0 L 32 0 L 32 32 L 0 32 Z"), Color::WHITE);
    let mut once = surface();
    once.draw_commands(&[clip(curve.clone(), FillRule::NonZero), paint.clone()])
        .unwrap();
    let mut twice = surface();
    twice
        .draw_commands(&[
            clip(curve.clone(), FillRule::NonZero),
            paint.clone(),
            paint.clone(),
        ])
        .unwrap();
    let one = once.snapshot();
    let two = twice.snapshot();
    let mut edge = 0;
    for (a, b) in one
        .premultiplied_rgba()
        .chunks(4)
        .zip(two.premultiplied_rgba().chunks(4))
    {
        let x = a[3] as u16;
        let expected = x + (x * (255 - x) + 127) / 255;
        assert!((b[3] as i32 - expected as i32).abs() <= 1);
        if x > 10 && x < 245 {
            edge += 1;
            assert!(b[3] > a[3]);
        }
    }
    assert!(edge > 15);
    once.draw_commands(&[clip(curve, FillRule::NonZero), paint])
        .unwrap();
    assert_eq!(
        once.snapshot().premultiplied_rgba(),
        two.premultiplied_rgba()
    );
}
#[test]
fn canvas_clips_do_not_leak_between_batches_or_paint_by_themselves() {
    let mut s = surface();
    let old = s.snapshot();
    assert!(
        !s.draw_commands(&[clip(square(), FillRule::NonZero)])
            .unwrap()
    );
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
    s.draw_commands(&[
        clip(Path::default(), FillRule::NonZero),
        fill(square(), Color::WHITE),
    ])
    .unwrap();
    assert!(s.snapshot().premultiplied_rgba().iter().all(|v| *v == 0));
    s.draw_commands(&[
        CanvasCommand::Transform(Affine::translation(20., 20.)),
        fill(square(), Color::WHITE),
    ])
    .unwrap();
    assert_eq!(pixel(&s, 22, 22), [255; 4]);
}
#[test]
fn canvas_clips_invalid_late_path_and_compiled_mask_budgets_are_atomic() {
    rejected(&[
        fill(square(), Color::WHITE),
        clip(
            Path::new(vec![gpui_flow_scene::display::PathCommand::MoveTo {
                to: Point::new(f32::NAN, 1.),
            }]),
            FillRule::NonZero,
        ),
    ]);
    let commands = [
        clip(square(), FillRule::NonZero),
        fill(path("M 0 0 L 32 0 L 32 32 L 0 32 Z"), Color::WHITE),
        fill(path("M 0 0 L 32 0 L 32 32 L 0 32 Z"), Color::WHITE),
    ];
    for limits in [
        CompositeLimits {
            max_total_points: 15,
            ..Default::default()
        },
        CompositeLimits {
            max_nodes: 6,
            ..Default::default()
        },
        CompositeLimits {
            max_mask_runs: 1,
            ..Default::default()
        },
        CompositeLimits {
            max_depth: 1,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_bytes: 10_000,
            ..Default::default()
        },
        CompositeLimits {
            max_surface_writes: 4000,
            ..Default::default()
        },
    ] {
        let mut s = CanvasSurface::new(Canvas::new(32., 32.), limits).unwrap();
        let old = s.snapshot();
        assert!(s.draw_commands(&commands).is_err(), "{limits:?}");
        assert!(Arc::ptr_eq(&old, &s.snapshot()));
        assert_eq!(s.generation(), 1);
    }
}
