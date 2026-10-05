//! Project-owned custom values and the deliberately small local pass-through host.
use crate::custom_nodes::*;
use crate::{CreativeError, ProjectAsset};
use gpui_flow_core::Node;
use serde_json::json;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

pub fn local_custom_executors() -> CustomNodeExecutors {
    let mut executors = CustomNodeExecutors::default();
    executors
        .register(
            "local.pass_through",
            Arc::new(|request: &CustomNodeRequest, cancelled: &AtomicBool| {
                if cancelled.load(Ordering::Acquire) {
                    return Err(CustomExecutionError::Cancelled);
                }
                let routes = request.settings["routes"]
                    .as_array()
                    .filter(|r| r.len() <= MAX_CUSTOM_PORTS)
                    .ok_or_else(|| {
                        CustomExecutionError::Failed("透传接口需要已声明的输入输出对应关系".into())
                    })?;
                let mut outputs = BTreeMap::new();
                for route in routes {
                    let input = route["input"].as_str().unwrap_or_default();
                    let output = route["output"].as_str().unwrap_or_default();
                    let value = request
                        .inputs
                        .get(input)
                        .ok_or_else(|| CustomExecutionError::Failed("透传输入不存在".into()))?;
                    if output.is_empty()
                        || outputs.insert(output.into(), value.value.clone()).is_some()
                    {
                        return Err(CustomExecutionError::Failed("透传输出重复或为空".into()));
                    }
                }
                Ok(CustomNodeResult { outputs })
            }),
        )
        .expect("valid local host interface");
    executors
}
pub fn configure_pass_through(definition: &mut CustomNodeDefinition) -> Result<(), CreativeError> {
    if definition.executor != "local.pass_through" {
        return Ok(());
    }
    if definition.inputs.len() != definition.outputs.len()
        || definition
            .inputs
            .iter()
            .zip(&definition.outputs)
            .any(|(a, b)| a.data_type != b.data_type)
    {
        return Err(CreativeError(
            "本地透传要求输入/输出数量相等，按顺序类型一一匹配".into(),
        ));
    }
    definition.default_settings = json!({"routes":definition.inputs.iter().zip(&definition.outputs)
        .map(|(input,output)| json!({"input":input.id,"output":output.id})).collect::<Vec<_>>()});
    Ok(())
}
pub fn validate_custom_value_assets(
    value: &CustomValue,
    assets: &[ProjectAsset],
) -> Result<(), CreativeError> {
    value.validate()?;
    let Some(key) = value.asset_key() else {
        return Ok(());
    };
    let asset = assets
        .iter()
        .find(|a| a.key == key)
        .ok_or_else(|| CreativeError("自定义输出素材不属于当前项目".into()))?;
    let valid = match value {
        CustomValue::Image { .. } => asset.media_type.starts_with("image/"),
        CustomValue::Video { .. } => asset.media_type.starts_with("video/"),
        CustomValue::Audio { .. } => asset.media_type.starts_with("audio/"),
        CustomValue::Model { .. } => asset.media_type == "model/gltf-binary",
        CustomValue::Mask { source, .. } => {
            asset.media_type == crate::MASK_MEDIA_TYPE
                && asset.width == source.width
                && asset.height == source.height
                && assets.iter().any(|a| {
                    a.key == source.asset_key
                        && matches!(a.media_type.as_str(), "image/png" | "image/jpeg")
                        && a.width == source.width
                        && a.height == source.height
                })
        }
        _ => false,
    };
    if !valid {
        return Err(CreativeError(
            "自定义输出的素材类型或Mask原图尺寸不匹配".into(),
        ));
    }
    Ok(())
}
pub fn validate_custom_node_assets(
    node: &Node,
    assets: &[ProjectAsset],
) -> Result<(), CreativeError> {
    if !node.kind.starts_with("custom.") {
        return Ok(());
    }
    let data: CustomNodeData = serde_json::from_value(node.data.clone())?;
    for value in data.outputs.values() {
        validate_custom_value_assets(value, assets)?;
    }
    if let Some(request) = &data.completed_request {
        for input in request.inputs.values() {
            validate_custom_value_assets(&input.value, assets)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
