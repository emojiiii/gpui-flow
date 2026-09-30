//! Retained-scene canvas, independent of node graphs, media runners and Studio chrome.
mod interaction;
mod render;
use crate::{
    FlowTheme,
    display::{RenderIssue, RenderObject},
};
use gpui::{prelude::*, *};
use gpui_flow_core::{Point as WorldPoint, Size as WorldSize};
use gpui_flow_editor::SceneEditor;
use gpui_flow_scene::{
    Affine, DisplayId, EventDispatcher, EventKind, HitTestOptions, Scene, SceneError, SceneEvent,
};

pub enum SceneViewEvent {
    Changed,
    SelectionChanged,
    Pointer(SceneEvent),
}
impl EventEmitter<SceneViewEvent> for SceneView {}
#[derive(Clone)]
enum Gesture {
    Pan {
        last: WorldPoint,
    },
    Move {
        last: WorldPoint,
    },
    Resize {
        id: DisplayId,
        inverse: Affine,
        original: Affine,
        extent: WorldSize,
    },
    Marquee {
        start: WorldPoint,
        current: WorldPoint,
    },
}
/// Embed this view in any host shell. Attach core event listeners to `dispatcher`.
/// Actual content lives in `editor.scene`; no demo-specific UI or provider runs occur here.
pub struct SceneView {
    pub editor: SceneEditor,
    pub theme: FlowTheme,
    pub dispatcher: EventDispatcher,
    pub show_grid: bool,
    pub editable: bool,
    pub diagnostics: Vec<RenderIssue>,
    pub renderers: crate::display::DisplayRegistry,
    focus: FocusHandle,
    origin: WorldPoint,
    extent: WorldSize,
    gesture: Option<Gesture>,
    pressed: Option<(DisplayId, WorldPoint)>,
}
impl SceneView {
    pub fn new(scene: Scene, cx: &mut Context<Self>) -> Result<Self, SceneError> {
        Ok(Self {
            editor: SceneEditor::new(scene)?,
            theme: FlowTheme::default(),
            dispatcher: EventDispatcher::default(),
            show_grid: true,
            editable: true,
            diagnostics: vec![],
            renderers: crate::display::DisplayRegistry::default(),
            focus: cx.focus_handle(),
            origin: WorldPoint::ZERO,
            extent: WorldSize::new(1000., 700.),
            gesture: None,
            pressed: None,
        })
    }
    pub fn fit(&mut self) {
        if let Ok(items) = self.editor.scene.paint_list() {
            let bounds = items
                .iter()
                .filter_map(|item| {
                    item.object
                        .paint_bounds()
                        .map(|r| item.world_transform.transform_bounds(r))
                })
                .reduce(|a, b| a.union(b));
            if let Some(bounds) = bounds {
                self.editor.viewport.fit(bounds, self.extent, 50.);
            }
        }
    }
    pub fn set_scene(&mut self, scene: Scene, cx: &mut Context<Self>) -> Result<(), SceneError> {
        self.editor = SceneEditor::new(scene)?;
        self.gesture = None;
        self.pressed = None;
        self.dispatcher = EventDispatcher::default();
        cx.emit(SceneViewEvent::Changed);
        cx.notify();
        Ok(())
    }
    fn local(&self, p: gpui::Point<Pixels>) -> WorldPoint {
        WorldPoint::new(p.x.into(), p.y.into()) - self.origin
    }
    fn world(&self, p: gpui::Point<Pixels>) -> WorldPoint {
        self.editor.viewport.screen_to_world(self.local(p))
    }
    fn hit(&self, p: WorldPoint) -> Option<DisplayId> {
        self.editor
            .scene
            .hit_test(
                p,
                HitTestOptions {
                    tolerance: 4. / self.editor.viewport.zoom,
                    ..Default::default()
                },
            )
            .ok()
            .flatten()
            .map(|hit| hit.id)
    }
    fn dispatch(
        &mut self,
        kind: EventKind,
        target: DisplayId,
        p: WorldPoint,
        cx: &mut Context<Self>,
    ) -> bool {
        match self
            .dispatcher
            .dispatch(&self.editor.scene, SceneEvent::new(kind, target).at(p))
        {
            Ok(result) => {
                let prevented = result.event.default_prevented();
                cx.emit(SceneViewEvent::Pointer(result.event));
                prevented
            }
            Err(_) => false,
        }
    }
    fn finish(&mut self, cx: &mut Context<Self>) {
        if let Some(Gesture::Marquee { start, current }) = self.gesture.take() {
            let rect = gpui_flow_core::Rect::from_points(start, current);
            self.editor.selection.clear();
            if let Ok(items) = self.editor.scene.paint_list() {
                self.editor.selection.ids.extend(
                    items
                        .into_iter()
                        .filter(|i| {
                            !i.locked
                                && i.object.paint_bounds().is_some_and(|b| {
                                    rect.intersects(i.world_transform.transform_bounds(b))
                                })
                        })
                        .map(|i| i.object.id.clone()),
                );
            }
            cx.emit(SceneViewEvent::SelectionChanged);
        }
        self.editor.commit_edit();
        cx.emit(SceneViewEvent::Changed);
        cx.notify();
    }
    fn selection_corners(&self, id: &DisplayId) -> Option<[WorldPoint; 4]> {
        let node = self.editor.scene.get(id)?;
        let bounds = node.object.paint_bounds()?;
        let transform = self.editor.scene.world_transform(id).ok()?;
        Some(
            [
                bounds.origin,
                WorldPoint::new(bounds.max_x(), bounds.min_y()),
                WorldPoint::new(bounds.max_x(), bounds.max_y()),
                WorldPoint::new(bounds.min_x(), bounds.max_y()),
            ]
            .map(|p| {
                self.editor
                    .viewport
                    .world_to_screen(transform.transform_point(p))
            }),
        )
    }
}
impl Focusable for SceneView {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}
