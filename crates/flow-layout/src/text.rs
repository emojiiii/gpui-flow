use crate::{AlignItems, FlowError, FlowLayout, FlowReport, MAX_FLOW_LAYOUT_PASSES, invalid};
use gpui_flow_editor::SceneEditor;
use gpui_flow_engine::{ChangeOrigin, ChangeSet, LayoutProviderId, SceneEngine};
use gpui_flow_scene::{DisplayId, Scene, SceneError, Shape};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

/// Natural measures both physical axes without wrapping. AutoHeight preserves
/// fixed/parent-allocated width; AutoWidth preserves fixed/parent-allocated
/// height. Wrapping is allowed only when measurement does not own the inline
/// axis: horizontal AutoHeight or vertical AutoWidth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSizing {
    Natural,
    AutoHeight,
    AutoWidth,
}
impl TextSizing {
    fn owns_width(self) -> bool {
        self != Self::AutoHeight
    }
    fn owns_height(self) -> bool {
        self != Self::AutoWidth
    }
}
// Compatibility exports: the measurement contract lives in the standalone crate.
pub use gpui_flow_text::{TextMeasureRequest, TextMeasurer, TextMetrics};

pub(super) fn validate(layout: &FlowLayout, scene: &Scene) -> Result<(), FlowError> {
    for (id, sizing) in &layout.texts {
        let node = scene
            .get(id)
            .ok_or_else(|| invalid(id, "measured text does not exist"))?;
        let Shape::Text(text) = &node.object.shape else {
            return Err(invalid(id, "text sizing requires Text"));
        };
        if *sizing == TextSizing::AutoHeight && text.writing_mode.is_vertical() && text.wrap {
            return Err(invalid(
                id,
                "vertical AutoHeight requires wrap=false; measured height cannot also be its wrap axis",
            ));
        }
        if *sizing == TextSizing::AutoWidth && !text.writing_mode.is_vertical() && text.wrap {
            return Err(invalid(
                id,
                "horizontal AutoWidth requires wrap=false; measured width cannot also be its wrap axis",
            ));
        }
        if *sizing == TextSizing::Natural && text.wrap {
            return Err(invalid(id, "natural text sizing requires wrap=false"));
        }
        if let Some(parent) = node.parent.as_ref().and_then(|p| layout.containers.get(p)) {
            let item = layout.items.get(id).cloned().unwrap_or_default();
            if !item.in_flow {
                continue;
            }
            let main = item.grow > 0. || item.basis.is_some();
            let cross = item.align_self.unwrap_or(parent.align_items) == AlignItems::Stretch;
            let (width, height) = if parent.direction.is_row() {
                (main, cross)
            } else {
                (cross, main)
            };
            let width = width
                || item.width_percent.is_some()
                || item.min_width.is_some()
                || item.max_width.is_some();
            let height = height
                || item.height_percent.is_some()
                || item.min_height.is_some()
                || item.max_height.is_some();
            if (sizing.owns_height() && height) || (sizing.owns_width() && width) {
                return Err(invalid(
                    id,
                    "measured text dimension is also allocated by its parent",
                ));
            }
        }
    }
    Ok(())
}
pub(super) fn measure(
    scene: &mut Scene,
    texts: &BTreeMap<DisplayId, TextSizing>,
    backend: Option<&dyn TextMeasurer>,
) -> Result<(), FlowError> {
    let Some(backend) = backend else {
        return Ok(());
    };
    for (id, sizing) in texts {
        let Shape::Text(text) = &scene.get(id).expect("validated text").object.shape else {
            unreachable!("validated text")
        };
        let request = request(text, *sizing);
        request.validate().map_err(|e| invalid(id, e))?;
        let metrics = catch_unwind(AssertUnwindSafe(|| {
            backend.measure_writing_mode(&request, writing_mode(text.writing_mode))
        }))
        .map_err(|_| invalid(id, "text measurement backend panicked"))?
        .map_err(|e| invalid(id, e.to_string()))?;
        metrics.validate().map_err(|e| invalid(id, e))?;
        let expected_height = request.block_height(metrics.lines);
        if metrics.lines <= request.paragraph_breaks()
            || (*sizing == TextSizing::AutoWidth
                && !text.writing_mode.is_vertical()
                && metrics.lines != request.paragraph_breaks() + 1)
            || metrics.line_height != request.line_height
            || !expected_height.is_finite()
            || ((if text.writing_mode.is_vertical() {
                metrics.width
            } else {
                metrics.height
            }) - expected_height)
                .abs()
                > expected_height.max(1.) * 0.00001
        {
            return Err(invalid(
                id,
                "backend metrics disagree with requested line boxes",
            ));
        }
        let mut size = text.size;
        if sizing.owns_width() {
            size.width = metrics.width.max(1.) + text.padding[1] + text.padding[3];
        }
        if sizing.owns_height() {
            size.height = metrics.height + text.padding[0] + text.padding[2];
        }
        let origin = if text.auto_size_align {
            use gpui_flow_scene::display::{TextAlign, TextVerticalAlign};
            let inline_origin = |content: f32, outer: f32, leading: f32| match text.align {
                TextAlign::Center => -content / 2. - leading,
                TextAlign::Start if !text.writing_mode.is_reverse() => 0.,
                TextAlign::End if text.writing_mode.is_reverse() => 0.,
                _ => -outer,
            };
            let cross_origin = |content: f32, outer: f32, leading: f32| match text.vertical_align {
                TextVerticalAlign::Top => 0.,
                TextVerticalAlign::Middle => -content / 2. - leading,
                TextVerticalAlign::Bottom => -outer,
            };
            if text.writing_mode.is_vertical() {
                gpui_flow_scene::Point::new(
                    if sizing.owns_width() {
                        cross_origin(metrics.width, size.width, text.padding[3])
                    } else {
                        0.
                    },
                    if sizing.owns_height() {
                        inline_origin(metrics.height, size.height, text.padding[0])
                    } else {
                        0.
                    },
                )
            } else {
                gpui_flow_scene::Point::new(
                    if sizing.owns_width() {
                        inline_origin(
                            size.width - text.padding[1] - text.padding[3],
                            size.width,
                            text.padding[3],
                        )
                    } else {
                        0.
                    },
                    if sizing.owns_height() {
                        cross_origin(metrics.height, size.height, text.padding[0])
                    } else {
                        0.
                    },
                )
            }
        } else {
            gpui_flow_scene::Point::ZERO
        };
        if size != text.size || origin != text.layout_origin {
            scene.update_object(id, |object| {
                let Shape::Text(text) = &mut object.shape else {
                    unreachable!()
                };
                text.size = size;
                text.layout_origin = origin;
            })?;
        }
    }
    Ok(())
}

fn request(text: &gpui_flow_scene::display::Text, sizing: TextSizing) -> TextMeasureRequest {
    TextMeasureRequest {
        content: text.content.clone(),
        font_family: text.font_family.clone(),
        font_size: text.font_size,
        font_weight: text.font_weight,
        italic: text.italic,
        text_case: match text.text_case {
            gpui_flow_scene::display::TextCase::None => gpui_flow_text::TextCase::None,
            gpui_flow_scene::display::TextCase::Upper => gpui_flow_text::TextCase::Upper,
            gpui_flow_scene::display::TextCase::Lower => gpui_flow_text::TextCase::Lower,
        },
        para_spacing: text.para_spacing,
        letter_spacing: text.letter_spacing,
        word_spacing: text.word_spacing,
        para_indent: text.para_indent,
        line_height: text.font_size * text.line_height,
        wrap_width: if sizing != TextSizing::Natural && text.wrap {
            // Backend wrap_width is an inline advance. Padding stays physical:
            // horizontal uses width - left/right; vertical uses height - top/bottom.
            Some(if text.writing_mode.is_vertical() {
                text.content_bounds().size.height
            } else {
                text.content_bounds().size.width
            })
        } else {
            None
        },
    }
}

pub(super) fn baselines(
    scene: &Scene,
    layout: &FlowLayout,
    containers: &[DisplayId],
    backend: Option<&dyn TextMeasurer>,
) -> Result<BTreeMap<DisplayId, f64>, FlowError> {
    let mut result = BTreeMap::new();
    for id in containers {
        let style = &layout.containers[id];
        for child in &scene.get(id).expect("validated container").children {
            let item = layout.items.get(child).cloned().unwrap_or_default();
            if !item.in_flow || item.align_self.unwrap_or(style.align_items) != AlignItems::Baseline
            {
                continue;
            }
            let object = &scene.get(child).expect("validated child").object;
            let Shape::Text(text) = &object.shape else {
                unreachable!("validated baseline Text")
            };
            let request = request(text, layout.texts[child]);
            request.validate().map_err(|e| invalid(child, e))?;
            let backend = backend.ok_or_else(|| {
                invalid(child, "baseline alignment requires a measurement backend")
            })?;
            let vertical = text.writing_mode.is_vertical();
            let content = text.content_bounds().size;
            let align = match text.align {
                gpui_flow_scene::display::TextAlign::Start => gpui_flow_text::InlineAlign::Start,
                gpui_flow_scene::display::TextAlign::Center => gpui_flow_text::InlineAlign::Center,
                gpui_flow_scene::display::TextAlign::End => gpui_flow_text::InlineAlign::End,
            };
            let baseline = catch_unwind(AssertUnwindSafe(|| {
                backend.first_baseline_writing_mode(
                    &request,
                    writing_mode(text.writing_mode),
                    if vertical {
                        content.height
                    } else {
                        content.width
                    },
                    align,
                )
            }))
            .map_err(|_| invalid(child, "text baseline backend panicked"))?
            .map_err(|e| invalid(child, e.to_string()))?;
            let limit = if vertical {
                content.height
            } else {
                request.line_height
            };
            if !baseline.is_finite() || !(0.0..=limit).contains(&baseline) {
                return Err(invalid(
                    child,
                    if vertical {
                        "backend returned a baseline outside the content box"
                    } else {
                        "backend returned a baseline outside the first line box"
                    },
                ));
            }
            // Horizontal AutoWidth keeps a fixed height. It cannot soft-wrap,
            // so hard paragraphs determine its block height exactly. Match the
            // painter's nonnegative alignment slack, including clipped overflow.
            let offset = if !vertical && layout.texts[child] == TextSizing::AutoWidth {
                let align = match text.vertical_align {
                    gpui_flow_scene::display::TextVerticalAlign::Top => {
                        gpui_flow_text::VerticalAlign::Top
                    }
                    gpui_flow_scene::display::TextVerticalAlign::Middle => {
                        gpui_flow_text::VerticalAlign::Middle
                    }
                    gpui_flow_scene::display::TextVerticalAlign::Bottom => {
                        gpui_flow_text::VerticalAlign::Bottom
                    }
                };
                align.offset(
                    text.content_bounds().size.height,
                    request.block_height(request.paragraph_breaks() + 1),
                )
            } else {
                0.
            };
            // Distances are relative to the geometric box top. Arrangement
            // already removes its layout_origin when setting the transform.
            let baseline = (f64::from(text.padding[0]) + f64::from(offset) + f64::from(baseline))
                * f64::from(object.transform.d);
            if !baseline.is_finite() || baseline > f64::from(f32::MAX) {
                return Err(invalid(child, "text baseline overflow"));
            }
            result.insert(child.clone(), baseline);
        }
    }
    Ok(result)
}

/// Runtime binding of serializable layout input to a real measurement service.
/// Serialize layout() in FlowDocument; the backend and its caches are runtime-only.
#[derive(Clone)]
pub struct MeasuredFlowLayout {
    pub(super) layout: FlowLayout,
    pub(super) measurer: Arc<dyn TextMeasurer>,
}
impl FlowLayout {
    pub fn with_text_measurer(self, measurer: Arc<dyn TextMeasurer>) -> MeasuredFlowLayout {
        MeasuredFlowLayout {
            layout: self,
            measurer,
        }
    }
}
impl MeasuredFlowLayout {
    pub fn layout(&self) -> &FlowLayout {
        &self.layout
    }
    pub fn arranged(&self, scene: &Scene) -> Result<(Scene, FlowReport), FlowError> {
        self.layout
            .arranged_measured(scene, MAX_FLOW_LAYOUT_PASSES, Some(self.measurer.as_ref()))
    }
    pub fn apply_to_scene(&self, scene: &mut Scene) -> Result<FlowReport, FlowError> {
        let (next, report) = self.arranged(scene)?;
        *scene = next;
        Ok(report)
    }
    pub fn apply_to_engine(
        &self,
        engine: &mut SceneEngine,
    ) -> Result<(FlowReport, ChangeSet), FlowError> {
        let prepared = engine.prepare_scene(engine.scene().clone())?;
        let (next, report) = self.arranged(&prepared)?;
        let changes = engine.transaction_with_origin(ChangeOrigin::Layout, |scene| {
            *scene = next;
            Ok(())
        })?;
        Ok((report, changes))
    }
    pub fn transaction_editor(
        &self,
        editor: &mut SceneEditor,
        edit: impl FnOnce(&mut Scene) -> Result<(), SceneError>,
    ) -> Result<FlowReport, FlowError> {
        let mut candidate = editor.scene().clone();
        edit(&mut candidate)?;
        let prepared = editor.engine.prepare_scene(candidate)?;
        let (next, report) = self.arranged(&prepared)?;
        editor.transaction(|scene| {
            *scene = next;
            Ok(())
        })?;
        Ok(report)
    }
    pub fn apply_to_editor(&self, editor: &mut SceneEditor) -> Result<FlowReport, FlowError> {
        self.transaction_editor(editor, |_| Ok(()))
    }
    /// Explicit resource invalidation and an actual atomic reflow. A failed
    /// refresh retains the document and earlier history; errors are not cached.
    pub fn refresh_text_metrics(&self, editor: &mut SceneEditor) -> Result<FlowReport, FlowError> {
        catch_unwind(AssertUnwindSafe(|| self.measurer.invalidate()))
            .map_err(|_| FlowError::TextMeasurement("backend invalidation panicked".into()))?;
        self.apply_to_editor(editor)
    }
    pub fn install_reactive(
        &self,
        engine: &mut SceneEngine,
    ) -> Result<LayoutProviderId, FlowError> {
        self.layout
            .install_reactive_measured(engine, Some(self.measurer.clone()))
    }
}

fn writing_mode(mode: gpui_flow_scene::display::TextWritingMode) -> gpui_flow_text::WritingMode {
    use gpui_flow_scene::display::TextWritingMode as Scene;
    match mode {
        Scene::X => gpui_flow_text::WritingMode::X,
        Scene::XReverse => gpui_flow_text::WritingMode::XReverse,
        Scene::Y => gpui_flow_text::WritingMode::Y,
        Scene::YReverse => gpui_flow_text::WritingMode::YReverse,
    }
}
