use super::*;
use crate::display::DisplayElement;
use gpui_flow_composite::{CanvasBackdrop, CanvasGrid, CompositeLimits, SceneUnit};
struct StateFrame {
    objects: Vec<RenderObject>,
    units: Vec<SceneUnit>,
}
fn animation_images(
    frame: &StateFrame,
    controller: &crate::display::composite::CompositeController,
) -> std::collections::BTreeSet<DisplayId> {
    let mut visible = std::collections::BTreeSet::new();
    for unit in &frame.units {
        match unit {
            SceneUnit::Domain(domain) if controller.can_animate_domain(domain.root_id()) => {
                visible.extend(domain.images().into_iter().map(|(id, _)| id.clone()));
            }
            SceneUnit::Native(id)
                if frame.objects.iter().any(|o| {
                    &o.object.id == id
                        && matches!(o.object.shape, gpui_flow_scene::Shape::Image(_))
                        && o.native_renderable()
                }) =>
            {
                visible.insert(id.clone());
            }
            _ => {}
        }
    }
    visible
}
fn state_render_frame_with_canvas(
    editor: &SceneEditor,
    router: &gpui_flow_interaction::scene::SceneEventRouter,
    styles: &mut gpui_flow_interaction::scene::SceneStateStyles,
    now: std::time::Duration,
    backdrop: Option<CanvasBackdrop>,
) -> (StateFrame, Option<String>) {
    let convert = |scene: &Scene| -> Result<StateFrame, String> {
        let units = if let Some(backdrop) = backdrop {
            vec![match gpui_flow_composite::owned_canvas(
                scene,
                backdrop,
                CompositeLimits::default(),
            ) {
                Ok(source) => SceneUnit::Domain(Box::new(source)),
                Err(error) => SceneUnit::Rejected(gpui_flow_composite::CompositeIssue {
                    object: DisplayId::default(),
                    error,
                }),
            }]
        } else {
            gpui_flow_composite::scene_units(scene, CompositeLimits::default())
                .map_err(|e| e.to_string())?
        };
        let objects = scene
            .paint_list()
            .map_err(|e| e.to_string())?
            .iter()
            .map(|item| RenderObject::from_paint_item(item, editor.viewport))
            .collect();
        Ok(StateFrame { objects, units })
    };
    let presented = styles
        .presentation_at(editor, router, now)
        .map_err(|e| e.to_string())
        .and_then(|scene| convert(&scene));
    match presented {
        Ok(frame) => (frame, None),
        Err(error) => match convert(editor.scene()) {
            Ok(frame) => (frame, Some(error)),
            Err(fallback) => (
                StateFrame {
                    objects: vec![],
                    units: vec![],
                },
                Some(format!("{error}; canonical render: {fallback}")),
            ),
        },
    }
}
#[cfg(test)]
fn state_render_frame(
    editor: &SceneEditor,
    router: &gpui_flow_interaction::scene::SceneEventRouter,
    styles: &mut gpui_flow_interaction::scene::SceneStateStyles,
    now: std::time::Duration,
) -> (StateFrame, Option<String>) {
    state_render_frame_with_canvas(editor, router, styles, now, None)
}
fn canvas_backdrop(
    theme: FlowTheme,
    show_grid: bool,
    viewport: gpui_flow_core::Viewport,
) -> CanvasBackdrop {
    CanvasBackdrop {
        color: gpui_flow_scene::Color::hex(theme.background),
        grid: show_grid.then_some(CanvasGrid {
            color: gpui_flow_scene::Color::hex(theme.grid),
            spacing: (24. * viewport.zoom).max(12.),
            offset: viewport.pan,
        }),
    }
}
#[cfg(test)]
fn state_render_objects(
    editor: &SceneEditor,
    router: &gpui_flow_interaction::scene::SceneEventRouter,
    styles: &mut gpui_flow_interaction::scene::SceneStateStyles,
    now: std::time::Duration,
) -> (Vec<RenderObject>, Option<String>) {
    let (frame, error) = state_render_frame(editor, router, styles, now);
    (frame.objects, error)
}
impl Render for SceneView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.reconcile_text_editor(window, cx);
        self.subscribe_input(window, cx);
        self.sync_requested_focus(window, cx);
        if let Some(resources) = &mut self.resources {
            match resources.prepare(&mut self.editor.engine, window, cx) {
                Ok(events) => {
                    self.resource_error = None;
                    for event in events {
                        cx.emit(SceneViewEvent::Resource(event));
                    }
                    if let Some(event) = resources.layout_ready(&self.editor.engine) {
                        cx.emit(SceneViewEvent::ResourceView(event));
                    }
                }
                Err(error) => self.resource_error = Some(error.to_string()),
            }
        }
        let resource_frame = self
            .resources
            .as_ref()
            .and_then(|resources| resources.frame(&self.editor.engine));
        let diagnostic_entity = cx.entity().downgrade();
        let diagnostic_window = window.window_handle().window_id().as_u64();
        window.on_next_frame(move |_, cx| {
            let issues = crate::display::gradient_render_issues(diagnostic_window);
            let _ = diagnostic_entity.update(cx, |this, cx| {
                let issues = issues
                    .into_iter()
                    .filter(|issue| {
                        this.editor
                            .scene()
                            .get(&issue.object.clone().into())
                            .is_some()
                    })
                    .collect::<Vec<_>>();
                if this.gradient_diagnostics != issues {
                    this.gradient_diagnostics = issues;
                    cx.notify();
                }
            });
        });
        let theme = self.theme;
        let viewport = self.editor.viewport;
        let show_grid = self.show_grid;
        let owned_canvas = self.compositing_mode == SceneCompositing::OwnedCanvas;
        let entity = cx.entity().downgrade();
        let background = canvas(
            move |bounds, window, cx| {
                let actual = super::composite::geometry(bounds, viewport, window);
                let changed = entity
                    .update(cx, |this, _| {
                        this.origin =
                            WorldPoint::new(bounds.origin.x.into(), bounds.origin.y.into());
                        this.extent =
                            WorldSize::new(bounds.size.width.into(), bounds.size.height.into());
                        let changed = this.composite_geometry != Some(actual);
                        this.composite_geometry = Some(actual);
                        changed
                    })
                    .unwrap_or(false);
                if changed {
                    let entity = entity.clone();
                    window.on_next_frame(move |_, cx| {
                        let _ = entity.update(cx, |_, cx| cx.notify());
                    });
                }
            },
            move |bounds, _, window, _| {
                if show_grid && !owned_canvas {
                    let gap = (24. * viewport.zoom).max(12.);
                    let w: f32 = bounds.size.width.into();
                    let h: f32 = bounds.size.height.into();
                    let mut x = viewport.pan.x.rem_euclid(gap);
                    while x < w {
                        let mut y = viewport.pan.y.rem_euclid(gap);
                        while y < h {
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin + point(px(x), px(y)),
                                    size(px(1.), px(1.)),
                                ),
                                rgb(theme.grid),
                            ));
                            y += gap;
                        }
                        x += gap;
                    }
                }
            },
        )
        .absolute()
        .size_full();
        self.diagnostics.clear();
        let (frame, state_error) = state_render_frame_with_canvas(
            &self.editor,
            &self.router,
            &mut self.state_styles,
            self.state_clock.elapsed(),
            owned_canvas.then(|| canvas_backdrop(theme, show_grid, viewport)),
        );
        self.state_style_error = state_error;
        let domains = frame
            .units
            .iter()
            .filter_map(|u| match u {
                SceneUnit::Domain(d) => Some((**d).clone()),
                _ => None,
            })
            .collect();
        let now = self.state_clock.elapsed();
        let active = window.is_visible() && window.is_window_active() && !cx.reduce_motion();
        let visible = animation_images(&frame, &self.compositing);
        if let Some(resources) = &mut self.resources {
            resources.sample_images(&self.editor.engine, &visible, now, active);
        }
        let composite_key = self.request_composites(domains, window, cx);
        // Semantic recovery can clear an upload/domain error without starting a
        // job (an unchanged accepted frame is reusable). Re-evaluate after the
        // reset so playback wakes even when no worker completion will notify us.
        let visible = animation_images(&frame, &self.compositing);
        if let Some(resources) = &mut self.resources
            && resources.sample_images(&self.editor.engine, &visible, now, active)
        {
            window.request_animation_frame();
        }
        self.composite_diagnostics = frame
            .units
            .iter()
            .filter_map(|u| match u {
                SceneUnit::Rejected(e) => Some(e.clone()),
                _ => None,
            })
            .collect();
        self.composite_diagnostics
            .extend(self.compositing.diagnostics());
        self.compositing_pending = self.compositing.pending();
        if self.state_style_error.is_none() && self.state_styles.has_active_transitions() {
            let entity = cx.entity().downgrade();
            window.on_next_frame(move |_, cx| {
                let _ = entity.update(cx, |_, cx| cx.notify());
            });
        }
        let corners: Vec<_> = self
            .editor
            .selection
            .ids
            .iter()
            .filter(|_| !self.text_editor_active())
            .filter_map(|id| self.selection_corners(id))
            .collect();
        let marquee = self.interactions.marquee();
        let resize_handles = self.resize_handles().map(|(_, handles)| handles);
        let overlay = canvas(
            |_, _, _| (),
            move |bounds, _, window, _| {
                for corners in corners {
                    let mut path = PathBuilder::stroke(px(1.5));
                    let p = |p: WorldPoint| bounds.origin + point(px(p.x), px(p.y));
                    path.move_to(p(corners[0]));
                    for corner in &corners[1..] {
                        path.line_to(p(*corner));
                    }
                    path.close();
                    if let Ok(path) = path.build() {
                        window.paint_path(path, rgb(theme.accent));
                    }
                }
                if let Some(handles) = resize_handles {
                    let side = gpui_flow_interaction::scene::RESIZE_HANDLE_PAINT_SIZE;
                    for handle in handles {
                        window.paint_quad(fill(
                            Bounds::new(
                                bounds.origin
                                    + point(
                                        px(handle.position.x - side / 2.),
                                        px(handle.position.y - side / 2.),
                                    ),
                                size(px(side), px(side)),
                            ),
                            rgb(theme.accent),
                        ));
                    }
                }
                if let Some(rect) = marquee {
                    let r = viewport.world_rect_to_screen(rect);
                    window.paint_quad(fill(
                        Bounds::new(
                            bounds.origin + point(px(r.origin.x), px(r.origin.y)),
                            size(px(r.size.width), px(r.size.height)),
                        ),
                        rgb(theme.accent).alpha(0.15),
                    ));
                }
            },
        )
        .absolute()
        .size_full();
        let mut view = div()
            .id("retained-scene")
            .relative()
            .size_full()
            .overflow_hidden()
            .when(!owned_canvas, |view| view.bg(rgb(theme.background)))
            .track_focus(&self.focus)
            .child(background);
        let objects = frame
            .objects
            .into_iter()
            .map(|o| (o.object.id.clone(), o))
            .collect::<std::collections::BTreeMap<_, _>>();
        for unit in frame.units {
            let (id, phase) = match unit {
                SceneUnit::Native(id) => (id, gpui_flow_scene::PaintPhase::All),
                SceneUnit::NativePhase { id, phase } => (id, phase),
                SceneUnit::Domain(domain) => {
                    if let Some(key) = &composite_key {
                        view = view.child(self.composite_element(
                            domain.root_id().clone(),
                            key.clone(),
                            cx,
                        ));
                    }
                    continue;
                }
                SceneUnit::Rejected(_) => continue,
            };
            let Some(object) = objects.get(&id).cloned() else {
                continue;
            };
            if phase != gpui_flow_scene::PaintPhase::Stroke {
                self.diagnostics.extend(object.diagnostics());
            }
            if phase != gpui_flow_scene::PaintPhase::All {
                view = view.child(DisplayElement::new(object).with_paint_phase(phase));
                continue;
            }
            if let gpui_flow_scene::Shape::Custom(custom) = &object.object.shape
                && let Some(renderer) = self.renderers.get(&custom.kind)
            {
                view = view.child(renderer.render(&object, window, cx));
            } else {
                let image = if matches!(object.object.shape, gpui_flow_scene::Shape::Image(_)) {
                    Some(
                        self.editor
                            .engine
                            .handle(&object.object.id)
                            .and_then(|handle| {
                                self.resources
                                    .as_ref()
                                    .map(|resources| resources.image(&self.editor.engine, handle))
                            })
                            .unwrap_or(crate::resources::SceneImagePresentation::Cancelled),
                    )
                } else {
                    None
                };
                let selected_frame = self
                    .editor
                    .engine
                    .handle(&object.object.id)
                    .and_then(|handle| {
                        self.resources
                            .as_ref()
                            .map(|r| r.image_frame(&self.editor.engine, handle))
                    })
                    .unwrap_or(0);
                let element = DisplayElement::new(object);
                view = match image {
                    Some(image) => {
                        view.child(element.with_image(image).selected_frame(selected_frame))
                    }
                    None => view.child(element),
                };
            }
        }
        view = view.child(overlay);
        if let Some(frame) = resource_frame {
            let entity = cx.entity().downgrade();
            // This callback is a real GPUI paint boundary, not a timer that
            // guesses rendering succeeded. Defer entity notification until the
            // current effect cycle has returned all borrowed entities.
            view = view.child(
                canvas(
                    |_, _, _| (),
                    move |_, _, _, cx| {
                        cx.defer(move |cx| {
                            let _ = entity.update(cx, |this, cx| {
                                if let Some(resources) = &mut this.resources {
                                    for event in resources.frame_drawn(&this.editor.engine, frame) {
                                        cx.emit(SceneViewEvent::ResourceView(event));
                                    }
                                }
                            });
                        });
                    },
                )
                .absolute()
                .size_full(),
            );
        }
        if self.compositing.loading_visible() || !self.composite_diagnostics.is_empty() {
            let message = self
                .composite_diagnostics
                .first()
                .map(|e| {
                    format!(
                        "Composite {}: {}",
                        if e.object.as_str().is_empty() {
                            "canvas"
                        } else {
                            e.object.as_str()
                        },
                        e.error
                    )
                })
                .unwrap_or_else(|| "Compositing…".into());
            view = view.child(
                div()
                    .absolute()
                    .top_2()
                    .left_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(theme.panel))
                    .text_color(rgb(0xf5b971))
                    .text_xs()
                    .child(message),
            );
        }
        if !self.diagnostics.is_empty() || !self.gradient_diagnostics.is_empty() {
            view = view.child(
                div()
                    .absolute()
                    .top_2()
                    .right_2()
                    .px_3()
                    .py_2()
                    .rounded_md()
                    .bg(rgb(theme.panel))
                    .text_color(rgb(0xf5b971))
                    .text_xs()
                    .child(format!(
                        "{} unsupported features · {} gradient errors{}",
                        self.diagnostics.len(),
                        self.gradient_diagnostics.len(),
                        self.gradient_diagnostics
                            .first()
                            .map(|e| format!(" · {}: {}", e.object, e.message))
                            .unwrap_or_default()
                    )),
            );
        }
        if let Some(error) = self
            .state_style_error
            .as_ref()
            .or(self.interaction_error.as_ref())
            .or(self.resource_error.as_ref())
        {
            view = view.child(
                div()
                    .absolute()
                    .bottom_2()
                    .left_2()
                    .p_2()
                    .bg(rgb(theme.panel))
                    .text_color(rgb(0xf5b971))
                    .text_xs()
                    .child(error.clone()),
            );
        }
        if let Some(panel) = self.text_editor_panel(cx) {
            view = view.child(panel);
        }
        view.cursor(self.input_cursor())
            .on_hover(cx.listener(|this, hovered, _, cx| {
                if !hovered {
                    this.pointer_leave(cx);
                }
            }))
            .on_mouse_down(MouseButton::Left, cx.listener(Self::down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::down))
            .on_mouse_down(MouseButton::Right, cx.listener(Self::down))
            .on_mouse_move(cx.listener(|this, event, _, cx| this.moved(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event, _, cx| this.up(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel_input(cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, event, _, cx| this.up(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(|this, _, _, cx| this.cancel_input(cx)),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, event, _, cx| this.up(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| this.cancel_input(cx)),
            )
            .on_scroll_wheel(cx.listener(|this, event, _, cx| this.scroll(event, cx)))
            .on_key_down(cx.listener(|this, event, window, cx| {
                if this.focus.is_focused(window) {
                    this.key(event, cx);
                }
            }))
            .on_key_up(cx.listener(|this, event, window, cx| {
                if this.focus.is_focused(window) {
                    this.key_up(event, cx);
                }
            }))
    }
}

#[cfg(test)]
mod state_tests {
    #[::core::prelude::v1::test]
    fn rejected_native_image_is_not_an_animation_driver() {
        let mut object = crate::display::RenderObject {
            object: gpui_flow_scene::DisplayObject::new(
                "image",
                gpui_flow_scene::display::Image::new(
                    "animated.gif",
                    gpui_flow_scene::Size::new(20., 20.),
                ),
            ),
            screen_transform: gpui_flow_scene::Affine::IDENTITY,
            viewport_transform: gpui_flow_scene::Affine::IDENTITY,
            opacity: 1.,
            clips: vec![],
        };
        let controller = crate::display::composite::CompositeController::default();
        let mut frame = super::StateFrame {
            objects: vec![object.clone()],
            units: vec![gpui_flow_composite::SceneUnit::Native("image".into())],
        };
        assert!(super::animation_images(&frame, &controller).contains(&"image".into()));
        object.screen_transform.b = 0.5;
        frame.objects = vec![object];
        assert!(super::animation_images(&frame, &controller).is_empty());
    }

    use super::state_render_objects;
    use gpui_flow_editor::SceneEditor;
    use gpui_flow_interaction::{CanvasEvent, HitTarget, PointerInput, scene::*};
    use gpui_flow_scene::{Color, DisplayObject, Point, Scene, display};
    #[test]
    fn invalid_overlay_reports_error_and_draws_canonical_then_recovers() {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("a", display::Rect::new(40., 30.)),
                None,
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        let handle = editor.engine.handle(&"a".into()).unwrap();
        let mut sheet = SceneStateStyles::new(&editor.engine);
        sheet
            .bind(
                &editor.engine,
                handle,
                StateStyles {
                    hover: StatePaint::fill(Color::WHITE),
                    ..Default::default()
                },
            )
            .unwrap();
        let mut router = SceneEventRouter::new();
        let mut host = ScenePluginHost::new();
        router.dispatch(
            &mut editor,
            &mut host,
            CanvasEvent::PointerMove(PointerInput::left(
                Point::new(10., 10.),
                HitTarget::Object("a".into()),
            )),
        );
        editor
            .engine
            .transaction(|scene| {
                scene.update_object(&"a".into(), |object| {
                    object.style.fill = None;
                    object.style.fill_layers = vec![Color::BLACK.into(); 32];
                })
            })
            .unwrap();
        let canonical = editor.scene().clone();
        let revision = editor.engine.revision();
        let (objects, error) =
            state_render_objects(&editor, &router, &mut sheet, std::time::Duration::ZERO);
        assert!(error.unwrap().contains("paint layers"));
        assert_eq!(objects.len(), 1);
        assert_eq!(
            &objects[0].object,
            &canonical.get(&"a".into()).unwrap().object
        );
        assert_eq!(editor.scene(), &canonical);
        assert_eq!(editor.engine.revision(), revision);
        sheet.unbind(handle);
        let (objects, error) =
            state_render_objects(&editor, &router, &mut sheet, std::time::Duration::ZERO);
        assert!(error.is_none());
        assert_eq!(objects.len(), 1);
    }
    #[test]
    fn transition_frames_render_interpolated_values_and_stop_requesting_motion() {
        use std::time::Duration;
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("a", display::Rect::new(40., 30.))
                    .with_style(gpui_flow_scene::Style::fill(Color::BLACK)),
                None,
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        let handle = editor.engine.handle(&"a".into()).unwrap();
        let mut sheet = SceneStateStyles::new(&editor.engine);
        sheet
            .bind(
                &editor.engine,
                handle,
                StateStyles {
                    hover: StatePaint {
                        fill: Some(Color::WHITE),
                        opacity: Some(0.5),
                    },
                    ..Default::default()
                },
            )
            .unwrap();
        sheet
            .set_transitions(
                &editor.engine,
                handle,
                StateTransitions {
                    transition: StateTransition::new(100),
                    transition_out: None,
                },
            )
            .unwrap();
        let mut router = SceneEventRouter::new();
        let mut host = ScenePluginHost::new();
        router.dispatch(
            &mut editor,
            &mut host,
            CanvasEvent::PointerMove(PointerInput::left(
                Point::new(10., 10.),
                HitTarget::Object("a".into()),
            )),
        );
        let (_, error) = state_render_objects(&editor, &router, &mut sheet, Duration::ZERO);
        assert!(error.is_none());
        assert!(sheet.has_active_transitions());
        let (objects, error) =
            state_render_objects(&editor, &router, &mut sheet, Duration::from_millis(50));
        assert!(error.is_none());
        assert_eq!(objects[0].object.style.opacity, 0.75);
        assert_eq!(
            objects[0].object.style.fill,
            Some(Color::rgb(128, 128, 128).into())
        );
        let (objects, error) =
            state_render_objects(&editor, &router, &mut sheet, Duration::from_millis(100));
        assert!(error.is_none());
        assert_eq!(objects[0].object.style.opacity, 0.5);
        assert!(!sheet.has_active_transitions()); // SceneView's next-frame request gate is now false.
        state_render_objects(&editor, &router, &mut sheet, Duration::from_millis(1000));
        assert!(!sheet.has_active_transitions());
    }

    #[test]
    fn presented_group_opacity_replaces_whole_subtree_without_native_children() {
        use gpui_flow_composite::SceneUnit;
        let mut s = Scene::new();
        s.insert(DisplayObject::new("g", display::Group::new()), None, None)
            .unwrap();
        s.insert(
            DisplayObject::new("r", display::Rect::new(20., 20.)),
            Some("g".into()),
            None,
        )
        .unwrap();
        let mut editor = SceneEditor::new(s).unwrap();
        let h = editor.engine.handle(&"g".into()).unwrap();
        let mut styles = SceneStateStyles::new(&editor.engine);
        styles
            .bind(
                &editor.engine,
                h,
                StateStyles {
                    hover: StatePaint {
                        fill: None,
                        opacity: Some(0.5),
                    },
                    ..Default::default()
                },
            )
            .unwrap();
        let mut router = SceneEventRouter::new();
        router.dispatch(
            &mut editor,
            &mut ScenePluginHost::new(),
            CanvasEvent::PointerMove(PointerInput::left(
                Point::new(5., 5.),
                HitTarget::Object("g".into()),
            )),
        );
        let (frame, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        assert_eq!(frame.units.len(), 1);
        assert!(matches!(&frame.units[0],SceneUnit::Domain(d) if d.root_id().as_str()=="g"));
        assert_eq!(
            editor
                .scene()
                .get(&"g".into())
                .unwrap()
                .object
                .style
                .opacity,
            1.
        );
    }
    #[test]
    fn opacity_on_frame_produces_one_complete_domain() {
        use gpui_flow_composite::SceneUnit;
        let mut s = Scene::new();
        let mut f = DisplayObject::new("f", display::Frame::new(100., 100.));
        f.style.opacity = 0.5;
        s.insert(f, None, None).unwrap();
        s.insert(
            DisplayObject::new("g", display::Group::new()).with_style(gpui_flow_scene::Style {
                blend_mode: gpui_flow_scene::BlendMode::Normal,
                ..Default::default()
            }),
            Some("f".into()),
            None,
        )
        .unwrap();
        let e = SceneEditor::new(s).unwrap();
        let mut styles = SceneStateStyles::new(&e.engine);
        let (frame, error) = super::state_render_frame(
            &e,
            &SceneEventRouter::new(),
            &mut styles,
            std::time::Duration::ZERO,
        );
        assert!(error.is_none());
        assert!(matches!(
            &frame.units[..],
            [SceneUnit::Domain(d)] if d.root_id().as_str() == "f" && d.ids().len() == 2
        ));
    }
    fn raster_container(frame: &super::StateFrame) -> gpui_flow_composite::CpuFrame {
        use gpui_flow_composite::*;
        use gpui_flow_scene::{Affine, Bounds, Size};
        let [SceneUnit::Domain(d)] = frame.units.as_slice() else {
            panic!("one domain expected")
        };
        let bounds = Bounds::new(Point::ZERO, Size::new(100., 100.));
        rasterize(
            d,
            RasterViewport {
                world_to_screen: Affine::IDENTITY,
                canvas_bounds: bounds,
                content_mask: bounds,
                device_scale: 1.,
            },
            &mut PixelBudget::new(CompositeLimits::default(), 0).unwrap(),
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap()
        .unwrap()
    }
    #[test]
    fn presented_container_inherits_hover_and_keeps_canonical_hit_and_focus() {
        use gpui_flow_scene::{Shape, Style};
        for shape in [
            Shape::Box(display::Box::new(30., 30.)),
            Shape::Frame(display::Frame::new(30., 30.).rounded(8.)),
        ] {
            let mut s = Scene::new();
            s.insert(
                DisplayObject::new("container", shape).with_style(Style::fill(Color::BLACK)),
                None,
                None,
            )
            .unwrap();
            s.insert(
                DisplayObject::new("child", display::Rect::new(50., 50.))
                    .with_style(Style::fill(Color::WHITE)),
                Some("container".into()),
                None,
            )
            .unwrap();
            let mut editor = SceneEditor::new(s).unwrap();
            let canonical = editor.scene().clone();
            let h = editor.engine.handle(&"container".into()).unwrap();
            let child = editor.engine.handle(&"child".into()).unwrap();
            let mut styles = SceneStateStyles::new(&editor.engine);
            styles
                .bind(
                    &editor.engine,
                    h,
                    StateStyles {
                        hover: StatePaint {
                            fill: None,
                            opacity: Some(0.5),
                        },
                        ..Default::default()
                    },
                )
                .unwrap();
            let mut router = SceneEventRouter::new();
            let mut host = ScenePluginHost::new();
            router.dispatch(
                &mut editor,
                &mut host,
                CanvasEvent::PointerMove(PointerInput::left(
                    Point::new(15., 15.),
                    HitTarget::Object("child".into()),
                )),
            );
            router
                .set_focus(&mut editor, &mut host, Some(child))
                .unwrap();
            let hit = editor
                .scene()
                .hit_test(Point::new(15., 15.), Default::default())
                .unwrap();
            let (frame, error) =
                super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
            assert!(error.is_none());
            assert_eq!(frame.units.len(), 1);
            let pixels = raster_container(&frame);
            assert!(pixels.premul_rgba.as_chunks::<4>().0.contains(&[128; 4]));
            assert_eq!(editor.scene(), &canonical);
            assert_eq!(router.focused(&editor.engine), Some(child));
            assert_eq!(
                editor
                    .scene()
                    .hit_test(Point::new(15., 15.), Default::default())
                    .unwrap(),
                hit
            );
        }
    }
    #[test]
    fn shadow_property_edit_undo_and_hover_share_the_presented_scene_domain() {
        use gpui_flow_scene::{Affine, Shadow, Style};
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("shadow", display::Rect::new(10., 10.))
                    .with_transform(Affine::translation(20., 20.))
                    .with_style(Style {
                        shadow: Some(Shadow::new(20., 0., 2., Color::BLACK)),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        let mut styles = SceneStateStyles::new(&editor.engine);
        let router = SceneEventRouter::new();
        let (before, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        assert!(matches!(
            &before.units[..],
            [gpui_flow_composite::SceneUnit::Domain(_)]
        ));
        let original = raster_container(&before);
        let geometry = editor.engine.world_bounds(&"shadow".into()).unwrap();
        editor
            .transaction(|s| {
                s.update_object(&"shadow".into(), |o| {
                    o.style.shadow.as_mut().unwrap().x = 40.
                })
            })
            .unwrap();
        let (after, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        assert_ne!(
            raster_container(&after).screen_bounds,
            original.screen_bounds
        );
        assert_eq!(
            editor.engine.world_bounds(&"shadow".into()).unwrap(),
            geometry
        );
        assert!(
            editor
                .engine
                .hit_test(Point::new(45., 25.), Default::default())
                .unwrap()
                .is_none()
        );
        assert!(editor.undo());
        let (restored, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        assert_eq!(restored.units, before.units);
        let restored = raster_container(&restored);
        assert_eq!(restored.screen_bounds, original.screen_bounds);
        assert_eq!(restored.premul_rgba, original.premul_rgba);
        // State overlay preserves the canonical shadow while changing its alpha source.
        let object = &editor.scene().get(&"shadow".into()).unwrap().object;
        let next = gpui_flow_interaction::scene::StateStyles {
            hover: gpui_flow_interaction::scene::StatePaint::fill(Color::TRANSPARENT),
            ..Default::default()
        }
        .resolve(
            &object.style,
            gpui_flow_interaction::scene::VisualState {
                hover: true,
                ..Default::default()
            },
        );
        assert_eq!(next.shadow, object.style.shadow);
        assert_eq!(next.fill, Some(Color::TRANSPARENT.into()));
    }
    #[test]
    fn shadow_layers_edit_undo_and_hover_share_the_presented_scene_domain() {
        use gpui_flow_scene::{Affine, Shadow, Style};
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("shadow", display::Rect::new(10., 10.))
                    .with_transform(Affine::translation(20., 20.))
                    .with_style(Style {
                        shadow: Some(Shadow::new(20., 0., 2., Color::BLACK)),
                        shadow_layers: vec![Shadow::new(0., 20., 0., Color::rgba(255, 0, 0, 128))],
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        let mut styles = SceneStateStyles::new(&editor.engine);
        let router = SceneEventRouter::new();
        let (before, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        assert!(matches!(
            &before.units[..],
            [gpui_flow_composite::SceneUnit::Domain(_)]
        ));
        let original = raster_container(&before);
        let geometry = editor.engine.world_bounds(&"shadow".into()).unwrap();
        editor
            .transaction(|s| {
                s.update_object(&"shadow".into(), |o| {
                    o.style
                        .shadow_layers
                        .push(Shadow::new(40., 0., 2., Color::BLACK))
                })
            })
            .unwrap();
        let (after, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        assert_ne!(
            raster_container(&after).screen_bounds,
            original.screen_bounds
        );
        assert_eq!(
            editor.engine.world_bounds(&"shadow".into()).unwrap(),
            geometry
        );
        assert!(
            editor
                .engine
                .hit_test(Point::new(45., 25.), Default::default())
                .unwrap()
                .is_none()
        );
        assert!(editor.undo());
        let (restored, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        assert_eq!(restored.units, before.units);
        let restored = raster_container(&restored);
        assert_eq!(restored.screen_bounds, original.screen_bounds);
        assert_eq!(restored.premul_rgba, original.premul_rgba);
        // State overlay preserves the canonical shadow while changing its alpha source.
        let object = &editor.scene().get(&"shadow".into()).unwrap().object;
        let next = gpui_flow_interaction::scene::StateStyles {
            hover: gpui_flow_interaction::scene::StatePaint::fill(Color::TRANSPARENT),
            ..Default::default()
        }
        .resolve(
            &object.style,
            gpui_flow_interaction::scene::VisualState {
                hover: true,
                ..Default::default()
            },
        );
        assert_eq!(next.shadow, object.style.shadow);
        assert_eq!(next.shadow_layers, object.style.shadow_layers);
        assert_eq!(next.fill, Some(Color::TRANSPARENT.into()));
    }
    #[test]
    fn inner_layers_edit_undo_and_state_preserve_geometry_and_pixels() {
        use gpui_flow_scene::{Affine, Shadow, Style};
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("r", display::Rect::new(20., 20.))
                    .with_transform(Affine::translation(20., 20.))
                    .with_style(Style {
                        fill: Some(Color::rgb(0, 0, 255).into()),
                        inner_shadow_layers: vec![Shadow::new(2., 0., 0., Color::rgb(255, 0, 0))],
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        let mut styles = SceneStateStyles::new(&editor.engine);
        let router = SceneEventRouter::new();
        let (before, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        let old = raster_container(&before);
        let geometry = editor.engine.world_bounds(&"r".into()).unwrap();
        editor
            .transaction(|s| {
                s.update_object(&"r".into(), |o| o.style.inner_shadow_layers[0].x = 10.)
            })
            .unwrap();
        let (after, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        let new = raster_container(&after);
        assert_eq!(new.screen_bounds, old.screen_bounds);
        assert_ne!(new.premul_rgba, old.premul_rgba);
        assert_eq!(editor.engine.world_bounds(&"r".into()).unwrap(), geometry);
        assert!(editor.undo());
        let (restored, _) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert_eq!(raster_container(&restored).premul_rgba, old.premul_rgba);
        let object = &editor.scene().get(&"r".into()).unwrap().object;
        let next = gpui_flow_interaction::scene::StateStyles {
            hover: gpui_flow_interaction::scene::StatePaint::fill(Color::TRANSPARENT),
            ..Default::default()
        }
        .resolve(
            &object.style,
            gpui_flow_interaction::scene::VisualState {
                hover: true,
                ..Default::default()
            },
        );
        assert_eq!(next.inner_shadow_layers, object.style.inner_shadow_layers);
        assert_eq!(next.fill, Some(Color::TRANSPARENT.into()));
    }
    #[test]
    fn container_clip_edit_and_undo_restore_the_actual_composite_pixels() {
        use gpui_flow_scene::{Shape, Style};
        let mut s = Scene::new();
        s.insert(
            DisplayObject::new("container", display::Frame::new(20., 20.).rounded(6.)).with_style(
                Style {
                    fill: None,
                    opacity: 0.5,
                    ..Default::default()
                },
            ),
            None,
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new("child", display::Rect::new(40., 40.))
                .with_style(Style::fill(Color::WHITE)),
            Some("container".into()),
            None,
        )
        .unwrap();
        let mut editor = SceneEditor::new(s).unwrap();
        let mut styles = SceneStateStyles::new(&editor.engine);
        let router = SceneEventRouter::new();
        let (before, _) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        let original = raster_container(&before);
        editor
            .transaction(|s| {
                s.update_object(&"container".into(), |o| {
                    if let Shape::Frame(f) = &mut o.shape {
                        f.clip_children = false;
                    }
                })
            })
            .unwrap();
        let (after, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        assert!(raster_container(&after).width > original.width);
        assert!(editor.undo());
        let (restored, error) =
            super::state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(error.is_none());
        assert_eq!(restored.units, before.units);
        let restored = raster_container(&restored);
        assert_eq!(restored.screen_bounds, original.screen_bounds);
        assert_eq!(restored.premul_rgba, original.premul_rgba);
    }
    #[test]
    fn container_phase_edit_and_undo_share_native_and_presented_domain_planning() {
        use gpui_flow_composite::SceneUnit;
        use gpui_flow_scene::{ContainerPaintOrder, PaintPhase, Shape, Stroke, Style};
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("frame", display::Frame::new(30., 30.)).with_style(Style {
                    fill: Some(Color::BLACK.into()),
                    stroke: Some(Stroke::new(Color::WHITE, 8.)),
                    ..Default::default()
                }),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new("child", display::Rect::new(20., 20.))
                    .with_style(Style::fill(Color::rgb(0, 0, 255))),
                Some("frame".into()),
                None,
            )
            .unwrap();
        let mut editor = SceneEditor::new(scene).unwrap();
        let mut styles = SceneStateStyles::new(&editor.engine);
        let router = SceneEventRouter::new();
        let snapshot = |editor: &SceneEditor, styles: &mut SceneStateStyles| {
            let (frame, error) =
                super::state_render_frame(editor, &router, styles, std::time::Duration::ZERO);
            assert!(error.is_none());
            frame
        };
        let before = snapshot(&editor, &mut styles);
        assert_eq!(
            before.units,
            vec![
                SceneUnit::Native("frame".into()),
                SceneUnit::Native("child".into())
            ]
        );
        editor
            .transaction(|s| {
                s.update_object(&"frame".into(), |o| {
                    let Shape::Frame(f) = &mut o.shape else {
                        unreachable!()
                    };
                    f.paint_order = ContainerPaintOrder::StrokeAfterChildren;
                })
            })
            .unwrap();
        let after = snapshot(&editor, &mut styles);
        assert_eq!(after.objects.len(), 2);
        assert_eq!(
            after.units,
            vec![
                SceneUnit::NativePhase {
                    id: "frame".into(),
                    phase: PaintPhase::Fill
                },
                SceneUnit::Native("child".into()),
                SceneUnit::NativePhase {
                    id: "frame".into(),
                    phase: PaintPhase::Stroke
                }
            ]
        );
        editor
            .transaction(|s| s.update_object(&"frame".into(), |o| o.style.opacity = 0.5))
            .unwrap();
        let domain = snapshot(&editor, &mut styles);
        assert!(matches!(domain.units.as_slice(), [SceneUnit::Domain(_)]));
        let frame = raster_container(&domain);
        let x = (2. - frame.screen_bounds.origin.x) as usize;
        let y = (15. - frame.screen_bounds.origin.y) as usize;
        assert_eq!(
            &frame.premul_rgba[(y * frame.width as usize + x) * 4..][..4],
            &[128; 4]
        );
        assert!(editor.undo());
        assert_eq!(snapshot(&editor, &mut styles).units, after.units);
        assert!(editor.undo());
        assert_eq!(snapshot(&editor, &mut styles).units, before.units);
        assert!(editor.redo());
        assert_eq!(snapshot(&editor, &mut styles).units, after.units);
    }
}

#[cfg(test)]
mod owned_canvas_tests {
    use super::*;
    use core::prelude::v1::test;
    use gpui_flow_scene::{BlendMode, Color, DisplayObject, Style, display};
    #[test]
    fn owned_mode_captures_one_complete_presented_tree_without_native_siblings() {
        let mut scene = Scene::new();
        for (id, blend) in [
            ("behind", BlendMode::PassThrough),
            ("blend", BlendMode::Multiply),
        ] {
            scene
                .insert(
                    DisplayObject::new(id, display::Rect::new(20., 20.)).with_style(Style {
                        fill: Some(Color::WHITE.into()),
                        blend_mode: blend,
                        ..Default::default()
                    }),
                    None,
                    None,
                )
                .unwrap();
        }
        let editor = SceneEditor::new(scene.clone()).unwrap();
        let router = gpui_flow_interaction::scene::SceneEventRouter::new();
        let mut styles = gpui_flow_interaction::scene::SceneStateStyles::new(&editor.engine);
        let (native, _) =
            state_render_frame(&editor, &router, &mut styles, std::time::Duration::ZERO);
        assert!(
            native
                .units
                .iter()
                .any(|u| matches!(u, SceneUnit::Rejected(_)))
        );
        let (owned, error) = state_render_frame_with_canvas(
            &editor,
            &router,
            &mut styles,
            std::time::Duration::ZERO,
            Some(canvas_backdrop(FlowTheme::default(), true, editor.viewport)),
        );
        assert!(error.is_none());
        assert_eq!(owned.units.len(), 1);
        let SceneUnit::Domain(domain) = &owned.units[0] else {
            panic!("whole canvas missing")
        };
        assert!(domain.is_owned_canvas());
        assert_eq!(
            domain
                .ids()
                .into_iter()
                .map(DisplayId::as_str)
                .collect::<Vec<_>>(),
            vec!["behind", "blend"]
        );
        assert_eq!(editor.scene(), &scene);
        assert_eq!(SceneCompositing::default(), SceneCompositing::Domains);
    }
    #[test]
    fn canvas_grid_key_tracks_theme_pan_zoom_and_grid_visibility() {
        let mut theme = FlowTheme::default();
        let mut view = gpui_flow_core::Viewport {
            pan: WorldPoint::new(-13., 27.),
            zoom: 0.25,
            ..Default::default()
        };
        let a = canvas_backdrop(theme, true, view);
        assert_eq!(a.grid.unwrap().spacing, 12.);
        assert_eq!(a.grid.unwrap().offset, view.pan);
        view.zoom = 2.;
        let b = canvas_backdrop(theme, true, view);
        assert_eq!(b.grid.unwrap().spacing, 48.);
        assert_ne!(a, b);
        theme.background ^= 0xffffff;
        assert_ne!(b, canvas_backdrop(theme, true, view));
        assert!(canvas_backdrop(theme, false, view).grid.is_none());
    }
    #[test]
    fn visible_custom_rejects_entire_owned_canvas_instead_of_native_fallback() {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new(
                    "native",
                    display::Custom::new(
                        "host-control",
                        gpui_flow_scene::Bounds::new(WorldPoint::ZERO, WorldSize::new(20., 20.)),
                        vec![],
                    ),
                ),
                None,
                None,
            )
            .unwrap();
        let editor = SceneEditor::new(scene).unwrap();
        let router = gpui_flow_interaction::scene::SceneEventRouter::new();
        let mut styles = gpui_flow_interaction::scene::SceneStateStyles::new(&editor.engine);
        let (owned, error) = state_render_frame_with_canvas(
            &editor,
            &router,
            &mut styles,
            std::time::Duration::ZERO,
            Some(canvas_backdrop(
                FlowTheme::default(),
                false,
                editor.viewport,
            )),
        );
        assert!(error.is_none());
        assert!(matches!(
            &owned.units[..],
            [SceneUnit::Rejected(gpui_flow_composite::CompositeIssue {
                error: gpui_flow_composite::CompositeError::UnsupportedShape(_),
                ..
            })]
        ));
    }
}
