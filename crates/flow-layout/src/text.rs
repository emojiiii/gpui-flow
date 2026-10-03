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

/// Natural measures an unwrapped width and height. AutoHeight preserves the
/// fixed/parent-allocated width and uses the Text.wrap policy at that width.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TextSizing {
    Natural,
    AutoHeight,
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
            if height || (*sizing == TextSizing::Natural && width) {
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
        let metrics = catch_unwind(AssertUnwindSafe(|| backend.measure(&request)))
            .map_err(|_| invalid(id, "text measurement backend panicked"))?
            .map_err(|e| invalid(id, e))?;
        metrics.validate().map_err(|e| invalid(id, e))?;
        let expected_height = request.block_height(metrics.lines);
        if metrics.lines <= request.paragraph_breaks()
            || metrics.line_height != request.line_height
            || !expected_height.is_finite()
            || (metrics.height - expected_height).abs() > expected_height.max(1.) * 0.00001
        {
            return Err(invalid(
                id,
                "backend metrics disagree with requested line boxes",
            ));
        }
        let mut size = text.size;
        if *sizing == TextSizing::Natural {
            size.width = metrics.width.max(1.) + text.padding[1] + text.padding[3];
        }
        size.height = metrics.height + text.padding[0] + text.padding[2];
        let origin = if text.auto_size_align {
            use gpui_flow_scene::display::{TextAlign, TextVerticalAlign};
            gpui_flow_scene::Point::new(
                if *sizing == TextSizing::Natural {
                    match text.align {
                        TextAlign::Start => 0.,
                        TextAlign::Center => {
                            -(size.width - text.padding[1] - text.padding[3]) / 2. - text.padding[3]
                        }
                        TextAlign::End => -size.width,
                    }
                } else {
                    0.
                },
                match text.vertical_align {
                    TextVerticalAlign::Top => 0.,
                    TextVerticalAlign::Middle => -metrics.height / 2. - text.padding[0],
                    TextVerticalAlign::Bottom => -size.height,
                },
            )
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
        wrap_width: if sizing == TextSizing::AutoHeight && text.wrap {
            Some(text.content_bounds().size.width)
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
            let baseline = catch_unwind(AssertUnwindSafe(|| backend.first_baseline(&request)))
                .map_err(|_| invalid(child, "text baseline backend panicked"))?
                .map_err(|e| invalid(child, e))?;
            if !baseline.is_finite() || !(0.0..=request.line_height).contains(&baseline) {
                return Err(invalid(
                    child,
                    "backend returned a baseline outside the first line box",
                ));
            }
            // Measured Text owns its complete line-box height, so there is no
            // extra vertical-align slack. Padding and positive scale are real.
            let baseline =
                (f64::from(text.padding[0]) + f64::from(baseline)) * f64::from(object.transform.d);
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
