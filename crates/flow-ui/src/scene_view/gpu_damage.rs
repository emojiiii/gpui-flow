//! The GPU surface encloses the ordinary opaque background and DisplayElements.
//! Selection and tool widgets stay outside; semantic focus uses native fallback.
use super::*;
use crate::gpu_damage::GpuDamageHost;
impl SceneView {
    /// Opt in on a window created by the matching host. Unsupported frames use
    /// normal native painting; document editing, picking and history are unchanged.
    pub fn set_gpu_damage(
        &mut self,
        host: Option<&GpuDamageHost>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(binding) = self.gpu_damage.take() {
            binding.dispose(&mut self.editor.engine, cx);
        }
        if let Some(host) = host {
            if self.is_layer_managed() {
                return Err("detach this LayeredApp view before enabling GPU damage".into());
            }
            self.gpu_damage = Some(
                host.bind(&mut self.editor.engine, window)
                    .map_err(|e| e.to_string())?,
            );
        }
        cx.notify();
        Ok(())
    }
    pub(super) fn gpu_eligible(&self, frame: &render::StateFrame, current_canonical: bool) -> bool {
        current_canonical
            && !self.accessibility_enabled()
            && frame.resolved_scene.is_none()
            && !self.is_layer_managed()
            && self.state_styles.is_empty()
            && self.image_crop.is_none()
            && frame.units.iter().all(|unit| {
                matches!(
                    unit,
                    gpui_flow_composite::SceneUnit::Native(_)
                        | gpui_flow_composite::SceneUnit::NativePhase { .. }
                )
            })
            && frame.objects.iter().all(|object| {
                object.native_renderable()
                    && !matches!(
                        object.object.shape,
                        gpui_flow_scene::Shape::Image(_)
                            | gpui_flow_scene::Shape::Text(_)
                            | gpui_flow_scene::Shape::Canvas(_)
                            | gpui_flow_scene::Shape::Custom(_)
                    )
            })
    }
}
