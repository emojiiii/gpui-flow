//! Explicit imperative Canvas commands using the same native font backend.
use super::SceneView;
use crate::display::composite_text::{TextBackendSnapshot, canvas_font_inputs};
use gpui::Context;
use gpui_flow_composite::{CanvasCommand, ImageInputs};
use gpui_flow_engine::ElementHandle;
use gpui_flow_text::{GpuiTextMeasurer, TextMeasureRequest, TextMeasurer, TextMetrics};
use std::sync::atomic::{AtomicBool, Ordering};

impl SceneView {
    /// Atomic pixel/current-path edit. Path construction persists across calls;
    /// transform, opacity and clipping remain local to each command batch.
    /// Fonts use the bridge already validated for this App;
    /// images are explicit immutable decoded inputs, never fetched here.
    /// This synchronous bounded API is for explicit host commands, not per-frame
    /// unbounded text layout. Pixels and current paths do not enter Undo history.
    pub fn draw_canvas_commands(
        &mut self,
        target: ElementHandle,
        commands: &[CanvasCommand],
        images: &ImageInputs,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::resources::CanvasEditError> {
        self.draw_canvas_commands_with_cancel(target, commands, images, &AtomicBool::new(false), cx)
    }

    /// Also guard font preparation, pixel publication, and resource publication
    /// after upload against cancellation or a superseded font snapshot.
    pub fn draw_canvas_commands_with_cancel(
        &mut self,
        target: ElementHandle,
        commands: &[CanvasCommand],
        images: &ImageInputs,
        cancelled: &AtomicBool,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::resources::CanvasEditError> {
        let backend = self.text_rasterizer.as_ref().map(TextBackendSnapshot::new);
        let owner = cx.text_system().clone();
        let validate = || {
            if cancelled.load(Ordering::Acquire) || backend.as_ref().is_some_and(|b| !b.current()) {
                Err(gpui_flow_composite::CompositeError::Superseded)
            } else {
                Ok(())
            }
        };
        self.edit_canvas_guarded(
            target,
            |surface| {
                surface.draw_commands_with_fonts_guarded(
                    commands,
                    images,
                    cancelled,
                    |r| canvas_font_inputs(r, backend.as_ref(), || owner, cancelled),
                    validate,
                )?;
                Ok(())
            },
            validate,
            cx,
        )
    }

    /// Actual same-App shaping metrics in logical units. This does not include
    /// a canvas command transform, DPR, clipping, ellipsis or synthetic ink bounds.
    /// Measurement needs the App text system, but not the CPU raster bridge.
    pub fn measure_canvas_text(
        &self,
        request: &TextMeasureRequest,
        cx: &Context<Self>,
    ) -> Result<TextMetrics, String> {
        GpuiTextMeasurer::new(cx.text_system().clone()).measure(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::display::composite_text::canvas_inputs;
    use gpui_flow_composite::{CanvasSurface, CompositeError};
    use gpui_flow_scene::{
        Color, FillRule, Point, Size,
        display::{Canvas, Path, Text},
    };
    use std::sync::Arc;
    #[test]
    fn canvas_text_adapter_missing_bridge_is_explicit_and_atomic() {
        let mut surface = CanvasSurface::new(Canvas::new(32., 32.), Default::default()).unwrap();
        let before = surface.snapshot();
        let commands = [CanvasCommand::FillTextBox {
            text: Text::new("real glyphs required", Size::new(20., 20.)),
            position: Point::ZERO,
            color: Color::WHITE,
        }];
        assert!(matches!(
            surface.draw_commands_with_text(&commands, &ImageInputs::default(), |r| canvas_inputs(
                r, None
            )),
            Err(CompositeError::TextBackendRequired(_))
        ));
        assert!(Arc::ptr_eq(&before, &surface.snapshot()));
    }
    #[test]
    fn canvas_text_adapter_does_not_require_fonts_for_vector_or_transparent_text() {
        let mut surface = CanvasSurface::new(Canvas::new(32., 32.), Default::default()).unwrap();
        let commands = [
            CanvasCommand::FillPath {
                path: Path::from_svg("M 1 1 L 5 1 L 5 5 L 1 5 Z").unwrap(),
                paint: Color::WHITE.into(),
                rule: FillRule::NonZero,
            },
            CanvasCommand::FillTextBox {
                text: Text::new("invisible", Size::new(20., 20.)),
                position: Point::ZERO,
                color: Color::rgba(255, 255, 255, 0),
            },
        ];
        assert!(
            surface
                .draw_commands_with_text(&commands, &ImageInputs::default(), |r| canvas_inputs(
                    r, None
                ))
                .unwrap()
        );
    }

    #[test]
    fn canvas_font_adapter_hidden_and_empty_text_still_requires_outline_capability() {
        for content in ["identity", ""] {
            for (position, color) in [
                (Point::ZERO, Color::WHITE),
                (Point::new(-400., -300.), Color::WHITE),
                (Point::ZERO, Color::rgba(255, 255, 255, 0)),
            ] {
                let mut surface =
                    CanvasSurface::new(Canvas::new(32., 32.), Default::default()).unwrap();
                let before = surface.snapshot();
                let commands = [
                    CanvasCommand::MoveTo {
                        to: Point::new(1., 2.),
                    },
                    CanvasCommand::StrokeTextBox {
                        text: Text::new(content, Size::new(20., 20.)),
                        position,
                        color,
                        options: Default::default(),
                    },
                ];
                let error = surface
                    .draw_commands_with_fonts(&commands, &ImageInputs::default(), |r| {
                        canvas_font_inputs(
                            r,
                            None,
                            || panic!("missing capability must precede owner retrieval"),
                            &AtomicBool::new(false),
                        )
                    })
                    .unwrap_err();
                #[cfg(feature = "canvas-stroke-text")]
                assert!(matches!(error, CompositeError::TextBackendRequired(_)));
                #[cfg(not(feature = "canvas-stroke-text"))]
                assert!(
                    matches!(error, CompositeError::TextRasterization { reason, .. } if reason.contains("canvas-stroke-text feature"))
                );
                assert!(Arc::ptr_eq(&before, &surface.snapshot()));
                assert_eq!(surface.current_path_byte_len(), 0);
            }
        }
    }

    #[test]
    fn canvas_font_adapter_preserves_fill_vector_route_and_cancelled_atomicity() {
        let mut surface = CanvasSurface::new(Canvas::new(32., 32.), Default::default()).unwrap();
        let commands = [
            CanvasCommand::FillPath {
                path: Path::from_svg("M 1 1 L 9 1 L 9 9 Z").unwrap(),
                paint: Color::WHITE.into(),
                rule: FillRule::NonZero,
            },
            CanvasCommand::FillTextBox {
                text: Text::new("transparent fill", Size::new(20., 20.)),
                position: Point::ZERO,
                color: Color::rgba(255, 255, 255, 0),
            },
        ];
        assert!(
            surface
                .draw_commands_with_fonts(&commands, &ImageInputs::default(), |r| {
                    let result = canvas_font_inputs(
                        r,
                        None,
                        || panic!("ordinary fills do not request outline owner"),
                        &AtomicBool::new(false),
                    )?;
                    assert_eq!(result.outline_work, 0);
                    Ok(result)
                })
                .unwrap()
        );
        let before = surface.snapshot();
        assert_eq!(
            surface
                .draw_commands_with_fonts(&commands, &ImageInputs::default(), |r| {
                    canvas_font_inputs(
                        r,
                        None,
                        || panic!("cancelled before owner"),
                        &AtomicBool::new(true),
                    )
                })
                .unwrap_err(),
            CompositeError::Superseded
        );
        assert!(Arc::ptr_eq(&before, &surface.snapshot()));
    }

    #[test]
    fn canvas_font_adapter_rejects_empty_content_box_without_preparing_fonts() {
        let mut surface = CanvasSurface::new(Canvas::new(32., 32.), Default::default()).unwrap();
        let before = surface.snapshot();
        let mut text = Text::new("identity", Size::new(20., 20.));
        text.padding = [10.; 4];
        let command = CanvasCommand::StrokeTextBox {
            text,
            position: Point::ZERO,
            color: Color::WHITE,
            options: Default::default(),
        };
        assert!(matches!(
            surface.draw_commands_with_fonts(&[command], &ImageInputs::default(), |_| {
                panic!("invalid content boxes are rejected before font preparation")
            }),
            Err(CompositeError::InvalidGeometry(_))
        ));
        assert!(Arc::ptr_eq(&before, &surface.snapshot()));
    }
}
