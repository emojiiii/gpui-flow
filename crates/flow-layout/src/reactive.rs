//! Live-instance bindings for the engine's opt-in precommit layout provider.
use crate::{FlowError, FlowLayout};
use gpui_flow_engine::{
    ElementHandle, EngineError, InstanceState, LayoutProvider, LayoutProviderId, SceneEngine,
};
use gpui_flow_scene::{DisplayId, Scene, Shape};
use std::collections::BTreeMap;
struct Binding {
    handle: ElementHandle,
    kind: String,
}
struct ReactiveFlow {
    layout: FlowLayout,
    bindings: BTreeMap<DisplayId, Binding>,
}
fn kind(shape: &Shape) -> String {
    if let Shape::Custom(custom) = shape {
        custom.kind.clone()
    } else {
        shape.tag().into()
    }
}
impl ReactiveFlow {
    fn active(&self, engine: &SceneEngine, candidate: &Scene, id: &DisplayId) -> bool {
        let Some(binding) = self.bindings.get(id) else {
            return false;
        };
        engine
            .instance(binding.handle)
            .is_ok_and(|info| info.state != InstanceState::Destroyed && info.id == *id)
            && candidate
                .get(id)
                .is_some_and(|node| kind(&node.object.shape) == binding.kind)
    }
}
impl LayoutProvider for ReactiveFlow {
    fn derive(&self, engine: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError> {
        let mut layout = self.layout.clone();
        layout
            .containers
            .retain(|id, _| self.active(engine, candidate, id));
        layout.items.retain(|id, _| {
            self.active(engine, candidate, id)
                && candidate
                    .get(id)
                    .and_then(|node| node.parent.as_ref())
                    .is_some_and(|parent| layout.containers.contains_key(parent))
        });
        layout
            .arranged(candidate)
            .map(|(scene, _)| scene)
            .map_err(|e| EngineError::Layout(e.to_string()))
    }
}
impl FlowLayout {
    /// Install live bindings after an explicit initial layout. Relevant engine
    /// edits then derive positions/sizes before the single commit notification.
    /// This runtime configuration is not serialized or included in Undo snapshots.
    /// Detached bindings pause; destroyed/replaced handles never bind a new ID.
    pub fn install_reactive(
        &self,
        engine: &mut SceneEngine,
    ) -> Result<LayoutProviderId, FlowError> {
        self.validate(engine.scene())?;
        let mut bindings = BTreeMap::new();
        for id in self.containers.keys().chain(self.items.keys()) {
            let handle = engine
                .handle(id)
                .ok_or_else(|| crate::invalid(id, "live instance missing"))?;
            bindings.insert(
                id.clone(),
                Binding {
                    handle,
                    kind: kind(&engine.object(handle)?.shape),
                },
            );
        }
        Ok(engine.install_layout_provider(ReactiveFlow {
            layout: self.clone(),
            bindings,
        })?)
    }
}
