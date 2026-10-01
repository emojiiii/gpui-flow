use crate::ElementHandle;
use gpui_flow_scene::{Bounds, DisplayId};
use serde_json::Value;
use std::collections::BTreeMap;
/// Categories describe derived work, independently of edit authorization.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct DirtyFlags(pub(crate) u16);
impl DirtyFlags {
    pub const NONE: Self = Self(0);
    pub const DATA: Self = Self(1);
    pub const TRANSFORM: Self = Self(2);
    pub const GEOMETRY: Self = Self(4);
    pub const PAINT: Self = Self(8);
    pub const OPACITY: Self = Self(16);
    pub const HIT: Self = Self(32);
    pub const ORDER: Self = Self(64);
    pub const STRUCTURE: Self = Self(128);
    pub const ALL: Self = Self(255);
    pub fn contains(self, other: Self) -> bool {
        self.0 & other.0 == other.0
    }
    pub fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }
    pub fn is_empty(self) -> bool {
        self.0 == 0
    }
}
impl std::ops::BitOr for DirtyFlags {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self {
        Self(self.0 | rhs.0)
    }
}
impl std::ops::BitOrAssign for DirtyFlags {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}
#[derive(Debug, Clone, PartialEq)]
pub struct PropertyChange {
    pub id: Option<DisplayId>,
    pub property: String,
    pub old: Value,
    pub new: Value,
    /// Presence flags distinguish an absent custom input from explicit JSON null.
    pub old_present: bool,
    pub new_present: bool,
    pub dirty: DirtyFlags,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureKind {
    Added,
    Removed,
    Reparented,
    Reordered,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructureChange {
    pub id: DisplayId,
    pub kind: StructureKind,
    pub old_parent: Option<DisplayId>,
    pub new_parent: Option<DisplayId>,
    pub old_index: Option<usize>,
    pub new_index: Option<usize>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleKind {
    Created,
    Mounted,
    Unmounted,
    Detached,
    Reparented,
    Destroyed,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleEvent {
    pub handle: ElementHandle,
    pub id: DisplayId,
    pub kind: LifecycleKind,
    pub old_parent: Option<DisplayId>,
    pub new_parent: Option<DisplayId>,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum ChangeOrigin {
    #[default]
    Edit,
    UndoRedo,
    Import,
    Animation,
    Layout,
    Lifecycle,
}
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct LayoutStats {
    pub visited: usize,
    pub world_recomputed: usize,
    pub bounds_recomputed: usize,
    pub paint_invalidated: usize,
}
#[derive(Debug, Default, Clone, PartialEq)]
pub struct ChangeSet {
    pub before_revision: u64,
    pub revision: u64,
    pub origin: ChangeOrigin,
    pub properties: Vec<PropertyChange>,
    pub structure: Vec<StructureChange>,
    pub lifecycle: Vec<LifecycleEvent>,
    pub dirty: BTreeMap<DisplayId, DirtyFlags>,
    /// Conservative world-space damage. CPU preparation only; a backend must
    /// explicitly consume this before claiming partial painting.
    pub damage: Vec<Bounds>,
    pub layout: LayoutStats,
}
impl ChangeSet {
    pub fn changed(&self) -> bool {
        self.before_revision != self.revision
    }
    pub fn damage_bounds(&self) -> Option<Bounds> {
        self.damage.iter().copied().reduce(|a, b| a.union(b))
    }
}

use crate::registry::property_dirty;
use crate::{ElementRegistry, EngineError};
use gpui_flow_scene::Scene;
use std::collections::BTreeSet;
fn position(scene: &Scene, id: &DisplayId) -> (Option<DisplayId>, Option<usize>) {
    if let Some(n) = scene.get(id) {
        let siblings = n
            .parent
            .as_ref()
            .map_or(scene.roots(), |p| &scene.get(p).expect("parent").children);
        (n.parent.clone(), siblings.iter().position(|i| i == id))
    } else {
        (None, None)
    }
}
pub(crate) fn diff(
    old: &Scene,
    new: &Scene,
    registry: &ElementRegistry,
    before_revision: u64,
    revision: u64,
    origin: ChangeOrigin,
) -> Result<ChangeSet, EngineError> {
    let mut change = ChangeSet {
        before_revision,
        revision,
        origin,
        ..Default::default()
    };
    let ids: BTreeSet<_> = old
        .nodes()
        .chain(new.nodes())
        .map(|n| n.object.id.clone())
        .collect();
    for id in ids {
        let (a, b) = (old.get(&id), new.get(&id));
        let (old_parent, old_index) = position(old, &id);
        let (new_parent, new_index) = position(new, &id);
        let kind = match (a, b) {
            (None, Some(_)) => Some(StructureKind::Added),
            (Some(_), None) => Some(StructureKind::Removed),
            (Some(_), Some(_)) if old_parent != new_parent => Some(StructureKind::Reparented),
            (Some(_), Some(_)) if old_index != new_index => Some(StructureKind::Reordered),
            _ => None,
        };
        if let Some(kind) = kind {
            change.structure.push(StructureChange {
                id: id.clone(),
                kind,
                old_parent,
                new_parent,
                old_index,
                new_index,
            });
            change.dirty.insert(
                id.clone(),
                if matches!(kind, StructureKind::Reordered) {
                    DirtyFlags::ORDER | DirtyFlags::PAINT
                } else {
                    DirtyFlags::ALL
                },
            );
        }
        if let (Some(a), Some(b)) = (a, b) {
            let a = serde_json::to_value(&a.object)
                .map_err(|e| EngineError::Property(e.to_string()))?;
            let b = serde_json::to_value(&b.object)
                .map_err(|e| EngineError::Property(e.to_string()))?;
            for (key, value) in a.as_object().expect("object") {
                if key == "id" {
                    continue;
                }
                if key == "style" {
                    for (field, old_value) in value.as_object().expect("style") {
                        let name = format!("style.{field}");
                        record_property(
                            &mut change,
                            &id,
                            &name,
                            old_value,
                            &b[key][field],
                            property_dirty(&name),
                        );
                    }
                } else if key == "shape" && a[key]["type"] == "text" && b[key]["type"] == "text" {
                    record_property(
                        &mut change,
                        &id,
                        "text.content",
                        &a[key]["geometry"]["content"],
                        &b[key]["geometry"]["content"],
                        property_dirty("text.content"),
                    );
                    let mut old_shape = a[key].clone();
                    let mut new_shape = b[key].clone();
                    old_shape["geometry"]["content"] = Value::Null;
                    new_shape["geometry"]["content"] = Value::Null;
                    if old_shape != new_shape {
                        record_property(
                            &mut change,
                            &id,
                            "shape",
                            &a[key],
                            &b[key],
                            property_dirty("shape"),
                        );
                    }
                } else if key == "shape"
                    && a[key]["type"] == "custom"
                    && b[key]["type"] == "custom"
                    && a[key]["geometry"]["kind"] == b[key]["geometry"]["kind"]
                {
                    let ag = &a[key]["geometry"];
                    let bg = &b[key]["geometry"];
                    for field in ["bounds", "contours"] {
                        record_property(
                            &mut change,
                            &id,
                            &format!("shape.{field}"),
                            &ag[field],
                            &bg[field],
                            DirtyFlags::GEOMETRY | DirtyFlags::PAINT | DirtyFlags::HIT,
                        );
                    }
                    if let (Some(am), Some(bm)) = (ag["data"].as_object(), bg["data"].as_object()) {
                        let keys: BTreeSet<_> = am.keys().chain(bm.keys()).collect();
                        let definition = ag["kind"].as_str().and_then(|k| registry.get(k));
                        for name in keys {
                            let dirty = definition
                                .and_then(|d| d.schema().descriptor(name).map(|p| p.dirty))
                                .unwrap_or(DirtyFlags::DATA);
                            record_optional_property(
                                &mut change,
                                &id,
                                &format!("custom.{name}"),
                                am.get(name),
                                bm.get(name),
                                dirty,
                            );
                        }
                    } else {
                        record_property(
                            &mut change,
                            &id,
                            "custom.data",
                            &ag["data"],
                            &bg["data"],
                            DirtyFlags::DATA,
                        );
                    }
                } else {
                    record_property(&mut change, &id, key, value, &b[key], property_dirty(key));
                }
            }
        }
    }
    if old.data != new.data {
        change.properties.push(PropertyChange {
            id: None,
            property: "data".into(),
            old: old.data.clone(),
            new: new.data.clone(),
            old_present: true,
            new_present: true,
            dirty: DirtyFlags::DATA,
        });
    }
    // Aggregated bounds must be refreshed after child insertion/removal/reparent.
    for structural in &change.structure {
        for parent in [&structural.old_parent, &structural.new_parent]
            .into_iter()
            .flatten()
        {
            *change.dirty.entry(parent.clone()).or_default() |= DirtyFlags::STRUCTURE;
        }
    }
    Ok(change)
}
fn record_property(
    change: &mut ChangeSet,
    id: &DisplayId,
    name: &str,
    old: &Value,
    new: &Value,
    dirty: DirtyFlags,
) {
    record_optional_property(change, id, name, Some(old), Some(new), dirty)
}
fn record_optional_property(
    change: &mut ChangeSet,
    id: &DisplayId,
    name: &str,
    old: Option<&Value>,
    new: Option<&Value>,
    dirty: DirtyFlags,
) {
    if old != new {
        change.properties.push(PropertyChange {
            id: Some(id.clone()),
            property: name.into(),
            old: old.cloned().unwrap_or(Value::Null),
            new: new.cloned().unwrap_or(Value::Null),
            old_present: old.is_some(),
            new_present: new.is_some(),
            dirty,
        });
        *change.dirty.entry(id.clone()).or_default() |= dirty;
    }
}
