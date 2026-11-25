// Modified by Longbridge for gpui-fast.
// Preserve unchanged upstream loops under the newer workspace Clippy.
#[allow(clippy::chunks_exact_to_as_chunks)]
mod cosmic_text_system;
mod fast;
#[allow(clippy::chunks_exact_to_as_chunks)]
mod wgpu_atlas;
mod wgpu_context;
mod wgpu_renderer;

pub use cosmic_text_system::*;
pub use fast::dynamic_textures::WgpuDynamicTexture;
#[cfg(not(target_family = "wasm"))]
pub use fast::wgpu_context::wgpu_context;
pub use wgpu;
pub use wgpu_atlas::*;
pub use wgpu_context::*;
#[cfg(all(
    not(target_family = "wasm"),
    any(feature = "bench-support", feature = "test-support")
))]
pub use wgpu_renderer::WgpuHeadlessRenderer;
pub use wgpu_renderer::{GpuContext, WgpuRenderer, WgpuSurfaceConfig};

#[cfg(all(feature = "flow-headless", not(target_family = "wasm")))]
pub use fast::flow_headless::{FlowHeadlessRenderer, FlowRenderedFrame};
pub use fast::flow_namespace::{
    FlowExecutionReceipt, FlowFailureStage, FlowLayerNamespace, with_flow_layer_namespace,
    with_flow_layer_namespace_receipt,
};
