use super::{PanInertia, PanInertiaOptions, ScenePanPolicy};
use crate::scene::SceneCommand;
use crate::*;
use gpui_flow_core::{Point, Viewport};
use gpui_flow_editor::SceneEditor;
use std::time::Duration;
struct PanDrag {
    position: Point,
    time: Duration,
    velocity: Point,
    last_motion: Duration,
}
#[derive(Default)]
pub struct ScenePanPlugin {
    policy: ScenePanPolicy,
    options: Option<PanInertiaOptions>,
    drag: Option<PanDrag>,
    inertia: Option<PanInertia>,
    expected: Option<Viewport>,
    document: Option<(u64, u64)>,
    now: Duration,
}
impl ScenePanPlugin {
    pub fn with_policy(mut self, policy: ScenePanPolicy) -> Self {
        self.policy = policy;
        self
    }
    pub fn with_inertia(mut self, options: PanInertiaOptions) -> Self {
        self.options = Some(options);
        self
    }
    fn stop(&mut self) {
        self.drag = None;
        self.inertia = None;
        self.expected = None;
        self.document = None;
    }
}
impl CanvasPlugin<SceneEditor, SceneCommand> for ScenePanPlugin {
    fn id(&self) -> &str {
        "pan"
    }
    fn priority(&self) -> i32 {
        -20
    }
    fn is_default_action(&self) -> bool {
        true
    }
    fn on_lifecycle(&mut self, event: PluginLifecycle) {
        if matches!(
            event,
            PluginLifecycle::Cancelled | PluginLifecycle::Disabled | PluginLifecycle::Uninstalled
        ) {
            self.stop();
        }
    }
    fn on_time(&mut self, now: Duration) {
        if now < self.now {
            self.stop();
        }
        self.now = now;
    }
    fn needs_frame(&self) -> bool {
        self.inertia.as_ref().is_some_and(PanInertia::is_active)
    }
    fn on_frame(
        &mut self,
        now: Duration,
        cx: &PluginContext<'_, SceneEditor>,
        out: &mut PluginOutput<SceneCommand>,
    ) {
        if now < self.now {
            self.stop();
        }
        self.now = now;
        if !cx.allows(Capability::Navigation)
            || cx.capture_owner.is_some_and(|owner| owner != "pan")
            || self.document.is_some_and(|identity| {
                identity
                    != (
                        cx.editor.engine.engine_id(),
                        cx.editor.engine.document_epoch(),
                    )
            })
            || self
                .expected
                .is_some_and(|expected| expected != cx.editor.viewport)
        {
            self.inertia = None;
            self.expected = None;
        }
        if let Some(inertia) = &mut self.inertia {
            let delta = inertia.advance(now);
            if delta != Point::ZERO {
                let mut expected = cx.editor.viewport;
                if expected.pan_by(delta) {
                    out.command(SceneCommand::Pan { delta });
                    self.expected = Some(expected);
                } else {
                    inertia.stop();
                }
            }
            if !inertia.is_active() {
                self.inertia = None;
                self.expected = None;
            }
        }
    }
    fn on_event(
        &mut self,
        event: &CanvasEvent,
        cx: &PluginContext<'_, SceneEditor>,
        out: &mut PluginOutput<SceneCommand>,
    ) {
        if !cx.allows(Capability::Navigation) {
            self.stop();
            return;
        }
        match event {
            CanvasEvent::PointerDown(p)
                if p.hit != HitTarget::Control
                    && ((self.policy.middle_drag && p.button == Some(PointerButton::Middle))
                        || (p.button == Some(PointerButton::Left)
                            && ((self.policy.space_drag && p.modifiers.space)
                                || (self.policy.background_drag
                                    && p.hit == HitTarget::Background
                                    && !p.modifiers.shift)))) =>
            {
                self.inertia = None;
                self.expected = None;
                self.drag = Some(PanDrag {
                    position: p.position,
                    time: self.now,
                    velocity: Point::ZERO,
                    last_motion: self.now,
                });
                out.command(SceneCommand::CapturePointer {
                    pointer_id: p.pointer_id,
                });
                out.effect(CanvasEffect::Cursor(CanvasCursor::Grabbing));
            }
            CanvasEvent::PointerMove(p) => {
                if let Some(drag) = &mut self.drag {
                    let delta = p.position - drag.position;
                    let elapsed = self.now.saturating_sub(drag.time).as_secs_f64();
                    if delta != Point::ZERO {
                        drag.velocity = if elapsed > 0.0 {
                            Point::new(
                                (delta.x as f64 / elapsed) as f32,
                                (delta.y as f64 / elapsed) as f32,
                            )
                        } else {
                            Point::ZERO
                        };
                        drag.last_motion = self.now;
                    }
                    drag.position = p.position;
                    drag.time = self.now;
                    out.command(SceneCommand::Pan { delta });
                }
            }
            CanvasEvent::PointerUp(_) => {
                if let Some(drag) = self.drag.take() {
                    self.inertia = self
                        .options
                        .filter(|options| {
                            self.now.saturating_sub(drag.last_motion) <= options.release_window
                        })
                        .and_then(|options| PanInertia::new(drag.velocity, self.now, options));
                    self.expected = self.inertia.as_ref().map(|_| cx.editor.viewport);
                    self.document = self.inertia.as_ref().map(|_| {
                        (
                            cx.editor.engine.engine_id(),
                            cx.editor.engine.document_epoch(),
                        )
                    });
                    out.command(SceneCommand::ReleasePointer);
                    out.effect(CanvasEffect::Cursor(CanvasCursor::Default));
                }
            }
            CanvasEvent::Wheel {
                delta, modifiers, ..
            } if self.policy.wheel.pans(modifiers.command()) => {
                self.inertia = None;
                self.expected = None;
                let delta = if self.policy.shift_wheel_horizontal && modifiers.shift {
                    Point::new(delta.x + delta.y, 0.0)
                } else {
                    *delta
                };
                out.command(SceneCommand::Pan { delta });
            }
            _ => {}
        }
    }
}
