use serde::{Deserialize, Serialize};
/// Geometry-free container; bounds come from its children.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {}
impl Group {
    pub fn new() -> Self {
        Self::default()
    }
}
