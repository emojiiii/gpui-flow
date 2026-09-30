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
#[derive(Debug, Clone, PartialEq)]
pub struct TextMeasureRequest {
    pub content: String,
    pub font_family: String,
    pub font_size: f32,
    pub font_weight: u16,
    /// Absolute logical line-box height, not a glyph's ink bounds.
    pub line_height: f32,
    pub wrap_width: Option<f32>,
}
impl TextMeasureRequest {
    pub fn validate(&self) -> Result<(), String> {
        if self.content.len() > 65_536 || self.font_family.len() > 512 {
            return Err(
                "text measurement allows at most 64KiB UTF-8 content and 512-byte family".into(),
            );
        }
        if self.font_family.trim().is_empty()
            || !self.font_size.is_finite()
            || self.font_size <= 0.
            || !(1..=1000).contains(&self.font_weight)
            || !self.line_height.is_finite()
            || self.line_height <= 0.
            || self.wrap_width.is_some_and(|w| !w.is_finite() || w <= 0.)
        {
            return Err("invalid text measurement input".into());
        }
        Ok(())
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct TextMetrics {
    /// May be zero for empty text. Scene geometry retains a minimum width of 1.
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
    pub lines: usize,
}
impl TextMetrics {
    pub fn validate(&self) -> Result<(), String> {
        if !self.width.is_finite()
            || self.width < 0.
            || !self.height.is_finite()
            || self.height <= 0.
            || !self.line_height.is_finite()
            || self.line_height <= 0.
            || !(1..=65_537).contains(&self.lines)
        {
            return Err("backend returned invalid text metrics".into());
        }
        Ok(())
    }
}
/// A deterministic host service; the layout/model layer does not depend on GPUI.
/// Errors and panics abort the complete candidate before document/history writes.
/// A cache must include all request fields and discard entries on invalidate.
pub trait TextMeasurer: Send + Sync {
    fn measure(&self, request: &TextMeasureRequest) -> Result<TextMetrics, String>;
    fn invalidate(&self) {}
}

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
            let width = width || item.min_width.is_some() || item.max_width.is_some();
            let height = height || item.min_height.is_some() || item.max_height.is_some();
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
        let request = TextMeasureRequest {
            content: text.content.clone(),
            font_family: text.font_family.clone(),
            font_size: text.font_size,
            font_weight: text.font_weight,
            line_height: text.font_size * text.line_height,
            wrap_width: if *sizing == TextSizing::AutoHeight && text.wrap {
                Some(text.size.width)
            } else {
                None
            },
        };
        request.validate().map_err(|e| invalid(id, e))?;
        let metrics = catch_unwind(AssertUnwindSafe(|| backend.measure(&request)))
            .map_err(|_| invalid(id, "text measurement backend panicked"))?
            .map_err(|e| invalid(id, e))?;
        metrics.validate().map_err(|e| invalid(id, e))?;
        let expected_height = request.line_height * metrics.lines as f32;
        if metrics.line_height != request.line_height
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
            size.width = metrics.width.max(1.);
        }
        size.height = metrics.height;
        if size != text.size {
            scene.update_object(id, |object| {
                let Shape::Text(text) = &mut object.shape else {
                    unreachable!()
                };
                text.size = size;
            })?;
        }
    }
    Ok(())
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
