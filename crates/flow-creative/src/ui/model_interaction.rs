//! A temporary camera preview over the existing model view. Only pointer release
//! commits graph data, so each orbit gesture is one ordinary undoable edit.
use super::*;
use crate::model_preview::{OrbitCamera, OrbitCameraState};
use gpui_flow_core::Rect;

#[derive(Clone)]
pub(super) struct ModelInteraction {
    pub node: NodeId,
    asset: String,
}

pub(super) struct ModelDrag {
    source: Node,
    initial: OrbitCameraState,
    current: OrbitCameraState,
    start: WorldPoint,
    size: WorldSize,
}
impl ModelDrag {
    fn new(source: Node, start: WorldPoint, size: WorldSize) -> Result<Self, String> {
        if !start.is_finite() || !size.is_positive() || !size.is_finite() {
            return Err("模型手势区域无效".into());
        }
        let initial = source
            .data
            .get("camera")
            .map(|value| serde_json::from_value(value.clone()))
            .transpose()
            .map_err(|_| "模型相机无效，请先复位".to_string())?
            .unwrap_or_else(|| OrbitCamera::default().state());
        if !initial.yaw_radians.is_finite()
            || !(0. ..=std::f32::consts::TAU).contains(&initial.yaw_radians)
            || !initial.pitch_radians.is_finite()
            || initial.pitch_radians.abs() > std::f32::consts::FRAC_PI_2 - 0.01
            || !(0.05..=100.).contains(&initial.distance_factor)
        {
            return Err("模型相机无效，请先复位".into());
        }
        Ok(Self {
            source,
            initial,
            current: initial,
            start,
            size,
        })
    }
    fn update(&mut self, point: WorldPoint) {
        if !point.is_finite() {
            return;
        }
        let delta = point - self.start;
        self.current = self.initial;
        if delta.x * delta.x + delta.y * delta.y < 9. {
            return;
        }
        self.current.yaw_radians = (self.initial.yaw_radians
            - delta.x / self.size.width * std::f32::consts::TAU)
            .rem_euclid(std::f32::consts::TAU);
        self.current.pitch_radians = (self.initial.pitch_radians
            + delta.y / self.size.height * std::f32::consts::PI)
            .clamp(-1.56, 1.56);
    }
    fn data_for(&self, current: &Node) -> Result<Option<Value>, String> {
        if current != &self.source {
            return Err("模型已变化，未完成的视角草稿已取消".into());
        }
        if self.initial == self.current {
            return Ok(None);
        }
        let mut data = current.data.clone();
        data["camera"] = serde_json::to_value(self.current).map_err(|e| e.to_string())?;
        Ok(Some(data))
    }
}

impl CreativeCanvas {
    pub(super) fn toggle_model_interaction(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model_interaction.is_some() {
            self.clear_model_interaction(cx);
            return;
        }
        let Some(node) = self
            .selected(cx)
            .filter(|node| matches!(node.kind.as_str(), "model" | "generate_model"))
        else {
            return;
        };
        if !self.canvas.read(cx).is_editable()
            || crate::layer_guard::node_is_locked(self.canvas.read(cx).editor.graph(), &node.id)
        {
            return;
        }
        let asset = node.data["asset"].as_str().unwrap_or_default().to_string();
        if asset.is_empty() {
            self.notice = Some("请先导入本地GLB".into());
            return;
        }
        self.canvas
            .update(cx, |canvas, cx| canvas.set_pan_mode(false, cx));
        self.model_interaction = Some(ModelInteraction {
            node: node.id,
            asset,
        });
        self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    pub(super) fn clear_model_interaction(&mut self, cx: &mut Context<Self>) {
        self.model_interaction = None;
        self.cancel_model_drag(cx);
        cx.notify();
    }
    pub(super) fn cancel_model_drag(&mut self, cx: &mut Context<Self>) {
        if self.model_drag.take().is_some() {
            self.model_views.clear_camera_override();
            self.canvas.update(cx, |_, cx| cx.notify());
            cx.notify();
        }
    }
    pub(super) fn reconcile_model_interaction(&mut self, cx: &mut Context<Self>) {
        let Some(mode) = self.model_interaction.clone() else {
            return;
        };
        let node = self.selected(cx);
        let valid = node.as_ref().is_some_and(|node| {
            node.id == mode.node
                && matches!(node.kind.as_str(), "model" | "generate_model")
                && node.data["asset"].as_str() == Some(mode.asset.as_str())
                && self.canvas.read(cx).is_editable()
                && gpui_flow_core::node_is_visible(self.canvas.read(cx).editor.graph(), &node.id)
                && !crate::layer_guard::node_is_locked(
                    self.canvas.read(cx).editor.graph(),
                    &node.id,
                )
        });
        if !valid {
            self.clear_model_interaction(cx);
            return;
        }
        if self
            .model_drag
            .as_ref()
            .is_some_and(|draft| node.as_ref() != Some(&draft.source))
        {
            self.cancel_model_drag(cx);
        }
    }
    fn begin_model_drag(
        &mut self,
        bounds: Rect,
        point: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.reconcile_model_interaction(cx);
        if self.temporary_hand.is_some()
            || self.model_interaction.is_none()
            || self.model_drag.is_some()
        {
            return;
        }
        let Some(node) = self.selected(cx) else {
            return;
        };
        match ModelDrag::new(
            node,
            WorldPoint::new(point.x.into(), point.y.into()),
            bounds.size,
        ) {
            Ok(draft) => self.model_drag = Some(draft),
            Err(error) => self.notice = Some(error),
        }
        self.canvas.read(cx).focus_handle(cx).focus(window, cx);
        cx.notify();
    }
    pub(super) fn move_model_drag(&mut self, point: Point<Pixels>, cx: &mut Context<Self>) {
        self.reconcile_model_interaction(cx);
        let Some(draft) = self.model_drag.as_mut() else {
            return;
        };
        draft.update(WorldPoint::new(point.x.into(), point.y.into()));
        let asset = draft.source.data["asset"]
            .as_str()
            .unwrap_or_default()
            .to_string();
        self.model_views
            .set_camera_override(draft.source.id.clone(), asset, draft.current);
        self.canvas.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
    pub(super) fn finish_model_drag(&mut self, point: Point<Pixels>, cx: &mut Context<Self>) {
        self.move_model_drag(point, cx);
        let Some(draft) = self.model_drag.take() else {
            return;
        };
        self.model_views.clear_camera_override();
        let current = self
            .canvas
            .read(cx)
            .editor
            .graph()
            .node(&draft.source.id)
            .cloned();
        if let Some(current) = current {
            match draft.data_for(&current) {
                Ok(Some(data)) => {
                    self.command(
                        CanvasCommand::SetNodeData {
                            node: current.id,
                            data,
                        },
                        cx,
                    );
                }
                Ok(None) => {}
                Err(error) => self.notice = Some(error),
            }
        }
        self.canvas.update(cx, |_, cx| cx.notify());
        cx.notify();
    }
    pub(super) fn model_interaction_overlay(
        &self,
        node: &Node,
        bounds: Rect,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if self.temporary_hand.is_some()
            || self.canvas.read(cx).pan_mode()
            || !self.canvas.read(cx).is_editable()
            || crate::layer_guard::node_is_locked(self.canvas.read(cx).editor.graph(), &node.id)
            || self.model_interaction.as_ref().is_none_or(|mode| {
                mode.node != node.id || node.data["asset"].as_str() != Some(mode.asset.as_str())
            })
        {
            return None;
        }
        Some(
            div()
                .id("model-orbit-surface")
                .occlude()
                .absolute()
                .left(px(bounds.origin.x))
                .top(px(bounds.origin.y))
                .w(px(bounds.size.width))
                .h(px(bounds.size.height))
                .cursor(if self.model_drag.is_some() {
                    CursorStyle::ClosedHand
                } else {
                    CursorStyle::OpenHand
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        window.prevent_default();
                        this.begin_model_drag(bounds, event.position, window, cx);
                    }),
                )
                .on_mouse_move(cx.listener(|this, event: &MouseMoveEvent, _, cx| {
                    if event.dragging() && this.model_drag.is_some() {
                        cx.stop_propagation();
                        this.move_model_drag(event.position, cx);
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseUpEvent, _, cx| {
                        cx.stop_propagation();
                        this.finish_model_drag(event.position, cx);
                    }),
                )
                .on_mouse_up_out(
                    MouseButton::Left,
                    cx.listener(|this, event: &MouseUpEvent, _, cx| {
                        if this.model_drag.is_some() {
                            cx.stop_propagation();
                            this.finish_model_drag(event.position, cx);
                        }
                    }),
                ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::{ModelDrag, OrbitCamera};
    use gpui_flow_core::{Point, Size};
    #[test]
    fn orbit_draft_is_one_camera_change_and_keeps_geometry_asset_and_unrelated_data() {
        let mut node = crate::standard_registry()
            .make_node("model", "model", Point::new(10., 20.), None)
            .unwrap();
        node.data["asset"] = serde_json::json!("assets/test.glb");
        let original = node.clone();
        let mut draft =
            ModelDrag::new(node.clone(), Point::new(100., 200.), Size::new(400., 300.)).unwrap();
        draft.update(Point::new(101., 201.));
        assert_eq!(draft.data_for(&node).unwrap(), None);
        draft.update(Point::new(200., 250.));
        let data = draft.data_for(&node).unwrap().unwrap();
        assert_ne!(
            data["camera"],
            serde_json::to_value(OrbitCamera::default().state()).unwrap()
        );
        assert_eq!(data["asset"], node.data["asset"]);
        assert_eq!(data["caption"], node.data["caption"]);
        assert_eq!(node, original);
        node.data["asset"] = serde_json::json!("assets/replaced.glb");
        assert!(draft.data_for(&node).is_err());
        let mut locked = original.clone();
        locked.locked = true;
        assert!(draft.data_for(&locked).is_err());
    }
    #[test]
    fn orbit_uses_screen_fraction_and_cancel_never_changes_source() {
        let node = crate::standard_registry()
            .make_node("model", "model", Point::ZERO, None)
            .unwrap();
        let mut a = ModelDrag::new(node.clone(), Point::ZERO, Size::new(200., 150.)).unwrap();
        let mut b = ModelDrag::new(node.clone(), Point::ZERO, Size::new(800., 600.)).unwrap();
        a.update(Point::new(50., 25.));
        b.update(Point::new(200., 100.));
        assert_eq!(a.current, b.current);
        a.update(Point::new(10000., -10000.));
        assert!(a.current.yaw_radians >= 0. && a.current.yaw_radians < std::f32::consts::TAU);
        assert_eq!(a.current.pitch_radians, -1.56);
        a.update(Point::ZERO);
        assert_eq!(a.data_for(&node).unwrap(), None);
    }
}
