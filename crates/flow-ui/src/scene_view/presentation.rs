//! A bounded two-slot presentation: one displayed snapshot and one admitted CPU
//! sample. State time keeps advancing while busy; the next admission samples the
//! latest state, rather than cancelling useful work at every vsync.
use super::*;
use crate::display::composite::RequestKey;
use gpui_flow_engine::SceneEngine;
use std::{rc::Rc, sync::Arc};

/// Endpoint paint and hit routing share this gate. A pending or invalidated
/// presentation supplies None, so invisible line handles cannot remain targets.
pub(super) fn line_handles_for_presentation(
    editor: &SceneEditor,
    geometry: Option<&SceneEngine>,
) -> Option<(
    DisplayId,
    [gpui_flow_interaction::scene::SceneLineHandle; 2],
)> {
    let geometry = gpui_flow_interaction::scene::SceneLineGeometry::for_selection_with_geometry(
        editor,
        geometry?.scene(),
    )?;
    let id = editor.engine.instance(geometry.target).ok()?.id.clone();
    Some((id, geometry.screen_handles(editor)?))
}

fn unretained_hit_geometry<'a>(
    canonical: &'a SceneEngine,
    styled: Option<&'a SceneEngine>,
) -> Option<&'a SceneEngine> {
    (!render::fixed_presentation_required(canonical.scene())).then_some(styled.unwrap_or(canonical))
}

pub(super) fn view_matches(
    key: &RequestKey,
    actual: gpui_flow_composite::RasterViewport,
    window: u64,
) -> bool {
    key.inputs_current() && key.window == window && key.viewport == actual
}
fn upload_container(upload: impl IntoElement) -> gpui::Div {
    // Div supplies opacity to all descendants even at fractional origin/DPR.
    div().absolute().size_full().opacity(0.).child(upload)
}

#[derive(Clone, PartialEq)]
struct Configuration {
    mode: SceneCompositing,
    background: u32,
    grid: Option<u32>,
    crop: Option<DisplayId>,
    owner: Option<u64>,
}
struct Snapshot {
    key: Arc<RequestKey>,
    frame: render::StateFrame,
    geometry: Option<SceneEngine>,
    configuration: Configuration,
}
#[derive(Default)]
pub(super) struct Presentations {
    active: bool,
    displayed: Option<Rc<Snapshot>>,
    admitted: Option<Rc<Snapshot>>,
}
impl Presentations {
    fn publish(
        &mut self,
        controller: &mut crate::display::composite::CompositeController,
    ) -> Option<Vec<crate::display::composite::ImageFrame>> {
        let retired = controller.publish_presentation(&self.admitted.as_ref()?.key)?;
        self.displayed = self.admitted.take();
        Some(retired)
    }
    fn needs_admission(
        &self,
        key: &Arc<RequestKey>,
        frame: &render::StateFrame,
        failed: bool,
    ) -> bool {
        let changed = self
            .displayed
            .as_ref()
            .is_none_or(|s| &s.key != key || &s.frame != frame);
        let failed_newer = failed
            && self
                .admitted
                .as_ref()
                .is_some_and(|s| &s.key != key || &s.frame != frame);
        changed && (self.admitted.is_none() || failed_newer)
    }
}
impl SceneView {
    fn presentation_configuration(&self) -> Configuration {
        Configuration {
            mode: self.compositing_mode,
            background: self.theme.background,
            grid: (self.show_grid && !self.is_layer_managed()).then_some(self.theme.grid),
            crop: self.image_crop_preview_id(),
            owner: match self.layer_host {
                layer_host::LayerHostState::Managed(owner) => Some(owner),
                _ => None,
            },
        }
    }
    fn presentation_current(&self, snapshot: &Snapshot) -> bool {
        let Some(mut viewport) = self.composite_geometry else {
            return false;
        };
        viewport.world_to_screen = gpui_flow_scene::Affine::translation(
            viewport.canvas_bounds.origin.x,
            viewport.canvas_bounds.origin.y,
        ) * gpui_flow_scene::Affine::translation(
            self.editor.viewport.pan.x,
            self.editor.viewport.pan.y,
        ) * gpui_flow_scene::Affine::scale(
            self.editor.viewport.zoom,
            self.editor.viewport.zoom,
        );
        snapshot.configuration == self.presentation_configuration()
            && snapshot.key.presentation_current(
                &self.editor.engine,
                self.resources.as_ref(),
                self.text_rasterizer.as_ref(),
                &self.custom_rasterizers,
                viewport,
                snapshot.key.window,
            )
    }
    /// Picking always follows the visible snapshot. A hard invalidation suppresses
    /// hits until a replacement is shown; falling back to live geometry here
    /// would make an invisible moved/deleted subtree clickable.
    pub(super) fn presentation_hit_geometry(&self) -> Option<&SceneEngine> {
        if !self.presentations.active {
            return unretained_hit_geometry(
                &self.editor.engine,
                self.state_styles.geometry_engine(&self.editor.engine),
            );
        }
        let snapshot = self.presentations.displayed.as_ref()?;
        self.presentation_current(snapshot)
            .then(|| snapshot.geometry.as_ref().unwrap_or(&self.editor.engine))
    }
    /// Read-only geometry of the actually displayed state frame. None means an
    /// invalidated/initial CPU presentation has not been published yet.
    pub fn presented_geometry(&self) -> Option<&Scene> {
        self.presentation_hit_geometry().map(SceneEngine::scene)
    }
    pub(super) fn presentation_hit_key(&self) -> Option<&RequestKey> {
        self.presentations
            .displayed
            .as_ref()
            .filter(|snapshot| self.presentations.active && self.presentation_current(snapshot))
            .map(|snapshot| snapshot.key.as_ref())
    }
    pub(super) fn presentation_content(
        &self,
        child: AnyElement,
        key: Option<Arc<RequestKey>>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        PresentationElement {
            child,
            key,
            entity: cx.entity().downgrade(),
            viewport: self.editor.viewport,
        }
    }
    pub(super) fn presentation_draw_current(&self, key: &RequestKey) -> bool {
        self.presentations
            .displayed
            .as_ref()
            .is_some_and(|snapshot| *snapshot.key == *key && self.presentation_current(snapshot))
    }
    pub(super) fn presentation_paint_failed(
        &mut self,
        key: &RequestKey,
        id: DisplayId,
        error: String,
    ) {
        if self
            .presentations
            .displayed
            .as_ref()
            .is_some_and(|snapshot| *snapshot.key == *key)
        {
            // A driver/atlas failure after preflight must not leave invisible
            // accepted geometry clickable. Recovery needs another valid batch.
            self.presentations.displayed = None;
            self.compositing.presentation_paint_failed(id, error);
        }
    }
    pub(super) fn coherent_presentation(
        &mut self,
        frame: render::StateFrame,
        key: Option<Arc<RequestKey>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> (render::StateFrame, Option<Arc<RequestKey>>, bool) {
        let fixed = render::fixed_presentation_required(self.editor.scene())
            || frame.resolved_scene.is_some();
        if fixed && frame.resolved_scene.is_none() {
            self.presentations.active = true;
            self.presentations.displayed = None;
            self.presentations.admitted = None;
            return (Default::default(), None, true);
        }
        if !self.presentations.active && self.state_styles.is_empty() && !fixed {
            if let Some(key) = key {
                self.request_composite_key(key, None, window, cx);
            }
            return (frame, self.compositing.requested.clone(), false);
        }
        self.presentations.active = true;
        let Some(key) = key else {
            return (Default::default(), None, true);
        };
        let valid = |snapshot: &Rc<Snapshot>| {
            snapshot.key.window == key.window && self.presentation_current(snapshot)
        };
        let hard_invalid = self
            .presentations
            .displayed
            .as_ref()
            .is_some_and(|s| !valid(s))
            || self
                .presentations
                .admitted
                .as_ref()
                .is_some_and(|s| !valid(s));
        if hard_invalid {
            self.presentations.displayed = None;
            self.presentations.admitted = None;
        }
        // Publication is before constructing any native or CPU elements. The
        // previous paint phase uploaded the whole candidate batch invisibly.
        if let Some(retired) = self.presentations.publish(&mut self.compositing) {
            for image in retired {
                self.compositing
                    .retire(image, window, cx.entity().downgrade());
            }
        }
        if self
            .presentations
            .needs_admission(&key, &frame, self.compositing.presentation_failed())
        {
            let geometry = frame
                .resolved_scene
                .as_ref()
                .map(|scene| {
                    SceneEngine::with_registry(scene.clone(), self.editor.engine.registry().clone())
                        .expect("validated fixed-stroke presentation geometry")
                })
                .or_else(|| {
                    self.state_styles
                        .geometry_engine(&self.editor.engine)
                        .map(|engine| {
                            SceneEngine::with_registry(
                                engine.scene().clone(),
                                self.editor.engine.registry().clone(),
                            )
                            .expect("validated presentation geometry")
                        })
                });
            self.presentations.admitted = Some(Rc::new(Snapshot {
                key: key.clone(),
                frame,
                geometry,
                configuration: self.presentation_configuration(),
            }));
        }
        let request = self
            .presentations
            .admitted
            .as_ref()
            .or(self.presentations.displayed.as_ref())
            .map(|s| s.key.clone())
            .unwrap_or(key);
        self.request_composite_key(
            request,
            Some(self.presentations.displayed.is_some()),
            window,
            cx,
        );
        // Native-only successors and unchanged CPU samples need no upload.
        if let Some(retired) = self.presentations.publish(&mut self.compositing) {
            for image in retired {
                self.compositing
                    .retire(image, window, cx.entity().downgrade());
            }
        }
        self.presentations
            .displayed
            .as_ref()
            .map(|snapshot| (snapshot.frame.clone(), Some(snapshot.key.clone()), true))
            .unwrap_or_else(|| (Default::default(), None, true))
    }
    pub(super) fn presentation_upload_element(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let key = self.presentations.admitted.as_ref().map(|s| s.key.clone());
        let entity = cx.entity().downgrade();
        let viewport = self.editor.viewport;
        let upload = canvas(
            |_, _, _| (),
            move |bounds, _, window, cx| {
                let Some(key) = key else {
                    return;
                };
                if !view_matches(
                    &key,
                    super::composite::geometry(bounds, viewport, window),
                    window.window_handle().window_id().as_u64(),
                ) {
                    return;
                }
                let uploads = entity
                    .update(cx, |this, _| {
                        if this
                            .presentations
                            .admitted
                            .as_ref()
                            .is_none_or(|s| s.key != key || !this.presentation_current(s))
                        {
                            return vec![];
                        }
                        this.compositing.presentation_uploads(&key)
                    })
                    .unwrap_or_default();
                let changed = !uploads.is_empty();
                for (id, frame) in uploads {
                    // Positive bounds force the real atlas upload. The parent Div
                    // supplies an explicit zero-opacity scope; unlike an empty clip,
                    // this cannot round into a visible pixel at fractional origins/DPR.
                    let uploaded = crate::display::composite::paint(&frame, window);
                    let _ = entity.update(cx, |this, _| match uploaded {
                        Ok(()) => this
                            .compositing
                            .presentation_uploaded(&key, &id, &frame.image),
                        Err(error) => this.compositing.upload_failed(&key, &id, error),
                    });
                }
                if changed {
                    window.on_next_frame(move |_, cx| {
                        let _ = entity.update(cx, |_, cx| cx.notify());
                    });
                }
            },
        )
        .absolute()
        .size_full();
        // Canvas itself does not apply element opacity in pinned GPUI; Div does.
        upload_container(upload)
    }
}

/// A single guard encloses all native/custom and CPU children. Layout may
/// change the canvas between render and prepaint; per-domain checks alone would
/// otherwise leave a partial native-only scene visible for that frame.
struct PresentationElement {
    child: AnyElement,
    key: Option<Arc<RequestKey>>,
    entity: WeakEntity<SceneView>,
    viewport: gpui_flow_core::Viewport,
}
impl PresentationElement {
    fn current(&self, bounds: Bounds<Pixels>, window: &Window, cx: &mut App) -> bool {
        let Some(key) = &self.key else {
            return false;
        };
        view_matches(
            key,
            super::composite::geometry(bounds, self.viewport, window),
            window.window_handle().window_id().as_u64(),
        ) && self
            .entity
            .update(cx, |this, _| this.presentation_draw_current(key))
            .unwrap_or(false)
    }
}
impl IntoElement for PresentationElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}
impl Element for PresentationElement {
    type RequestLayoutState = ();
    type PrepaintState = bool;
    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static ::core::panic::Location<'static>> {
        None
    }
    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }
    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let current = self.current(bounds, window, cx);
        if current {
            self.child.prepaint(window, cx);
        }
        current
    }
    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        if *prepaint && self.current(bounds, window, cx) {
            self.child.paint(window, cx);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_flow_composite::{CompositeLimits, RasterViewport, SceneUnit};
    use gpui_flow_interaction::scene::{
        SceneEventRouter, SceneStateStyles, StateGeometry, StateStyles, StateTransition,
        StateTransitions,
    };
    use gpui_flow_scene::{
        Affine, BlendMode, Bounds, Color, DisplayObject, HitTestOptions, Point, Size, Style,
        display,
    };
    use std::{collections::BTreeMap, time::Duration};

    fn fixture() -> (SceneEditor, SceneStateStyles, SceneEventRouter) {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("g", display::Group::new()).with_style(Style {
                    blend_mode: BlendMode::Normal,
                    ..Default::default()
                }),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new("r", display::Rect::new(20., 20.))
                    .with_style(Style::fill(Color::WHITE)),
                Some("g".into()),
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new("native", display::Rect::new(10., 10.))
                    .with_transform(Affine::translation(70., 40.)),
                None,
                None,
            )
            .unwrap();
        let editor = SceneEditor::new(scene).unwrap();
        let mut styles = SceneStateStyles::new(&editor.engine);
        for (id, geometry) in [
            (
                "r",
                StateGeometry {
                    width: Some(60.),
                    ..Default::default()
                },
            ),
            (
                "native",
                StateGeometry {
                    x: Some(110.),
                    ..Default::default()
                },
            ),
        ] {
            let handle = editor.engine.handle(&id.into()).unwrap();
            styles
                .bind(&editor.engine, handle, StateStyles::default())
                .unwrap();
            styles
                .set_named_geometry(
                    &editor.engine,
                    handle,
                    BTreeMap::from([("wide".into(), geometry)]),
                )
                .unwrap();
            styles
                .set_transitions(
                    &editor.engine,
                    handle,
                    StateTransitions {
                        transition: StateTransition::new(1000),
                        transition_out: None,
                    },
                )
                .unwrap();
        }
        (editor, styles, SceneEventRouter::new())
    }
    fn snapshot(
        editor: &SceneEditor,
        styles: &mut SceneStateStyles,
        router: &SceneEventRouter,
        time: u64,
    ) -> Rc<Snapshot> {
        let (frame, error) = render::state_render_frame_with_canvas(
            editor,
            router,
            styles,
            Duration::from_millis(time),
            None,
            None,
        );
        assert!(error.is_none());
        let bounds = Bounds::new(Point::ZERO, Size::new(200., 100.));
        let domains = frame
            .units
            .iter()
            .filter_map(|unit| match unit {
                SceneUnit::Domain(domain) => Some((**domain).clone()),
                _ => None,
            })
            .collect();
        let key = Arc::new(RequestKey::new(
            domains,
            &editor.engine,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: bounds,
                content_mask: bounds,
                device_scale: 1.,
            },
            1,
        ));
        let geometry = styles.geometry_engine(&editor.engine).map(|engine| {
            SceneEngine::with_registry(engine.scene().clone(), editor.engine.registry().clone())
                .unwrap()
        });
        Rc::new(Snapshot {
            key,
            frame,
            geometry,
            configuration: Configuration {
                mode: SceneCompositing::Domains,
                background: 0,
                grid: None,
                crop: None,
                owner: None,
            },
        })
    }
    fn publish(
        gate: &mut Presentations,
        controller: &mut crate::display::composite::CompositeController,
    ) {
        let key = gate.admitted.as_ref().unwrap().key.clone();
        for (id, frame) in controller.presentation_uploads(&key) {
            controller.presentation_uploaded(&key, &id, &frame.image);
        }
        gate.publish(controller).unwrap();
    }
    #[::core::prelude::v1::test]
    fn coherent_state_clock_and_retargets_do_not_starve_cpu_or_split_hit_and_native_paint() {
        let (editor, mut styles, router) = fixture();
        let mut controller = crate::display::composite::CompositeController::default();
        let first = snapshot(&editor, &mut styles, &router, 0);
        let mut gate = Presentations {
            active: true,
            displayed: None,
            admitted: Some(first.clone()),
        };
        controller.request_coherent(first.key.clone(), false);
        let work = controller.take_work().unwrap();
        controller.finish(work.key.clone(), work.run());
        publish(&mut gate, &mut controller);
        for id in ["r", "native"] {
            styles
                .select_state(
                    &editor.engine,
                    editor.engine.handle(&id.into()).unwrap(),
                    Some("wide"),
                )
                .unwrap();
        }
        snapshot(&editor, &mut styles, &router, 1);
        let admitted = snapshot(&editor, &mut styles, &router, 101);
        assert!(gate.needs_admission(&admitted.key, &admitted.frame, false));
        gate.admitted = Some(admitted.clone());
        controller.request_coherent(admitted.key.clone(), true);
        let work = controller.take_work().unwrap();
        for time in 102..501 {
            if time == 300 {
                for id in ["r", "native"] {
                    styles
                        .select_state(
                            &editor.engine,
                            editor.engine.handle(&id.into()).unwrap(),
                            None,
                        )
                        .unwrap();
                }
            }
            let latest = snapshot(&editor, &mut styles, &router, time);
            assert!(!gate.needs_admission(&latest.key, &latest.frame, false));
            assert!(Rc::ptr_eq(gate.displayed.as_ref().unwrap(), &first));
            assert!(!work.cancel.load(std::sync::atomic::Ordering::Relaxed));
        }
        assert_ne!(
            styles.geometry_presentation(&editor.engine),
            admitted.geometry.as_ref().map(SceneEngine::scene)
        );
        controller.finish(work.key.clone(), work.run());
        assert!(
            gate.publish(&mut controller).is_none(),
            "CPU completion alone is not publication"
        );
        assert!(Rc::ptr_eq(gate.displayed.as_ref().unwrap(), &first));
        publish(&mut gate, &mut controller);
        let displayed = gate.displayed.as_ref().unwrap();
        assert!(Rc::ptr_eq(displayed, &admitted));
        let geometry = displayed.geometry.as_ref().unwrap();
        let r = &displayed
            .frame
            .objects
            .iter()
            .find(|object| object.object.id.as_str() == "r")
            .unwrap()
            .object;
        assert_eq!(
            r.shape,
            geometry.scene().get(&"r".into()).unwrap().object.shape
        );
        let native = &displayed
            .frame
            .objects
            .iter()
            .find(|object| object.object.id.as_str() == "native")
            .unwrap()
            .object;
        assert_eq!(
            native.transform,
            geometry
                .scene()
                .get(&"native".into())
                .unwrap()
                .object
                .transform
        );
        let width = r.shape.local_bounds().unwrap().size.width;
        let hit = geometry
            .hit_test(Point::new(width - 0.1, 10.), HitTestOptions::default())
            .unwrap()
            .unwrap();
        assert_eq!(hit.id.as_str(), "r");
        assert!(
            editor
                .engine
                .hit_test(Point::new(width - 0.1, 10.), HitTestOptions::default())
                .unwrap()
                .is_none()
        );
        let latest = snapshot(&editor, &mut styles, &router, 501);
        assert!(gate.needs_admission(&latest.key, &latest.frame, false));
        assert_eq!(
            editor.engine.revision(),
            0,
            "runtime samples must not mutate canonical ownership"
        );
    }
    #[::core::prelude::v1::test]
    fn coherent_native_domain_route_change_keeps_complete_old_snapshot_until_upload() {
        let (editor, mut styles, router) = fixture();
        let first = snapshot(&editor, &mut styles, &router, 0);
        let mut gate = Presentations {
            active: true,
            displayed: Some(first.clone()),
            admitted: None,
        };
        let mut controller = crate::display::composite::CompositeController::default();
        controller.request_coherent(first.key.clone(), false);
        let work = controller.take_work().unwrap();
        controller.finish(work.key.clone(), work.run());
        gate.admitted = Some(first.clone());
        publish(&mut gate, &mut controller);
        let mut native = snapshot(&editor, &mut styles, &router, 1);
        let native = Rc::get_mut(&mut native).unwrap();
        native.frame.units = vec![
            SceneUnit::Native("r".into()),
            SceneUnit::Native("native".into()),
        ];
        native.key = Arc::new(RequestKey::new(
            vec![],
            &editor.engine,
            first.key.viewport,
            first.key.window,
        ));
        let native = Rc::new(Snapshot {
            key: native.key.clone(),
            frame: native.frame.clone(),
            geometry: None,
            configuration: native.configuration.clone(),
        });
        gate.admitted = Some(native.clone());
        controller.request_coherent(native.key.clone(), true);
        assert!(controller.take_work().is_none());
        assert_eq!(gate.publish(&mut controller).unwrap().len(), 1);
        assert!(Rc::ptr_eq(gate.displayed.as_ref().unwrap(), &native));
        assert!(
            controller
                .presentation_frame(&first.key, &"g".into())
                .is_none()
        );
        assert!(
            gpui_flow_composite::scene_units(editor.scene(), CompositeLimits::default()).is_ok()
        );
    }
    #[::core::prelude::v1::test]
    fn coherent_subtree_guard_rejects_late_fractional_viewport_changes() {
        let (editor, mut styles, router) = fixture();
        let sample = snapshot(&editor, &mut styles, &router, 0);
        for scale in [1., 1.25, 1.5, 2.] {
            let mut key = (*sample.key).clone();
            key.viewport.canvas_bounds.origin = Point::new(0.25, 0.75);
            key.viewport.world_to_screen = Affine::translation(0.25, 0.75);
            key.viewport.device_scale = scale;
            assert!(view_matches(&key, key.viewport, key.window));
            for axis in 0..4 {
                let mut actual = key.viewport;
                match axis {
                    0 => actual.canvas_bounds.origin.x += 0.25,
                    1 => actual.canvas_bounds.size.width += 1.,
                    2 => actual.content_mask.size.height -= 1.,
                    _ => actual.device_scale += 0.25,
                }
                // The same guard encloses the entire subtree in both prepaint
                // and paint, so native/custom children cannot bypass this result.
                assert!(!view_matches(&key, actual, key.window));
            }
            assert!(!view_matches(&key, key.viewport, key.window + 1));
        }
    }
    #[::core::prelude::v1::test]
    fn coherent_upload_uses_exact_zero_parent_opacity_not_a_rounded_empty_clip() {
        let mut parent = upload_container(div());
        assert_eq!(parent.style().opacity, Some(0.));
        // Opacity is an exact multiplicative zero at every origin/DPR. Pinned
        // Div still invokes child paint, so positive image bounds reach atlas
        // insertion; Canvas opacity alone would not provide this scope.
        for scale in [1., 1.25, 1.5, 2.] {
            for origin in [0., 0.1, 0.25, 0.75] {
                let fractional_device_origin: f32 = origin * scale;
                assert_eq!(
                    fractional_device_origin * parent.style().opacity.unwrap(),
                    0.
                );
            }
        }
    }
}

#[cfg(test)]
mod scale_fixed_tests {
    use super::*;
    use gpui_flow_scene::{Point, Style, display, *};
    #[::core::prelude::v1::test]
    fn scale_fixed_line_handles_follow_accepted_geometry_and_withhold_pending_or_invalid_views() {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new(
                    "line",
                    display::Line::new(Point::new(10., 10.), Point::new(40., 10.)),
                )
                .with_style(Style {
                    fill: None,
                    stroke: Some(
                        Stroke::new(Color::BLACK, MIN_ALIGNED_STROKE_WIDTH)
                            .with_scale_fixed(StrokeScaleFixed::Fixed)
                            .with_alignment(StrokeAlignment::Outside),
                    ),
                    ..Default::default()
                }),
                None,
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        editor.selection.select("line".into(), false);
        assert!(line_handles_for_presentation(&editor, None).is_none());
        let geometry = editor.engine.resolved_for_view(1.).unwrap();
        let handles = line_handles_for_presentation(&editor, Some(&geometry)).unwrap();
        assert_eq!(handles.0, DisplayId::from("line"));
        assert_eq!(handles.1[0].position, Point::new(10., 10.));
        let mut changed = editor.scene().clone();
        changed
            .update_object(&"line".into(), |o| {
                o.transform = Affine::translation(50., 0.)
            })
            .unwrap();
        let accepted = SceneEngine::new(changed.resolved_for_view(1.).unwrap()).unwrap();
        assert_eq!(
            line_handles_for_presentation(&editor, Some(&accepted))
                .unwrap()
                .1[0]
                .position,
            Point::new(60., 10.)
        );
        editor.viewport.zoom = 2.;
        assert!(editor.engine.resolved_for_view(2.).is_err());
        assert!(line_handles_for_presentation(&editor, None).is_none());
    }
    #[::core::prelude::v1::test]
    fn scale_fixed_derived_document_camera_mismatch_never_uses_unretained_hits_or_handles() {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new(
                    "line",
                    display::Line::new(Point::new(10., 10.), Point::new(40., 10.)),
                )
                .with_style(Style {
                    fill: None,
                    stroke: Some(
                        Stroke::new(Color::BLACK, 8.).with_scale_fixed(StrokeScaleFixed::Fixed),
                    ),
                    ..Default::default()
                }),
                None,
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene.resolved_for_view(2.).unwrap()).unwrap();
        editor.selection.select("line".into(), false);
        let mut styles = gpui_flow_interaction::scene::SceneStateStyles::new(&editor.engine);
        let router = gpui_flow_interaction::scene::SceneEventRouter::new();
        assert!(!editor.scene().has_scale_fixed_strokes());
        assert!(render::fixed_presentation_required(editor.scene()));
        let (frame, error) = render::state_render_frame_with_canvas(
            &editor,
            &router,
            &mut styles,
            std::time::Duration::ZERO,
            None,
            None,
        );
        assert!(error.is_some());
        assert!(frame.objects.is_empty());
        assert!(frame.resolved_scene.is_none());
        let hit = unretained_hit_geometry(&editor.engine, None);
        assert!(hit.is_none());
        assert!(line_handles_for_presentation(&editor, hit).is_none());
        editor.viewport.zoom = 2.;
        let (frame, error) = render::state_render_frame_with_canvas(
            &editor,
            &router,
            &mut styles,
            std::time::Duration::ZERO,
            None,
            None,
        );
        assert!(error.is_none());
        assert!(frame.resolved_scene.is_some());
        assert_eq!(
            frame.objects[0].object.style.stroke.as_ref().unwrap().width,
            4.
        );
        // Even a matching context waits for publication before it becomes pickable.
        assert!(unretained_hit_geometry(&editor.engine, None).is_none());
    }
}
