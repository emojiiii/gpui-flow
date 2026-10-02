use crate::scene::{SceneCommand, SceneRotationGeometry};
use crate::*;
use gpui_flow_core::Point;
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::Affine;

struct Rotation {
    geometry: SceneRotationGeometry,
    last_angle: f64,
    total_angle: f64,
}
/// Free pointer rotation; Shift snaps total gesture delta to `snap_gap` degrees.
/// The default 45-degree gap follows the pinned Leafer editor configuration.
pub struct SceneRotatePlugin {
    rotation: Option<Rotation>,
    snap_gap: f32,
}
impl Default for SceneRotatePlugin {
    fn default() -> Self {
        Self {
            rotation: None,
            snap_gap: 45.,
        }
    }
}
impl SceneRotatePlugin {
    pub fn with_snap_gap(degrees: f32) -> Option<Self> {
        (degrees.is_finite() && degrees > 0. && degrees <= 360.).then_some(Self {
            rotation: None,
            snap_gap: degrees,
        })
    }
}
fn angle(pivot: Point, point: Point) -> Option<f64> {
    if !point.is_finite() {
        return None;
    }
    let x = point.x as f64 - pivot.x as f64;
    let y = point.y as f64 - pivot.y as f64;
    (x != 0. || y != 0.).then(|| y.atan2(x).to_degrees())
}
impl CanvasPlugin<SceneEditor, SceneCommand> for SceneRotatePlugin {
    fn id(&self) -> &str {
        "rotate"
    }
    fn priority(&self) -> i32 {
        11
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn on_lifecycle(&mut self, event: PluginLifecycle) {
        if matches!(
            event,
            PluginLifecycle::Cancelled | PluginLifecycle::Disabled | PluginLifecycle::Uninstalled
        ) {
            self.rotation = None;
        }
    }
    fn on_event(
        &mut self,
        event: &CanvasEvent,
        cx: &PluginContext<'_, SceneEditor>,
        out: &mut PluginOutput<SceneCommand>,
    ) {
        if !cx.allows(Capability::Edit) {
            return;
        }
        match event {
            CanvasEvent::PointerDown(p)
                if p.button == Some(PointerButton::Left)
                    && p.hit == HitTarget::SelectionRotateHandle
                    && !p.modifiers.space
                    && !cx.editor.editing() =>
            {
                let Some(geometry) = SceneRotationGeometry::for_selection_with_capabilities(
                    cx.editor,
                    cx.transforms,
                ) else {
                    return;
                };
                let Some(last_angle) = angle(
                    geometry.pivot,
                    cx.editor.viewport.screen_to_world(p.position),
                ) else {
                    return;
                };
                out.command(SceneCommand::BeginGesture);
                out.command(SceneCommand::CapturePointerForSelection {
                    pointer_id: p.pointer_id,
                    targets: geometry
                        .selection
                        .roots
                        .iter()
                        .map(|root| root.target)
                        .collect(),
                });
                self.rotation = Some(Rotation {
                    geometry,
                    last_angle,
                    total_angle: 0.,
                });
                out.effect(CanvasEffect::Cursor(CanvasCursor::Crosshair));
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(rotation) = &mut self.rotation {
                    let world = cx.editor.viewport.screen_to_world(p.position);
                    if !world.is_finite() {
                        out.command(SceneCommand::CancelGesture);
                        return;
                    }
                    // The pivot has no angle. Retain the last accepted sample;
                    // leaving it resumes without an artificial atan2(0,0) jump.
                    let Some(current) = angle(rotation.geometry.pivot, world) else {
                        return;
                    };
                    rotation.total_angle +=
                        (current - rotation.last_angle + 180.).rem_euclid(360.) - 180.;
                    rotation.last_angle = current;
                    let degrees = if p.modifiers.shift {
                        (rotation.total_angle / self.snap_gap as f64).round() * self.snap_gap as f64
                    } else {
                        rotation.total_angle
                    };
                    let degrees = (degrees + 180.).rem_euclid(360.) - 180.;
                    let delta = if degrees.abs() < 1e-9 {
                        Affine::IDENTITY
                    } else {
                        Affine::rotation(degrees as f32).around(rotation.geometry.pivot)
                    };
                    let Some(transforms) = rotation.geometry.selection.transforms(delta) else {
                        out.command(SceneCommand::CancelGesture);
                        return;
                    };
                    out.command(SceneCommand::SetRotationTransforms { transforms });
                } else if cx.capture_owner.is_none()
                    && !matches!(
                        p.hit,
                        HitTarget::ObjectResize(_)
                            | HitTarget::ObjectResizeHandle { .. }
                            | HitTarget::SelectionResizeHandle { .. }
                    )
                {
                    let cursor = if p.hit == HitTarget::SelectionRotateHandle
                        && SceneRotationGeometry::for_selection_with_capabilities(
                            cx.editor,
                            cx.transforms,
                        )
                        .is_some()
                    {
                        CanvasCursor::Crosshair
                    } else {
                        CanvasCursor::Default
                    };
                    out.effect(CanvasEffect::Cursor(cursor));
                }
            }
            CanvasEvent::PointerUp(_) if self.rotation.take().is_some() => {
                out.command(SceneCommand::CommitGesture);
                out.command(SceneCommand::ReleasePointer);
                out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
            }
            CanvasEvent::PointerLeave if self.rotation.is_none() && cx.capture_owner.is_none() => {
                out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
            }
            _ => {}
        }
    }
}
