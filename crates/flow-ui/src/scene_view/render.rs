use super::*;
use crate::display::DisplayElement;
impl Render for SceneView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
        let objects = match self.editor.scene.paint_list() {
            Ok(items) => items
                .iter()
                .map(|item| RenderObject::from_paint_item(item, viewport))
                .collect::<Vec<_>>(),
            Err(_) => vec![],
        };
        let corners: Vec<_> = self
            .editor
            .selection
            .ids
            .iter()
            .filter_map(|id| self.selection_corners(id))
            .collect();
        let marquee = match self.gesture {
            Some(Gesture::Marquee { start, current }) => {
                Some(gpui_flow_core::Rect::from_points(start, current))
            }
            _ => None,
        };
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
                    window.paint_quad(fill(
                        Bounds::new(p(corners[2]) - point(px(4.), px(4.)), size(px(8.), px(8.))),
                        rgb(theme.accent),
                    ));
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
        if !self.diagnostics.is_empty() {
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
                        "{} unsupported rendering features · inspect SceneView.diagnostics",
                        self.diagnostics.len()
                    )),
            );
        }
        view.on_mouse_down(MouseButton::Left, cx.listener(Self::down))
            .on_mouse_down(MouseButton::Middle, cx.listener(Self::down))
            .on_mouse_move(cx.listener(|this, event, _, cx| this.moved(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event, _, cx| this.up(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, event, _, cx| this.up(event, cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, event, _, cx| this.up(event, cx)),
            )
            .on_scroll_wheel(cx.listener(|this, event: &ScrollWheelEvent, _, cx| {
                let delta = match event.delta {
                    ScrollDelta::Pixels(p) => WorldPoint::new(p.x.into(), p.y.into()),
                    ScrollDelta::Lines(p) => WorldPoint::new(p.x * 24., p.y * 24.),
                };
                if event.modifiers.control || event.modifiers.platform {
                    this.editor.viewport.zoom_at(
                        this.local(event.position),
                        this.editor.viewport.zoom * (-delta.y * 0.004).exp(),
                    );
                } else {
                    this.editor.viewport.pan_by(delta);
                }
                cx.notify();
            }))
            .on_key_down(cx.listener(|this, event, _, cx| this.key(event, cx)))
    }
}
