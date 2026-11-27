//! Capture the exact application's native font service; no independent fonts or network.
use super::*;
use crate::frame_text::FrameTextBackend;
use gpui_kit::component::ActiveTheme;

struct CreativeTextExportBackend(Arc<gpui_flow::GpuiTextRasterizer>);
impl Global for CreativeTextExportBackend {}

/// Install only a bridge created together with this same application.
pub fn install_creative_text_export_backend(
    bridge: Arc<gpui_flow::GpuiTextRasterizer>,
    cx: &mut App,
) -> Result<(), String> {
    if !bridge.matches_text_system(cx.text_system()) {
        return Err("Creative export requires the same application's text backend".into());
    }
    cx.set_global(CreativeTextExportBackend(bridge));
    Ok(())
}

impl CreativeCanvas {
    pub(super) fn frame_text_backend(&self, cx: &App) -> Result<FrameTextBackend, String> {
        frame_text_backend_for(self.font_families.clone(), cx)
    }
}

pub(super) fn frame_text_backend_for(
    font_families: Arc<Vec<String>>,
    cx: &App,
) -> Result<FrameTextBackend, String> {
    let backend = cx.try_global::<CreativeTextExportBackend>().ok_or(tr(
        "当前宿主未接入同一应用的文字导出服务",
        "Text export requires the application's text backend",
    ))?;
    FrameTextBackend::new(
        &backend.0,
        cx.theme().font_family.to_string(),
        font_families,
    )
}
