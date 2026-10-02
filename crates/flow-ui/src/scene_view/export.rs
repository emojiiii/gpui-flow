//! Explicit canonical-scene export. Never screenshots a window or mutates history.
use super::SceneView;
use crate::{
    display::{
        composite::selected_image_with_limits,
        composite_text::{TextBackendSnapshot, canvas_inputs_with_cancel},
    },
    resources::{CompositeImageObservation, SceneResourceAdapter},
};
use gpui_flow_composite::{
    CompositeError, CompositeLimits, DomainSource, ImageInputs, SceneExport, SceneExportPlan,
    SceneExportRequest, StaticImage,
};
use gpui_flow_engine::SceneEngine;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

impl SceneView {
    /// Export a fixed world region of the canonical document using current,
    /// authorized decoded frames and the installed same-App font bridge.
    ///
    /// This synchronous bounded host command does not load missing resources.
    /// Pending/failed/stale producers reject the entire output. Image animation
    /// uses its currently selected frame. Editor pan/zoom, grid, selection,
    /// transient state styles and other host widgets are intentionally absent.
    /// The caller owns file storage or delivery; no filesystem operation occurs.
    pub fn export_scene(
        &self,
        request: SceneExportRequest,
        limits: CompositeLimits,
    ) -> Result<SceneExport, CompositeError> {
        self.export_scene_cancellable(request, limits, &AtomicBool::new(false))
    }

    /// Cooperative cancellation is checked during preparation/rendering and
    /// before/after codec calls; it cannot preempt a codec's private computation.
    pub fn export_scene_cancellable(
        &self,
        request: SceneExportRequest,
        limits: CompositeLimits,
        cancelled: &AtomicBool,
    ) -> Result<SceneExport, CompositeError> {
        let plan = SceneExportPlan::new(self.editor.scene(), request, limits)?;
        let initial = plan.text_request(&ImageInputs::default(), 0)?;
        let capacity = plan
            .limits()
            .max_surface_bytes
            .saturating_sub(initial.retained_bytes);
        let (images, image_work) = export_images(
            plan.source(),
            &self.editor.engine,
            self.resources.as_ref(),
            plan.limits(),
            capacity,
            initial.limits.max_surface_writes,
            cancelled,
        )?;
        let backend = self.text_rasterizer.as_ref().map(TextBackendSnapshot::new);
        let output = plan.encode_with_text(&images, 0, cancelled, |mut r| {
            r.limits.max_surface_writes = r
                .limits
                .max_surface_writes
                .checked_sub(image_work)
                .ok_or(CompositeError::WorkLimit)?;
            let mut text = canvas_inputs_with_cancel(r, backend.as_ref(), cancelled)?;
            text.pixel_work = text
                .pixel_work
                .checked_add(image_work)
                .ok_or(CompositeError::WorkLimit)?;
            Ok(text)
        })?;
        if backend.as_ref().is_some_and(|b| !b.current()) || cancelled.load(Ordering::Relaxed) {
            return Err(CompositeError::Superseded);
        }
        Ok(output)
    }
}

pub(crate) fn export_images(
    source: &DomainSource,
    engine: &SceneEngine,
    resources: Option<&SceneResourceAdapter>,
    limits: CompositeLimits,
    capacity: u64,
    work_limit: u64,
    cancelled: &AtomicBool,
) -> Result<(ImageInputs, u64), CompositeError> {
    let mut inputs = ImageInputs::default();
    let mut converted: BTreeMap<(gpui::ImageId, usize), Arc<StaticImage>> = BTreeMap::new();
    let mut bytes = 0;
    let mut work = 0u64;
    let mut append = |id: &gpui_flow_scene::DisplayId,
                      observation: Option<CompositeImageObservation>| {
        if cancelled.load(Ordering::Relaxed) {
            return Err(CompositeError::Superseded);
        }
        let observation = observation.ok_or_else(|| CompositeError::ImagePending(id.clone()))?;
        let image = observation.image?;
        if observation.frame_index >= image.frame_count() {
            return Err(CompositeError::InvalidImage);
        }
        let key = (image.id, observation.frame_index);
        let input = if let Some(input) = converted.get(&key) {
            input.clone()
        } else {
            let data = image
                .as_bytes(observation.frame_index)
                .ok_or(CompositeError::InvalidImage)?;
            work = work
                .checked_add(data.len() as u64 / 4)
                .ok_or(CompositeError::WorkLimit)?;
            if work > work_limit {
                return Err(CompositeError::WorkLimit);
            }
            let input = selected_image_with_limits(
                &image,
                observation.frame_index,
                &mut bytes,
                capacity,
                limits,
            )?;
            converted.insert(key, input.clone());
            input
        };
        inputs.insert(id.clone(), Ok(input));
        Ok(())
    };
    for (id, image) in source.images() {
        append(
            id,
            resources.map(|r| r.composite_image(engine, id, &image.source)),
        )?;
    }
    for (id, canvas) in source.canvases() {
        append(
            id,
            resources.map(|r| r.composite_canvas(engine, id, canvas)),
        )?;
    }
    Ok((inputs, work))
}
