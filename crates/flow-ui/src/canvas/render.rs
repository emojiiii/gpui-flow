use super::*;
use gpui_flow_interaction::{CanvasCommand, HitTarget};

impl Render for FlowCanvas {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.subscribe_interaction_lifecycle(window, cx);
        self.sync_input(window, cx);
        self.sync_media(cx);
        let theme = self.theme;
        let viewport = self.editor.viewport;
        let grid = self.show_grid;
        let edge_renderer = self.edge_renderer.clone();
        let entity = cx.entity().downgrade();
        let edges: Vec<_> = self
            .editor
            .graph()
            .edges
            .values()
            .filter_map(|e| {
                Some((
                    self.editor.graph().port_position(&e.source)?,
                    self.editor.graph().port_position(&e.target)?,
                ))
            })
            .collect();
        let pending = self
            .interactions
            .connection()
            .preview(self.editor.graph(), viewport);
        let pending_color = if self.interactions.connection().hovered().is_some()
            && self.interactions.connection().target().is_none()
        {
            0xf27788
        } else {
            theme.accent
        };
        let marquee = self.interactions.marquee();
        let guides = self.interactions.guides().to_vec();
        let paint = canvas(
            move |bounds, _, cx| {
                let _ = entity.update(cx, |this, _| {
                    this.origin = model::Point::new(bounds.origin.x.into(), bounds.origin.y.into());
                    this.extent =
                        model::Size::new(bounds.size.width.into(), bounds.size.height.into());
                });
            },
            move |bounds, _, window, _| {
                let screen = |p: model::Point| {
                    let p = viewport.world_to_screen(p);
                    bounds.origin + point(px(p.x), px(p.y))
                };
                if grid {
                    let gap = (24. * viewport.zoom).max(12.);
                    let width: f32 = bounds.size.width.into();
                    let height: f32 = bounds.size.height.into();
                    let mut x = viewport.pan.x.rem_euclid(gap);
                    while x < width {
                        let mut y = viewport.pan.y.rem_euclid(gap);
                        while y < height {
                            window.paint_quad(fill(
                                Bounds::new(
                                    bounds.origin + point(px(x), px(y)),
                                    size(px(1.5), px(1.5)),
                                ),
                                rgb(theme.grid),
                            ));
                            y += gap;
                        }
                        x += gap;
                    }
                }
                for (from, to) in edges {
                    edge_renderer.paint(
                        screen(from),
                        screen(to),
                        rgb(theme.accent).alpha(0.55),
                        window,
                    );
                }
                if let Some((from, to)) = pending {
                    edge_renderer.paint(screen(from), screen(to), rgb(pending_color), window);
                }
                for guide in &guides {
                    let mut line = PathBuilder::stroke(px(1.));
                    match guide.axis {
                        gpui_flow_interaction::Axis::X => {
                            let x = screen(model::Point::new(guide.position, 0.)).x;
                            line.move_to(point(x, bounds.top()));
                            line.line_to(point(x, bounds.bottom()));
                        }
                        gpui_flow_interaction::Axis::Y => {
                            let y = screen(model::Point::new(0., guide.position)).y;
                            line.move_to(point(bounds.left(), y));
                            line.line_to(point(bounds.right(), y));
                        }
                    }
                    if let Ok(path) = line.build() {
                        window.paint_path(path, rgb(theme.accent).alpha(0.7));
                    }
                }
                if let Some(rect) = marquee {
                    let r = viewport.world_rect_to_screen(rect);
                    window.paint_quad(fill(
                        Bounds::new(
                            bounds.origin + point(px(r.origin.x), px(r.origin.y)),
                            size(px(r.size.width), px(r.size.height)),
                        ),
                        rgb(theme.accent).alpha(0.16),
                    ));
                }
            },
        )
        .absolute()
        .size_full();
        let mut surface = div()
            .id("flow-surface")
            .relative()
            .flex_1()
            .size_full()
            .overflow_hidden()
            .bg(rgb(theme.background))
            .child(paint)
            .cursor(self.canvas_cursor())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    let hit = this.hit_at(this.local(event.position));
                    this.pointer_down(hit, event, window, cx);
                }),
            )
            .on_mouse_down(
                MouseButton::Middle,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.pointer_down(HitTarget::Background, event, window, cx)
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, event: &MouseDownEvent, window, cx| {
                    this.pointer_down(HitTarget::Background, event, window, cx)
                }),
            )
            .on_mouse_move(cx.listener(|this, event, _, cx| this.move_pointer(event, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, event, _, cx| this.release_pointer(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.cancel_interaction(cx)),
            )
            .on_mouse_up(
                MouseButton::Middle,
                cx.listener(|this, event, _, cx| this.release_pointer(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Middle,
                cx.listener(|this, _, _, cx| this.cancel_interaction(cx)),
            )
            .on_mouse_up(
                MouseButton::Right,
                cx.listener(|this, event, _, cx| this.release_pointer(event, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| this.cancel_interaction(cx)),
            )
            .on_scroll_wheel(cx.listener(|this, event, _, cx| this.scroll(event, cx)));
        for group in self.editor.graph().groups.values() {
            let r = viewport.world_rect_to_screen(group.bounds);
            let group_id = group.id.clone();
            let selected = self.editor.selection.groups.contains(&group_id);
            surface = surface.child(
                div()
                    .absolute()
                    .left(px(r.origin.x))
                    .top(px(r.origin.y))
                    .w(px(r.size.width))
                    .h(px(r.size.height))
                    .border_1()
                    .border_color(rgb(theme.accent).alpha(if selected { 0.9 } else { 0.35 }))
                    .rounded_lg()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .text_xs()
                            .text_color(rgb(theme.accent))
                            .child(group.title.clone())
                            .cursor_grab()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, event: &MouseDownEvent, window, cx| {
                                    cx.stop_propagation();
                                    window.focus(&this.focus, cx);
                                    this.pointer_down(
                                        HitTarget::Group(group_id.clone()),
                                        event,
                                        window,
                                        cx,
                                    );
                                }),
                            ),
                    ),
            );
        }
        let visible = viewport.visible_world_rect(self.extent).expanded(100.);
        for node in self.editor.graph().nodes.values() {
            if visible.intersects(node.bounds()) {
                surface = surface.child(self.render_node(node, window, cx));
            }
        }
        if self.options.show_minimap {
            surface = surface.child(self.minimap(cx));
        }
        if self.options.show_controls {
            surface = surface.child(
                div().absolute().left(px(18.)).bottom(px(18.)).child(
                    ViewportControls::new("viewport-controls", viewport.zoom)
                        .theme(theme)
                        .on_action(cx.listener(|this, action: &ViewportAction, window, cx| {
                            window.focus(&this.focus, cx);
                            let center = model::Point::new(
                                this.extent.width * 0.5,
                                this.extent.height * 0.5,
                            );
                            this.cancel_interaction(cx);
                            match action {
                                ViewportAction::ZoomOut => {
                                    let _ = this.execute_command(
                                        CanvasCommand::Zoom {
                                            position: center,
                                            zoom: this.editor.viewport.zoom / 1.2,
                                        },
                                        cx,
                                    );
                                }
                                ViewportAction::Reset => {
                                    let _ = this.execute_command(
                                        CanvasCommand::Zoom {
                                            position: center,
                                            zoom: 1.,
                                        },
                                        cx,
                                    );
                                }
                                ViewportAction::ZoomIn => {
                                    let _ = this.execute_command(
                                        CanvasCommand::Zoom {
                                            position: center,
                                            zoom: this.editor.viewport.zoom * 1.2,
                                        },
                                        cx,
                                    );
                                }
                                ViewportAction::Fit => this.fit(cx),
                            }
                            cx.notify();
                        })),
                ),
            );
        }
        let palette = Palette::new("node-palette").disabled(!self.is_editable()).title("COMPONENTS / 组件").theme(theme)
            .entries(BUILTIN_KINDS.iter().map(|kind| PaletteEntry::new(*kind,format!("+  {kind}"),kind_color(kind))))
            .on_select(cx.listener(|this,kind: &str,_,cx| this.add(kind,cx)))
            .footer(div().mt_4().text_xs().text_color(rgb(theme.muted)).child("Middle / Space+drag: pan\nCtrl + wheel: zoom\nDrag background: select\nDrag port: connect / release: cancel\nCtrl + Z: undo\nCtrl + C/V: duplicate\nCtrl + S: copy JSON"));
        let mut inspector = crate::Inspector::new("node-inspector")
            .title("INSPECTOR / 属性")
            .theme(theme);
        if let Some(node) = self
            .editor
            .selection
            .nodes
            .iter()
            .next()
            .and_then(|id| self.editor.graph().node(id))
        {
            inspector = inspector
                .child(node.title.clone())
                .child(badge(node.kind.clone(), kind_color(&node.kind)))
                .child(div().text_xs().text_color(rgb(theme.muted)).child(format!(
                    "ID: {}\nPosition: {:.0}, {:.0}\nSize: {:.0} × {:.0}\nPorts: {}",
                    node.id,
                    node.position.x,
                    node.position.y,
                    node.size.width,
                    node.size.height,
                    node.ports.len()
                )));
            if let Some(record) = self
                .workflow
                .as_ref()
                .and_then(|w| w.records().get(&node.id))
            {
                inspector = inspector
                    .child(panel_title("WORKFLOW", theme))
                    .child(format!("{:?} · {}", record.status, record.message));
                if let Some(output) = &record.outputs {
                    let preview = serde_json::to_string_pretty(output)
                        .unwrap_or_default()
                        .chars()
                        .take(1600)
                        .collect::<String>();
                    inspector = inspector.child(
                        div()
                            .id("workflow-output")
                            .text_xs()
                            .h(px(150.))
                            .overflow_y_scroll()
                            .child(preview),
                    );
                }
            }
            for port in &node.ports {
                inspector = inspector.child(div().text_xs().text_color(rgb(theme.muted)).child(
                    format!("{:?} {} · {:?}", port.direction, port.label, port.data_type),
                ));
            }
        } else {
            inspector = inspector.child(
                div()
                    .text_sm()
                    .text_color(rgb(theme.muted))
                    .child("Select a node to inspect its type, geometry and ports"),
            );
        }
        if let Some(input) = &self.input {
            inspector = inspector
                .child(panel_title("EDIT CONTENT / 内容", theme))
                .child(Input::new(input).disabled(!self.is_editable()));
        }
        inspector = inspector
            .child(self.button("group", "Group selection", cx, |this, cx| {
                let _ = this.execute_command(
                    CanvasCommand::GroupSelection {
                        title: "Canvas group".into(),
                        padding: 22.,
                    },
                    cx,
                );
            }))
            .child(self.button("delete", "Delete selection", cx, |this, cx| {
                let _ = this.execute_command(CanvasCommand::DeleteSelection, cx);
            }))
            .child(self.button("cancel", "Cancel runs", cx, |this, cx| {
                if let Some(workflow) = this.workflow.as_mut() {
                    workflow.cancel();
                }
                let ids: Vec<_> = this.runtime.records.keys().cloned().collect();
                for id in ids {
                    this.runtime.cancel(&id);
                }
                cx.notify();
            }))
            .child(self.button("retry", "Retry selected", cx, |this, cx| {
                if let Some(workflow) = this.workflow.as_mut() {
                    let selected = this
                        .editor
                        .selection
                        .nodes
                        .iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    let result = if selected.is_empty() {
                        workflow.retry_failed()
                    } else {
                        workflow.retry_many(&selected)
                    };
                    if let Err(error) = result {
                        this.message = error;
                    }
                } else {
                    for id in &this.editor.selection.nodes {
                        if let Err(error) = this.runtime.retry(id.as_str()) {
                            this.message = error;
                        }
                    }
                }
                cx.notify();
            }));
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .font_family("sans-serif")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event, window, cx| {
                if this.focus.is_focused(window) {
                    this.key(event, cx);
                }
            }))
            .on_key_up(cx.listener(|this, event, _, cx| this.key_up(event, cx)))
            .when(self.options.show_toolbar, |root| {
                root.child(
                    Toolbar::new("flow-toolbar")
                        .theme(theme)
                        .leading(
                            div()
                                .flex()
                                .items_center()
                                .gap_3()
                                .child(div().text_lg().child("◈  gpui-flow"))
                                .child(badge("STUDIO  /  0.1", theme.accent)),
                        )
                        .child(
                            div()
                                .flex()
                                .gap_2()
                                .child(self.button("undo", "Undo", cx, |this, cx| {
                                    let _ = this.execute_command(CanvasCommand::Undo, cx);
                                }))
                                .child(self.button("redo", "Redo", cx, |this, cx| {
                                    let _ = this.execute_command(CanvasCommand::Redo, cx);
                                }))
                                .child(self.button("save", "Save snapshot", cx, |this, cx| {
                                    match this.export_json() {
                                        Ok(json) => {
                                            this.snapshot = Some(json);
                                            this.message =
                                                "In-memory snapshot saved; Ctrl+S exports JSON"
                                                    .into();
                                        }
                                        Err(e) => this.message = e.to_string(),
                                    }
                                    cx.notify();
                                }))
                                .child(self.button("load", "Load snapshot", cx, |this, cx| {
                                    if let Some(json) = this.snapshot.clone() {
                                        if let Err(error) = this.import_json(&json, cx) {
                                            this.message = error.to_string();
                                            cx.notify();
                                        }
                                    } else {
                                        this.message = "Save a snapshot first".into();
                                        cx.notify();
                                    }
                                }))
                                .child(self.button("file-save", "Save file", cx, Self::save_file))
                                .child(self.button("file-open", "Open", cx, Self::open_file))
                                .child(self.button(
                                    "mock-run",
                                    self.run_label.clone(),
                                    cx,
                                    Self::run_selected,
                                ))
                                .child(self.button(
                                    "workflow-run",
                                    "Run workflow",
                                    cx,
                                    |this, cx| {
                                        if let Err(e) = this.start_workflow(3, cx) {
                                            this.message = e;
                                            cx.notify();
                                        }
                                    },
                                )),
                        ),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .when(self.options.show_palette, |row| row.child(palette))
                    .child(surface)
                    .when(self.options.show_inspector, |row| row.child(inspector)),
            )
            .child(
                div()
                    .h(px(28.))
                    .flex_shrink_0()
                    .px_4()
                    .flex()
                    .when(!self.options.show_status, |d| d.hidden())
                    .items_center()
                    .justify_between()
                    .bg(rgb(theme.panel))
                    .text_xs()
                    .text_color(rgb(theme.muted))
                    .child(self.message.clone())
                    .child(format!(
                        "{} nodes  ·  {} edges  ·  {} selected",
                        self.editor.graph().nodes.len(),
                        self.editor.graph().edges.len(),
                        self.editor.selection.nodes.len()
                    )),
            )
    }
}
