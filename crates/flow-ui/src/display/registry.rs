//! A host can override a Custom scene element without changing scene serialization.
use super::RenderObject;
use gpui::{AnyElement, App, Window};
use std::{collections::BTreeMap, sync::Arc};
pub trait DisplayRenderer: Send + Sync + 'static {
    fn render(&self, object: &RenderObject, window: &mut Window, cx: &mut App) -> AnyElement;
}
#[derive(Default, Clone)]
pub struct DisplayRegistry {
    renderers: BTreeMap<String, Arc<dyn DisplayRenderer>>,
}
impl DisplayRegistry {
    pub fn register(&mut self, kind: impl Into<String>, renderer: impl DisplayRenderer) {
        self.renderers.insert(kind.into(), Arc::new(renderer));
    }
    pub fn get(&self, kind: &str) -> Option<Arc<dyn DisplayRenderer>> {
        self.renderers.get(kind).cloned()
    }
    pub fn remove(&mut self, kind: &str) -> bool {
        self.renderers.remove(kind).is_some()
    }
}
