//! Synchronous, pure layout derivation inside the engine's atomic commit.
use crate::{EngineError, SceneEngine};
use gpui_flow_scene::{Scene, Shape};
use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::Arc,
};

pub const MAX_LAYOUT_DERIVATION_PASSES: usize = 8;
/// Engine-scoped ownership of one installed layout provider. Runtime only.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LayoutProviderId {
    pub(crate) engine: u64,
    pub(crate) sequence: u64,
}
/// A deterministic geometry pass. Implementations may read current live handles
/// but receive only immutable engine/candidate references, so they cannot commit
/// recursively. Only affine transforms and Rect/Ellipse/Text/Image/Box/Frame
/// size fields are writable. All other shape/input fields remain unchanged. No I/O or
/// externally visible work belongs here; preparation can run more than once.
pub trait LayoutProvider: Send + Sync + 'static {
    fn derive(&self, engine: &SceneEngine, candidate: &Scene) -> Result<Scene, EngineError>;
    /// Opt into derivation after changes to base/additional stroke properties.
    /// Queried once when installed. Existing providers keep geometry-only
    /// invalidation; fill, opacity, visibility and metadata are unaffected.
    fn uses_stroke_bounds(&self) -> bool {
        false
    }
}
pub(crate) struct InstalledLayout {
    pub id: LayoutProviderId,
    pub provider: Arc<dyn LayoutProvider>,
    pub stroke_bounds: bool,
}
/// Geometry/order always invalidate. Providers using stroke boxes additionally
/// depend on stroke properties, including paint visibility and advanced styles.
pub(crate) fn inputs_changed(old: &Scene, next: &Scene, stroke_bounds: bool) -> bool {
    old.roots() != next.roots()
        || old.len() != next.len()
        || old.nodes().zip(next.nodes()).any(|(a, b)| {
            a.object.id != b.object.id
                || a.parent != b.parent
                || a.children != b.children
                || a.object.shape != b.object.shape
                || a.object.transform != b.object.transform
                || (stroke_bounds
                    && (a.object.style.stroke != b.object.style.stroke
                        || a.object.style.stroke_layers != b.object.style.stroke_layers
                        || a.object.style.fill_rule != b.object.style.fill_rule))
        })
}
fn validate_scope(before: &Scene, after: &Scene) -> Result<(), EngineError> {
    if before.data != after.data || before.roots() != after.roots() || before.len() != after.len() {
        return Err(EngineError::Layout(
            "provider changed document data or topology".into(),
        ));
    }
    for old in before.nodes() {
        let Some(new) = after.get(&old.object.id) else {
            return Err(EngineError::Layout("provider removed an element".into()));
        };
        if old.parent != new.parent || old.children != new.children {
            return Err(EngineError::Layout(
                "provider changed topology or element kind".into(),
            ));
        }
        let mut allowed_shape = old.object.shape.clone();
        match (&mut allowed_shape, &new.object.shape) {
            (Shape::Rect(a), Shape::Rect(b)) => a.size = b.size,
            (Shape::Ellipse(a), Shape::Ellipse(b)) => a.size = b.size,
            (Shape::Text(a), Shape::Text(b)) => a.size = b.size,
            (Shape::Image(a), Shape::Image(b)) => a.size = b.size,
            (Shape::Box(a), Shape::Box(b)) => a.size = b.size,
            (Shape::Frame(a), Shape::Frame(b)) => a.size = b.size,
            _ => {}
        }
        if allowed_shape != new.object.shape {
            return Err(EngineError::Layout(
                "provider changed shape fields other than supported size".into(),
            ));
        }
        let mut stripped = new.object.clone();
        stripped.shape = old.object.shape.clone();
        stripped.transform = old.object.transform;
        if stripped != old.object {
            return Err(EngineError::Layout(
                "provider changed a nongeometry property".into(),
            ));
        }
    }
    after.validate()?;
    Ok(())
}
pub(crate) fn run(
    provider: &dyn LayoutProvider,
    engine: &SceneEngine,
    mut candidate: Scene,
) -> Result<Scene, EngineError> {
    if engine.deriving_layout.replace(true) {
        return Err(EngineError::Layout(
            "recursive layout preparation is not allowed".into(),
        ));
    }
    struct Guard<'a>(&'a std::cell::Cell<bool>);
    impl Drop for Guard<'_> {
        fn drop(&mut self) {
            self.0.set(false);
        }
    }
    let _guard = Guard(&engine.deriving_layout);
    for _ in 0..MAX_LAYOUT_DERIVATION_PASSES {
        let next = catch_unwind(AssertUnwindSafe(|| provider.derive(engine, &candidate)))
            .map_err(|_| EngineError::Layout("provider panicked; candidate discarded".into()))??;
        validate_scope(&candidate, &next)?;
        let normalized = engine.prepare_scene(next.clone())?;
        if normalized != next {
            return Err(EngineError::Layout(
                "provider output disagrees with registered custom geometry".into(),
            ));
        }
        if next == candidate {
            return Ok(next);
        }
        candidate = next;
    }
    Err(EngineError::Layout(format!(
        "provider did not converge within {MAX_LAYOUT_DERIVATION_PASSES} passes"
    )))
}
