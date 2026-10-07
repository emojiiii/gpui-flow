//! Bounded, project-local history for an image object's applied edits.
//!
//! Version 1 is stored only at `data.flow_creative_image_versions`:
//! `{ "version": 1, "entries": [{ "asset_key": "assets/a.png",
//! "size": { "width": 120, "height": 80 }, "operation": "original" }] }`.
//! The first entry is the original; subsequent unique asset/size snapshots
//! retain their operation and order of most recent use. At most 16 survive.
//! Missing metadata is legacy data, never an implicit write or migration.
//!
//! This module neither copies nor deletes assets. Callers validate referenced
//! assets against the project manifest (including static image media), enforce
//! locks, and apply returned nodes through the normal graph/undo transaction.

use gpui_flow_core::{Node, Point, Size};
use serde_json::{Map, Value, json};

pub const IMAGE_VERSIONS_KEY: &str = "flow_creative_image_versions";
pub const IMAGE_VERSIONS_SCHEMA_VERSION: u64 = 1;
pub const MAX_IMAGE_VERSIONS: usize = 16;
const MAX_VERSION_SIZE: f64 = 16_384.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageVersionOperation {
    Original,
    CropFlip,
    Rotate,
    Adjust,
    BeforeEdit,
}

impl ImageVersionOperation {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Original => "original",
            Self::CropFlip => "crop_flip",
            Self::Rotate => "rotate",
            Self::Adjust => "adjust",
            Self::BeforeEdit => "before_edit",
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Original => "原图",
            Self::CropFlip => "裁剪与翻转",
            Self::Rotate => "旋转",
            Self::Adjust => "调整",
            Self::BeforeEdit => "编辑前",
        }
    }

    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "original" => Ok(Self::Original),
            "crop_flip" => Ok(Self::CropFlip),
            "rotate" => Ok(Self::Rotate),
            "adjust" => Ok(Self::Adjust),
            "before_edit" => Ok(Self::BeforeEdit),
            _ => Err("unsupported image version operation".into()),
        }
    }
}

/// Only an asset reference, its display size, and the operation that made it.
/// Membership in a node's history is checked again by [`restore_version`].
#[derive(Debug, Clone, PartialEq)]
pub struct ImageVersion {
    pub asset_key: String,
    pub size: Size,
    pub operation: ImageVersionOperation,
}

impl ImageVersion {
    fn same_content(&self, other: &Self) -> bool {
        self.asset_key == other.asset_key && self.size == other.size
    }
}

/// Read strictly without changing the node or validating its current geometry.
/// A missing key returns an empty list, including on older image data. Present
/// metadata requires version 1, 1..=16 unique entries, and exactly one original
/// in the first position. All nested objects reject unknown or missing fields.
pub fn entries(node: &Node) -> Result<Vec<ImageVersion>, String> {
    require_image(node)?;
    let Some(value) = node.data.get(IMAGE_VERSIONS_KEY) else {
        return Ok(Vec::new());
    };
    let history = exact_object(value, &["version", "entries"], "image versions")?;
    if history["version"].as_u64() != Some(IMAGE_VERSIONS_SCHEMA_VERSION) {
        return Err("image versions require integer schema version 1".into());
    }
    let values = history["entries"]
        .as_array()
        .filter(|values| !values.is_empty() && values.len() <= MAX_IMAGE_VERSIONS)
        .ok_or_else(|| "image versions require between 1 and 16 entries".to_string())?;
    let mut versions: Vec<ImageVersion> = Vec::with_capacity(values.len());
    for value in values {
        let entry = exact_object(value, &["asset_key", "size", "operation"], "image version")?;
        let asset_key = entry["asset_key"]
            .as_str()
            .ok_or_else(|| "image version asset key must be a string".to_string())?;
        crate::project::validate_asset_key(asset_key).map_err(|error| error.to_string())?;
        let size = exact_object(&entry["size"], &["width", "height"], "image version size")?;
        let width = dimension(&size["width"])?;
        let height = dimension(&size["height"])?;
        let operation = entry["operation"]
            .as_str()
            .ok_or_else(|| "image version operation must be a string".to_string())?;
        let version = ImageVersion {
            asset_key: asset_key.into(),
            size: Size::new(width, height),
            operation: ImageVersionOperation::parse(operation)?,
        };
        if (version.operation == ImageVersionOperation::Original) != versions.is_empty() {
            return Err("image versions require a single original as the first entry".into());
        }
        if versions.iter().any(|entry| entry.same_content(&version)) {
            return Err("image versions cannot repeat an asset and display size".into());
        }
        versions.push(version);
    }
    Ok(versions)
}

/// Record an applied edit on a copy of `after`, retaining all its host metadata.
/// The nodes must identify the same image. Only CropFlip, Rotate, and Adjust
/// are edit operations; Original and BeforeEdit are assigned by this helper.
/// `before` owns the history, so an unrelated list in `after` cannot add entries.
/// A restored branch keeps its other versions. An unrecorded replacement or
/// display resize is captured as BeforeEdit before the new result is recorded.
pub fn record_edit(
    before: &Node,
    after: &Node,
    operation: ImageVersionOperation,
) -> Result<Node, String> {
    if before.id != after.id {
        return Err("image versions can only record edits of the same node".into());
    }
    if !matches!(
        operation,
        ImageVersionOperation::CropFlip
            | ImageVersionOperation::Rotate
            | ImageVersionOperation::Adjust
    ) {
        return Err("image version edit operation must be crop/flip, rotate, or adjust".into());
    }
    let mut prior = snapshot(before, ImageVersionOperation::BeforeEdit)?;
    let next = snapshot(after, operation)?;
    let mut versions = entries(before)?;
    // Malformed destination metadata must not be silently overwritten either.
    entries(after)?;
    if versions.is_empty() {
        prior.operation = ImageVersionOperation::Original;
        versions.push(prior);
    } else if !versions.iter().any(|entry| entry.same_content(&prior)) {
        versions.push(prior);
    }
    match versions.iter().position(|entry| entry.same_content(&next)) {
        Some(0) => {} // The original always stays first, even when reused.
        Some(index) => {
            // Reuse its provenance while marking the snapshot recently used.
            let existing = versions.remove(index);
            versions.push(existing);
        }
        None => versions.push(next),
    }
    let excess = versions.len().saturating_sub(MAX_IMAGE_VERSIONS);
    versions.drain(1..1 + excess);
    let mut data = image_data(after)?.clone();
    data.insert(IMAGE_VERSIONS_KEY.into(), history_value(&versions));
    let mut updated = after.clone();
    updated.data = Value::Object(data);
    Ok(updated)
}

/// Restore an existing entry without appending or rewriting the history list.
/// The current center, caption, ports, locks, and all other host metadata stay
/// current. The complete supplied entry must already belong to this node;
/// another node's entry cannot be used to insert or import a version.
pub fn restore_version(current: &Node, version: &ImageVersion) -> Result<Node, String> {
    let versions = entries(current)?;
    if !versions.contains(version) {
        return Err("image version does not belong to the current node's history".into());
    }
    validate_size(current.size)?;
    if !current.position.is_finite() {
        return Err("image version restore requires a finite current position".into());
    }
    // Widen before arithmetic, rather than round a center and subtract from it.
    let position = Point::new(
        (f64::from(current.position.x)
            + (f64::from(current.size.width) - f64::from(version.size.width)) * 0.5) as f32,
        (f64::from(current.position.y)
            + (f64::from(current.size.height) - f64::from(version.size.height)) * 0.5)
            as f32,
    );
    if !position.is_finite() {
        return Err("image version restore position is outside the supported range".into());
    }
    let mut data = image_data(current)?.clone();
    data.insert("asset".into(), Value::String(version.asset_key.clone()));
    let mut restored = current.clone();
    restored.data = Value::Object(data);
    restored.position = position;
    restored.size = version.size;
    Ok(restored)
}

fn require_image(node: &Node) -> Result<(), String> {
    if node.kind != "image" {
        return Err("image versions are only supported for image objects".into());
    }
    Ok(())
}

fn image_data(node: &Node) -> Result<&Map<String, Value>, String> {
    require_image(node)?;
    node.data
        .as_object()
        .ok_or_else(|| "image data must be an object".into())
}

fn snapshot(node: &Node, operation: ImageVersionOperation) -> Result<ImageVersion, String> {
    let asset_key = image_data(node)?
        .get("asset")
        .and_then(Value::as_str)
        .ok_or_else(|| "image version requires a project-relative asset key".to_string())?;
    crate::project::validate_asset_key(asset_key).map_err(|error| error.to_string())?;
    validate_size(node.size)?;
    Ok(ImageVersion {
        asset_key: asset_key.into(),
        size: node.size,
        operation,
    })
}

fn validate_size(size: Size) -> Result<(), String> {
    if !size.is_positive()
        || f64::from(size.width) > MAX_VERSION_SIZE
        || f64::from(size.height) > MAX_VERSION_SIZE
    {
        return Err("image version dimensions must be finite, positive, and at most 16384".into());
    }
    Ok(())
}

fn dimension(value: &Value) -> Result<f32, String> {
    let number = value
        .as_f64()
        .filter(|number| number.is_finite() && *number > 0.0 && *number <= MAX_VERSION_SIZE)
        .ok_or_else(|| {
            "image version dimensions must be finite, positive numbers at most 16384".to_string()
        })?;
    let dimension = number as f32;
    if dimension <= 0.0 {
        return Err("image version dimension is too small to represent".into());
    }
    Ok(dimension)
}

fn exact_object<'a>(
    value: &'a Value,
    keys: &[&str],
    name: &str,
) -> Result<&'a Map<String, Value>, String> {
    value
        .as_object()
        .filter(|object| {
            object.len() == keys.len() && keys.iter().all(|key| object.contains_key(*key))
        })
        .ok_or_else(|| format!("{name} must be an object with exactly {}", keys.join(", ")))
}

fn history_value(versions: &[ImageVersion]) -> Value {
    let entries: Vec<Value> = versions
        .iter()
        .map(|entry| {
            json!({
                "asset_key": entry.asset_key,
                "size": { "width": entry.size.width, "height": entry.size.height },
                "operation": entry.operation.as_str(),
            })
        })
        .collect();
    json!({ "version": IMAGE_VERSIONS_SCHEMA_VERSION, "entries": entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_core::{DataType, Port};

    fn image(asset: &str, size: Size) -> Node {
        Node::new("image-1", "Host title", Point::new(20.0, 30.0))
            .with_kind("image")
            .with_size(size)
            .with_data(json!({ "asset": asset, "caption": "Host caption" }))
    }

    fn changed(node: &Node, asset: &str, size: Size) -> Node {
        let mut next = node.clone();
        next.data["asset"] = json!(asset);
        next.size = size;
        next
    }

    fn edited(node: &Node, asset: &str, size: Size) -> Node {
        record_edit(
            node,
            &changed(node, asset, size),
            ImageVersionOperation::Adjust,
        )
        .unwrap()
    }

    fn original_value() -> Value {
        json!({
            "version": 1,
            "entries": [{
                "asset_key": "assets/original.png",
                "size": { "width": 120, "height": 80 },
                "operation": "original",
            }],
        })
    }

    #[test]
    fn legacy_reads_are_empty_without_migration_or_current_geometry_limits() {
        let mut node = image("assets/original.png", Size::new(20_000.0, 30_000.0));
        node.data["host"] = json!({ "keep": [1, true, "opaque"] });
        let before = node.clone();
        assert!(entries(&node).unwrap().is_empty());
        assert_eq!(node, before);
        node.data = Value::Null;
        assert!(entries(&node).unwrap().is_empty());
        assert!(node.data.is_null());
        node.data = json!({ "asset": "assets/original.png" });
        node.data[IMAGE_VERSIONS_KEY] = original_value();
        assert_eq!(entries(&node).unwrap().len(), 1);
        assert!(record_edit(&node, &node, ImageVersionOperation::Adjust).is_err());
    }

    #[test]
    fn malformed_shapes_assets_dimensions_and_counts_are_rejected_without_changes() {
        let mut invalid = vec![Value::Null, json!([]), json!({}), json!({ "version": 1 })];
        for version in [json!(0), json!(2), json!(1.0), json!("1"), Value::Null] {
            let mut value = original_value();
            value["version"] = version;
            invalid.push(value);
        }
        for list in [
            json!([]),
            json!({}),
            json!(vec![original_value()["entries"][0].clone(); 17]),
        ] {
            let mut value = original_value();
            value["entries"] = list;
            invalid.push(value);
        }
        for key in [
            "https://example.com/image.png",
            "/tmp/image.png",
            "assets/../a.png",
            "assets/a.svg",
        ] {
            let mut value = original_value();
            value["entries"][0]["asset_key"] = json!(key);
            invalid.push(value);
        }
        for width in [
            json!(0),
            json!(-1),
            json!(16_384.000_1),
            json!(1e-100),
            json!("120"),
            Value::Null,
        ] {
            let mut value = original_value();
            value["entries"][0]["size"]["width"] = width;
            invalid.push(value);
        }
        for operation in [
            json!("unknown"),
            json!("adjust"),
            json!({ "original": null }),
        ] {
            let mut value = original_value();
            value["entries"][0]["operation"] = operation;
            invalid.push(value);
        }
        let mut value = original_value();
        value["extra"] = json!(true);
        invalid.push(value);
        let mut value = original_value();
        value["entries"][0]["timestamp"] = json!(1);
        invalid.push(value);
        let mut value = original_value();
        value["entries"][0]["size"]["unit"] = json!("px");
        invalid.push(value);
        let mut value = original_value();
        value["entries"][0].as_object_mut().unwrap().remove("size");
        invalid.push(value);
        let mut value = original_value();
        value["entries"].as_array_mut().unwrap().push(json!({
            "asset_key": "assets/second.png", "size": { "width": 10, "height": 10 },
            "operation": "original",
        }));
        invalid.push(value);
        let mut value = original_value();
        let mut duplicate = value["entries"][0].clone();
        duplicate["operation"] = json!("adjust");
        value["entries"].as_array_mut().unwrap().push(duplicate);
        invalid.push(value);
        let valid = image("assets/original.png", Size::new(120.0, 80.0));
        for value in invalid {
            let mut node = valid.clone();
            node.data[IMAGE_VERSIONS_KEY] = value;
            let untouched = node.clone();
            assert!(entries(&node).is_err(), "accepted {}", node.data);
            assert!(record_edit(&node, &valid, ImageVersionOperation::Rotate).is_err());
            assert!(record_edit(&valid, &node, ImageVersionOperation::Rotate).is_err());
            assert_eq!(node, untouched);
        }
        for size in [
            Size::new(f32::NAN, 10.0),
            Size::new(10.0, f32::INFINITY),
            Size::new(0.0, 10.0),
        ] {
            assert!(
                record_edit(
                    &valid,
                    &changed(&valid, "assets/edit.png", size),
                    ImageVersionOperation::Adjust
                )
                .is_err()
            );
        }
    }

    #[test]
    fn record_clones_destination_and_preserves_host_metadata_and_inputs() {
        let before = image("assets/original.jpg", Size::new(120.0, 80.0));
        let mut after = changed(&before, "assets/crop.png", Size::new(60.0, 40.0));
        after.title = "Current host title".into();
        after.position = Point::new(400.0, 500.0);
        after.locked = true; // Enforcement belongs to the caller.
        after
            .ports
            .push(Port::output("host", "Host port", DataType::Object));
        after.data["caption"] = json!("Current caption");
        after.data["host"] = json!({ "opaque": [1, null, false] });
        let before_copy = before.clone();
        let after_copy = after.clone();
        let recorded = record_edit(&before, &after, ImageVersionOperation::CropFlip).unwrap();
        let versions = entries(&recorded).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(
            versions[0],
            ImageVersion {
                asset_key: "assets/original.jpg".into(),
                size: before.size,
                operation: ImageVersionOperation::Original
            }
        );
        assert_eq!(versions[1].operation, ImageVersionOperation::CropFlip);
        assert_eq!(recorded.data[IMAGE_VERSIONS_KEY]["version"], json!(1));
        let mut without_history = recorded.clone();
        without_history
            .data
            .as_object_mut()
            .unwrap()
            .remove(IMAGE_VERSIONS_KEY);
        assert_eq!(without_history, after);
        assert_eq!(before, before_copy);
        assert_eq!(after, after_copy);
        let mut injected_after = after.clone();
        injected_after.data[IMAGE_VERSIONS_KEY] = original_value();
        assert_eq!(
            record_edit(&before, &injected_after, ImageVersionOperation::CropFlip).unwrap(),
            recorded
        );
        let roundtrip: Node =
            serde_json::from_str(&serde_json::to_string(&recorded).unwrap()).unwrap();
        assert_eq!(entries(&roundtrip).unwrap(), versions);
    }

    #[test]
    fn bounded_history_preserves_original_and_latest_unique_versions() {
        let size = Size::new(16_384.0, 0.01);
        let mut current = image("assets/v0.png", size);
        for index in 1..=30 {
            current = edited(&current, &format!("assets/v{index}.png"), size);
        }
        let versions = entries(&current).unwrap();
        assert_eq!(versions.len(), MAX_IMAGE_VERSIONS);
        assert_eq!(versions[0].asset_key, "assets/v0.png");
        assert_eq!(versions[0].operation, ImageVersionOperation::Original);
        assert_eq!(versions[1].asset_key, "assets/v16.png");
        assert_eq!(versions[15].asset_key, "assets/v30.png");
        current = edited(&current, "assets/v20.png", size);
        assert_eq!(
            entries(&current).unwrap().last().unwrap().asset_key,
            "assets/v20.png"
        );
        for _ in 0..20 {
            current = edited(&current, "assets/v20.png", size);
        }
        assert_eq!(entries(&current).unwrap().len(), MAX_IMAGE_VERSIONS);
        current = edited(&current, "assets/v0.png", size);
        assert_eq!(entries(&current).unwrap()[0], versions[0]);
        let original = image("assets/only.png", size);
        assert_eq!(
            entries(&edited(&original, "assets/only.png", size))
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn restoring_then_editing_keeps_branches_and_captures_unrecorded_display_changes() {
        let original = image("assets/original.png", Size::new(120.0, 80.0));
        let first = edited(&original, "assets/first.png", Size::new(100.0, 70.0));
        let second = edited(&first, "assets/second.png", Size::new(80.0, 60.0));
        let original_version = entries(&second).unwrap()[0].clone();
        let restored = restore_version(&second, &original_version).unwrap();
        let branch = edited(&restored, "assets/branch.png", Size::new(90.0, 65.0));
        let versions = entries(&branch).unwrap();
        assert_eq!(
            versions
                .iter()
                .map(|entry| entry.asset_key.as_str())
                .collect::<Vec<_>>(),
            [
                "assets/original.png",
                "assets/first.png",
                "assets/second.png",
                "assets/branch.png"
            ]
        );
        let resized = changed(&branch, "assets/branch.png", Size::new(45.0, 32.5));
        let next = edited(&resized, "assets/next.png", Size::new(50.0, 40.0));
        let versions = entries(&next).unwrap();
        assert_eq!(versions.len(), 6);
        assert_eq!(versions[4].operation, ImageVersionOperation::BeforeEdit);
        assert_eq!(versions[4].size, resized.size);
        assert_eq!(versions[5].asset_key, "assets/next.png");
        let replaced = changed(&next, "assets/external.png", next.size);
        let edited_replacement = edited(&replaced, "assets/replacement-edit.png", next.size);
        assert_eq!(
            entries(&edited_replacement).unwrap()[6].operation,
            ImageVersionOperation::BeforeEdit
        );
    }

    #[test]
    fn restore_preserves_current_center_metadata_and_history_and_is_reversible() {
        let original = image("assets/original.png", Size::new(120.0, 80.0));
        let mut current = edited(&original, "assets/edited.png", Size::new(60.0, 40.0));
        current.position = Point::new(1000.0, 2000.0);
        current.data["caption"] = json!("Latest caption");
        current.data["host"] = json!({ "current": true });
        let saved = current.clone();
        let versions = entries(&current).unwrap();
        let restored = restore_version(&current, &versions[0]).unwrap();
        assert_eq!(restored.bounds().center(), current.bounds().center());
        assert_eq!(restored.size, original.size);
        let mut expected = current.clone();
        expected.data["asset"] = original.data["asset"].clone();
        expected.size = original.size;
        expected.position = Point::new(970.0, 1980.0);
        assert_eq!(restored, expected);
        assert_eq!(restore_version(&restored, &versions[1]).unwrap(), saved);
        let mut forged = versions[0].clone();
        forged.operation = ImageVersionOperation::Adjust;
        assert!(restore_version(&current, &forged).is_err());
        let foreign = edited(
            &image("assets/foreign.png", original.size),
            "assets/foreign-edit.png",
            original.size,
        );
        assert!(restore_version(&current, &entries(&foreign).unwrap()[0]).is_err());
        assert!(restore_version(&original, &versions[0]).is_err());
        assert_eq!(current, saved);
    }

    #[test]
    fn identity_kind_reserved_operations_and_invalid_restore_geometry_are_rejected() {
        let original = image("assets/original.png", Size::new(120.0, 80.0));
        let mut other = original.clone();
        other.id = "different-node".into();
        assert!(record_edit(&original, &other, ImageVersionOperation::Adjust).is_err());
        for kind in ["generate_image", "video", "mask", "default"] {
            other = original.clone();
            other.kind = kind.into();
            assert!(entries(&other).is_err());
            assert!(record_edit(&original, &other, ImageVersionOperation::Adjust).is_err());
            assert!(record_edit(&other, &original, ImageVersionOperation::Adjust).is_err());
        }
        for operation in [
            ImageVersionOperation::Original,
            ImageVersionOperation::BeforeEdit,
        ] {
            assert!(record_edit(&original, &original, operation).is_err());
        }
        let current = edited(&original, "assets/edited.png", original.size);
        let version = entries(&current).unwrap()[0].clone();
        let mut invalid = current.clone();
        invalid.position.x = f32::INFINITY;
        assert!(restore_version(&invalid, &version).is_err());
        invalid = current.clone();
        invalid.size = Size::new(16_385.0, 80.0);
        assert!(restore_version(&invalid, &version).is_err());
        invalid = current.clone();
        invalid.kind = "generate_image".into();
        assert!(restore_version(&invalid, &version).is_err());
        assert_eq!(entries(&current).unwrap()[0], version);
    }
}
