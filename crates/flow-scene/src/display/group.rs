use serde::{Deserialize, Serialize};
/// Structural identity is retained without changing Group's geometry rules.
/// Leafer's Pen is a branch of styled Paths; Flow's `display::Pen` stays a leaf.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GroupKind {
    #[default]
    Group,
    Pen,
}
impl GroupKind {
    fn is_group(&self) -> bool {
        *self == Self::Group
    }
}
/// Geometry-free container; bounds come from its children. Existing ordinary
/// Group JSON stays `{}`; the Pen branch explicitly serializes its kind.
#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Group {
    #[serde(default, skip_serializing_if = "GroupKind::is_group")]
    kind: GroupKind,
}
impl Group {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn pen() -> Self {
        Self {
            kind: GroupKind::Pen,
        }
    }
    pub fn kind(&self) -> GroupKind {
        self.kind
    }
    pub fn tag(&self) -> &'static str {
        match self.kind {
            GroupKind::Group => "group",
            GroupKind::Pen => "pen_group",
        }
    }
}
