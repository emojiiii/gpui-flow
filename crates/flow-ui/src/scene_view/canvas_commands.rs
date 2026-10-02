//! Explicit imperative Canvas commands using the same native font backend.
use super::SceneView;
use crate::display::composite_text::{TextBackendSnapshot, canvas_inputs};
use gpui::Context;
use gpui_flow_composite::{CanvasCommand, ImageInputs};
use gpui_flow_engine::ElementHandle;
use gpui_flow_text::{GpuiTextMeasurer, TextMeasureRequest, TextMeasurer, TextMetrics};

impl SceneView {
    /// Atomic pixel edit. Fonts use the bridge already validated for this App;
    /// images are explicit immutable decoded inputs, never fetched here.
    /// This synchronous bounded API is for explicit host commands, not per-frame
    /// unbounded text layout. Canvas pixels do not enter document Undo history.
    pub fn draw_canvas_commands(
        &mut self,
        target: ElementHandle,
        commands: &[CanvasCommand],
        images: &ImageInputs,
        cx: &mut Context<Self>,
    ) -> Result<(), crate::resources::CanvasEditError> {
        let backend = self.text_rasterizer.as_ref().map(TextBackendSnapshot::new);
        self.edit_canvas(
            target,
            |surface| {
                surface.draw_commands_with_text(commands, images, |r| {
                    canvas_inputs(r, backend.as_ref())
                })?;
                Ok(())
            },
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
}
