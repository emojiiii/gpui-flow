use super::*;
use crate::display::DisplayElement;
fn state_render_objects(
    editor: &SceneEditor,
    router: &gpui_flow_interaction::scene::SceneEventRouter,
    styles: &mut gpui_flow_interaction::scene::SceneStateStyles,
    now: std::time::Duration,
) -> (Vec<RenderObject>, Option<String>) {
    let convert = |items: Vec<gpui_flow_scene::PaintItem<'_>>| {
        items
            .iter()
            .map(|item| RenderObject::from_paint_item(item, editor.viewport))
            .collect::<Vec<_>>()
    };
    let canonical = || {
        editor
            .engine
            .paint_list()
            .map(convert)
            .map_err(|e| e.to_string())
    };
    let presented = styles
        .presentation_at(editor, router, now)
        .map_err(|e| e.to_string())
        .and_then(|scene| match scene {
            std::borrow::Cow::Owned(scene) => {
                scene.paint_list().map(convert).map_err(|e| e.to_string())
            }
            std::borrow::Cow::Borrowed(_) => canonical(),
        });
    match presented {
        Ok(objects) => (objects, None),
        Err(error) => match canonical() {
            Ok(objects) => (objects, Some(error)),
            Err(fallback_error) => (
                vec![],
                Some(format!("{error}; canonical render: {fallback_error}")),
            ),
        },
    }
}
impl Render for SceneView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.subscribe_input(window, cx);
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
        let entity = cx.entity().downgrade();
        let background = canvas(
            move |bounds, _, cx| {
                let _ = entity.update(cx, |this, _| {
                    this.origin = WorldPoint::new(bounds.origin.x.into(), bounds.origin.y.into());
                    this.extent =
                        WorldSize::new(bounds.size.width.into(), bounds.size.height.into());
                });
            },
            move |bounds, _, window, _| {
                if show_grid {
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
        let (objects, state_error) = state_render_objects(
            &self.editor,
            &self.router,
            &mut self.state_styles,
            self.state_clock.elapsed(),
        );
        self.state_style_error = state_error;
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
            .filter_map(|id| self.selection_corners(id))
            .collect();
        let marquee = self.interactions.marquee();
        let show_resize = self.resize_target().is_some();
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
                    if show_resize {
                        window.paint_quad(fill(
                            Bounds::new(
                                p(corners[2]) - point(px(4.), px(4.)),
                                size(px(8.), px(8.)),
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
            .bg(rgb(theme.background))
            .track_focus(&self.focus)
            .child(background);
        for object in objects {
            self.diagnostics.extend(object.diagnostics());
            if let gpui_flow_scene::Shape::Custom(custom) = &object.object.shape
                && let Some(renderer) = self.renderers.get(&custom.kind)
            {
                view = view.child(renderer.render(&object, window, cx));
            } else {
                view = view.child(DisplayElement::new(object));
            }
        }
        view = view.child(overlay);
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
}
