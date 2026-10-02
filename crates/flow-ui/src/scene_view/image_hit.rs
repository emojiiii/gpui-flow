//! Ordered runtime Image candidates use decoded pixels without weakening fallback.
use super::SceneView;
use crate::{
    ImageAlphaHit, ImageHitFallback, image_hit::sample_alpha, resources::SceneResourceAdapter,
};
use gpui_flow_engine::{ElementHandle, EngineError, InstanceState, SceneEngine};
use gpui_flow_scene::{HitTestOptions, HitTestResult, Point, Shape};

#[derive(Clone, Copy)]
enum AlphaSample {
    Loaded(u8),
    Pending,
    Failed,
}
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
    #[cfg(test)]
    pub(crate) fn hit_test(
        &self,
        engine: &SceneEngine,
        resources: Option<&SceneResourceAdapter>,
        world: Point,
        options: HitTestOptions,
    ) -> Result<Option<HitTestResult>, EngineError> {
        self.hit_test_with_geometry(engine, engine, resources, world, options)
    }
    /// Resource identity stays canonical while geometry follows the accepted
    /// presentation snapshot. A snapshot never owns resource tickets or policies.
    #[cfg(test)]
    pub(crate) fn hit_test_with_geometry(
        &self,
        engine: &SceneEngine,
        geometry: &SceneEngine,
        resources: Option<&SceneResourceAdapter>,
        world: Point,
        options: HitTestOptions,
    ) -> Result<Option<HitTestResult>, EngineError> {
        self.hit_test_presented(engine, geometry, resources, world, options, None)
    }
    pub(crate) fn hit_test_presented(
        &self,
        engine: &SceneEngine,
        geometry: &SceneEngine,
        resources: Option<&SceneResourceAdapter>,
        world: Point,
        options: HitTestOptions,
        presentation: Option<&crate::display::composite::RequestKey>,
    ) -> Result<Option<HitTestResult>, EngineError> {
        // Resolve each current resource observation once. Only valid decoded
        // samples may bypass metadata's fit geometry; unavailable resources keep
        // exactly the ordinary engine candidate and Geometry fallback behavior.
        let mut samples = BTreeMap::new();
        let mut decoded_ids = std::collections::BTreeSet::new();
        for handle in self.entries.keys() {
            let Some(policy) = self.get(engine, *handle) else {
                continue;
            };
            let Ok(object) = engine.object(*handle) else {
                continue;
            };
            let Shape::Image(canonical_image) = &object.shape else {
                continue;
            };
            let Some(presented) = geometry.scene().get(&object.id) else {
                continue;
            };
            let Shape::Image(image) = &presented.object.shape else {
                continue;
            };
            let sample = resources.map_or(AlphaSample::Failed, |resources| {
                // This observation validates handle, attachment, source revision,
                // authorization, generation and the shared playback frame index.
                if image.source != canonical_image.source {
                    return AlphaSample::Failed;
                }
                let live = resources.composite_image(engine, &object.id, &canonical_image.source);
                let observation = if let Some(accepted) =
                    presentation.and_then(|key| key.image_sample(&object.id))
                {
                    if !accepted.resource_eq(&live) {
                        return AlphaSample::Failed;
                    }
                    accepted
                } else {
                    &live
                };
                match &observation.image {
                    Ok(frame) => {
                        let size = frame.size(observation.frame_index);
                        let alpha = frame.as_bytes(observation.frame_index).and_then(|bytes| {
                            sample_alpha(
                                image,
                                geometry.world_to_local(&object.id, world).ok()?,
                                size.width.0.try_into().ok()?,
                                size.height.0.try_into().ok()?,
                                bytes,
                            )
                        });
                        alpha.map_or(AlphaSample::Failed, AlphaSample::Loaded)
                    }
                    Err(gpui_flow_composite::CompositeError::ImagePending(_)) => {
                        AlphaSample::Pending
                    }
                    Err(_) => AlphaSample::Failed,
                }
            });
            if matches!(sample, AlphaSample::Loaded(_)) {
                decoded_ids.insert(object.id.clone());
            }
            samples.insert(object.id.clone(), (policy, sample));
        }
        Ok(geometry
            .hit_test_all_with_image_candidates(world, options, &decoded_ids)?
            .into_iter()
            .find(|hit| {
                let Some((policy, sample)) = samples.get(&hit.id) else {
                    return true;
                };
                let Some(node) = geometry.scene().get(&hit.id) else {
                    return false;
                };
                let Shape::Image(image) = &node.object.shape else {
                    return true;
                };
                // Even Bounds/tolerance candidates cannot enlarge an alpha
                // image's rounded silhouette. Ancestor clips remain engine-owned.
                if !image.contains(hit.local_point) {
                    return false;
                }
                match sample {
                    AlphaSample::Loaded(alpha) => *alpha > policy.threshold,
                    AlphaSample::Pending => policy.pending == ImageHitFallback::Geometry,
                    AlphaSample::Failed => policy.failed == ImageHitFallback::Geometry,
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
