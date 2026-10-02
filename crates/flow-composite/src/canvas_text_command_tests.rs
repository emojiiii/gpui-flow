//! Synthetic ink tests the compositor protocol; actual fonts use the UI bridge.
use super::*;
use gpui_flow_scene::{
    Affine, Bounds, Color, FillRule, Point, Size,
    display::{Canvas, Path, Text},
};
use std::{cell::Cell, sync::Arc};
fn surface() -> CanvasSurface {
    CanvasSurface::new(Canvas::new(32., 32.), Default::default()).unwrap()
}
fn command() -> CanvasCommand {
    CanvasCommand::FillTextBox {
        text: Text::new("Ink", Size::new(12., 10.)),
        position: Point::new(2., 3.),
        color: Color::WHITE,
    }
}
fn ink(request: CanvasTextRequest<'_>) -> Result<CanvasTextOutput, CompositeError> {
    let mut inputs = TextInputs::default();
    for (object, _, _) in request.source.text_sources() {
        let pixels = StaticImage::from_premultiplied_rgba(
            2,
            2,
            vec![255, 0, 0, 255, 0, 0, 0, 0, 0, 0, 0, 0, 255, 0, 0, 255],
            Default::default(),
        )
        .unwrap();
        inputs.insert(
            object.id.clone(),
            Ok(Some(Arc::new(TextImage {
                image: Arc::new(pixels),
                local_bounds: Bounds::new(Point::ZERO, Size::new(2., 2.)),
            }))),
        );
    }
    Ok(CanvasTextOutput {
        inputs,
        pixel_work: 4,
    })
}
fn pixel(s: &CanvasSurface, x: usize, y: usize) -> [u8; 4] {
    let p = s.snapshot();
    p.premultiplied_rgba()[(y * p.width() as usize + x) * 4..][..4]
        .try_into()
        .unwrap()
}
#[test]
fn canvas_text_requires_explicit_ink_and_preserves_pixels_on_missing_backend() {
    let mut s = surface();
    let before = s.snapshot();
    assert!(matches!(
        s.draw_commands(&[command()]),
        Err(CompositeError::TextBackendRequired(_))
    ));
    assert!(Arc::ptr_eq(&before, &s.snapshot()));
}
#[test]
fn canvas_text_uses_real_provided_ink_not_its_layout_rectangle() {
    let mut s = surface();
    s.draw_commands_with_text(&[command()], &ImageInputs::default(), ink)
        .unwrap();
    assert_eq!(s.generation(), 2);
    assert_eq!(pixel(&s, 2, 3), [255, 0, 0, 255]);
    assert_eq!(pixel(&s, 3, 3), [0; 4]);
    assert_eq!(pixel(&s, 10, 8), [0; 4]);
}
#[test]
fn canvas_text_state_transform_opacity_and_clip_use_the_shared_pipeline() {
    let mut s = surface();
    s.draw_commands_with_text(
        &[
            CanvasCommand::ClipPath {
                path: Path::from_svg("M 0 0 L 7 0 L 7 32 L 0 32 Z").unwrap(),
                rule: FillRule::NonZero,
            },
            CanvasCommand::Transform(Affine::translation(4., 0.)),
            CanvasCommand::SetOpacity(0.5),
            command(),
        ],
        &ImageInputs::default(),
        ink,
    )
    .unwrap();
    assert_eq!(pixel(&s, 6, 3), [128, 0, 0, 128]);
    assert_eq!(pixel(&s, 7, 4), [0; 4]);
}
#[test]
fn canvas_text_preparation_receives_physical_dpr_and_budget_then_reports_work() {
    let mut c = Canvas::new(16., 16.);
    c.pixel_ratio = 2.;
    let mut s = CanvasSurface::new(c, Default::default()).unwrap();
    let old = s.snapshot();
    assert_eq!(
        s.draw_commands_with_text(&[command()], &ImageInputs::default(), |r| {
            assert_eq!(r.viewport.world_to_screen, Affine::scale(2., 2.));
            assert_eq!(r.viewport.canvas_bounds.size, Size::new(32., 32.));
            assert_eq!(r.retained_bytes, 8192);
            Ok(CanvasTextOutput {
                inputs: TextInputs::default(),
                pixel_work: r.limits.max_surface_writes + 1,
            })
        }),
        Err(CompositeError::WorkLimit)
    );
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
}
#[test]
fn canvas_text_invalid_late_command_admits_nothing_and_never_calls_font_producer() {
    let mut s = surface();
    let old = s.snapshot();
    let called = Cell::new(false);
    assert!(
        s.draw_commands_with_text(
            &[command(), CanvasCommand::Restore],
            &ImageInputs::default(),
            |_| {
                called.set(true);
                Ok(Default::default())
            }
        )
        .is_err()
    );
    assert!(!called.get());
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
    assert_eq!(
        s.draw_commands_with_text(&[command()], &ImageInputs::default(), |_| Err(
            CompositeError::Superseded
        )),
        Err(CompositeError::Superseded)
    );
    assert!(Arc::ptr_eq(&old, &s.snapshot()));
}
#[test]
fn canvas_text_source_strings_and_finite_geometry_have_preclone_limits() {
    let mut s = surface();
    let before = s.snapshot();
    for mut bad in [
        Text::new("x".repeat(65_537), Size::new(10., 10.)),
        Text::new("x", Size::new(10., 10.)),
    ] {
        if bad.content.len() == 1 {
            bad.font_family = "f".repeat(513);
        }
        assert_eq!(
            s.draw_commands(&[CanvasCommand::FillTextBox {
                text: bad,
                position: Point::ZERO,
                color: Color::WHITE
            }]),
            Err(CompositeError::InputLimit)
        );
    }
    let commands = vec![
        CanvasCommand::FillTextBox {
            text: Text::new("a".repeat(60_000), Size::new(10., 10.)),
            position: Point::ZERO,
            color: Color::WHITE
        };
        5
    ];
    assert_eq!(s.draw_commands(&commands), Err(CompositeError::InputLimit));
    assert!(
        s.draw_commands(&[CanvasCommand::FillTextBox {
            text: Text::new("x", Size::new(10., 10.)),
            position: Point::new(f32::NAN, 0.),
            color: Color::WHITE
        }])
        .is_err()
    );
    assert!(Arc::ptr_eq(&before, &s.snapshot()));
}
