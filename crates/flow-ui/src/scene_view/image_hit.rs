//! Filter existing ordered engine candidates; never invent bounds-only targets.
use super::SceneView;
use crate::{
    ImageAlphaHit, ImageHitFallback, image_hit::sample_alpha, resources::SceneResourceAdapter,
};
use gpui_flow_engine::{ElementHandle, EngineError, InstanceState, SceneEngine};
use gpui_flow_scene::{HitTestOptions, HitTestResult, Point, Shape};
use std::collections::BTreeMap;

#[derive(Default)]
pub(crate) struct ImageHitPolicies {
    entries: BTreeMap<ElementHandle, (u64, ImageAlphaHit)>,
}
impl ImageHitPolicies {
    pub(crate) fn get(&self, engine: &SceneEngine, handle: ElementHandle) -> Option<ImageAlphaHit> {
        let (epoch, policy) = self.entries.get(&handle)?;
        let instance = engine.instance(handle).ok()?;
        (instance.state == InstanceState::Attached && instance.attachment_epoch == *epoch)
            .then_some(*policy)
    }
    pub(crate) fn set(
        &mut self,
        engine: &SceneEngine,
        handle: ElementHandle,
        policy: Option<ImageAlphaHit>,
    ) -> bool {
        // Dead settings cannot accumulate across repeated replacement/remount
        // cycles. No bytes, source names, or resource snapshots are retained.
        self.entries.retain(|handle, (epoch, _)| {
            engine.instance(*handle).is_ok_and(|instance| {
                instance.state == InstanceState::Attached && instance.attachment_epoch == *epoch
            })
        });
        let Ok(instance) = engine.instance(handle) else {
            return false;
        };
        if instance.state != InstanceState::Attached
            || !engine
                .object(handle)
                .is_ok_and(|object| matches!(object.shape, Shape::Image(_)))
        {
            return false;
        }
        match policy {
            Some(policy) => {
                self.entries
                    .insert(handle, (instance.attachment_epoch, policy));
            }
            None => {
                self.entries.remove(&handle);
            }
        }
        true
    }
    pub(crate) fn hit_test(
        &self,
        engine: &SceneEngine,
        resources: Option<&SceneResourceAdapter>,
        world: Point,
        options: HitTestOptions,
    ) -> Result<Option<HitTestResult>, EngineError> {
        // SceneView used engine.hit_test before opting in: preserve its complete
        // geometry/visibility/clip/locked/custom semantics and painted ordering.
        Ok(engine
            .hit_test_all(world, options)?
            .into_iter()
            .find(|hit| {
                let Some(handle) = engine.handle(&hit.id) else {
                    return false;
                };
                let Some(policy) = self.get(engine, handle) else {
                    return true;
                };
                let Some(node) = engine.scene().get(&hit.id) else {
                    return false;
                };
                let Shape::Image(image) = &node.object.shape else {
                    return true;
                };
                // Even Bounds/tolerance candidates cannot enlarge an alpha image's
                // rounded silhouette. Ancestor clips were already checked above.
                if !image.contains(hit.local_point) {
                    return false;
                }
                let Some(resources) = resources else {
                    return policy.failed == ImageHitFallback::Geometry;
                };
                // The established observation validates handle, attachment epoch,
                // source revision/generation, authorization, and timeline limits.
                // It shares the actual playback index with native and CPU paint.
                let observation = resources.composite_image(engine, &hit.id, &image.source);
                match observation.image {
                    Ok(frame) => {
                        let size = frame.size(observation.frame_index);
                        frame
                            .as_bytes(observation.frame_index)
                            .and_then(|bytes| {
                                sample_alpha(
                                    image,
                                    hit.local_point,
                                    size.width.0.try_into().ok()?,
                                    size.height.0.try_into().ok()?,
                                    bytes,
                                )
                            })
                            .map_or(policy.failed == ImageHitFallback::Geometry, |alpha| {
                                alpha > policy.threshold
                            })
                    }
                    Err(gpui_flow_composite::CompositeError::ImagePending(_)) => {
                        policy.pending == ImageHitFallback::Geometry
                    }
                    Err(_) => policy.failed == ImageHitFallback::Geometry,
                }
            }))
    }
}

impl SceneView {
    /// Opt an attached Image into source-alpha picking, or pass None to restore
    /// geometric picking. Returns false for a stale/detached/non-Image handle.
    /// The setting expires on unmount, destruction, or document replacement;
    /// reattachment requires explicit opt-in. Source edits keep the setting but
    /// can only sample the new resource generation. No scene or Undo mutation.
    pub fn set_image_alpha_hit(
        &mut self,
        handle: ElementHandle,
        policy: Option<ImageAlphaHit>,
        cx: &mut gpui::Context<Self>,
    ) -> bool {
        let applied = self
            .image_alpha_hits
            .set(&self.editor.engine, handle, policy);
        if applied {
            cx.notify();
        }
        applied
    }
    /// Current attachment's runtime policy. None is the default geometric hit.
    pub fn image_alpha_hit(&self, handle: ElementHandle) -> Option<ImageAlphaHit> {
        self.image_alpha_hits.get(&self.editor.engine, handle)
    }
}
