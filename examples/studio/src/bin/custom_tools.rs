//! Host-defined inner/outer widgets with token-owned edits and explicit release.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, SceneToolWidget, SceneView, SceneViewEvent, ToolWidgetContext,
    engine::PropertyKey,
    interaction::scene::{SceneCommand, ToolCloseReason, ToolSessionToken},
    scene::{Affine, Color, DisplayObject, Scene, Size, Style, display},
};
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

#[derive(Debug, Default)]
struct Counts {
    loads: usize,
    updates: usize,
    unloads: usize,
    destroys: usize,
    live_widgets: usize,
    live_entities: usize,
}
type Shared = Rc<RefCell<Counts>>;
struct PropertyWidget {
    context: ToolWidgetContext,
    counts: Shared,
}
impl PropertyWidget {
    fn new(context: ToolWidgetContext, counts: Shared) -> Self {
        counts.borrow_mut().live_widgets += 1;
        Self { context, counts }
    }
}
impl Drop for PropertyWidget {
    fn drop(&mut self) {
        self.counts.borrow_mut().live_widgets -= 1;
    }
}
impl SceneToolWidget for PropertyWidget {
    fn load(&mut self, _: &ToolWidgetContext, _: &mut Context<SceneView>) {
        self.counts.borrow_mut().loads += 1;
    }
    fn update(&mut self, c: &ToolWidgetContext, _: &mut Context<SceneView>) {
        self.context = c.clone();
        self.counts.borrow_mut().updates += 1;
    }
    fn unload(&mut self, reason: ToolCloseReason, _: &mut App) {
        self.counts.borrow_mut().unloads += 1;
        println!("widget_unload {} {reason:?}", self.context.tool_id);
    }
    fn destroy(&mut self, _: &mut App) {
        self.counts.borrow_mut().destroys += 1;
    }
    fn render(&mut self, _: &mut Window, cx: &mut Context<SceneView>) -> AnyElement {
        let mut body = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(format!("Host widget: {}", self.context.tool_id));
        if let Some(token) = self.context.token {
            let opacity = self.context.object.as_ref().map_or(0., |o| o.style.opacity);
            body = body.child(format!("Accepted opacity: {opacity:.2}")).child(
                ControlButton::new("set-opacity", "Set opacity .35").on_click(cx.listener(
                    move |view, _, _, cx| {
                        let result = view.set_inner_tool_property(
                            token,
                            PropertyKey::Opacity,
                            json!(0.35),
                            cx,
                        );
                        println!("widget_edit opacity=.35 accepted={}", result.is_ok());
                    },
                )),
            );
        } else {
            body = body.child(format!("Outer selection: {:?}", self.context.selection));
        }
        body.into_any_element()
    }
}
struct HostedEntity {
    parent: WeakEntity<SceneView>,
    context: ToolWidgetContext,
}
impl Render for HostedEntity {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(rgb(0x194a44))
            .child("A separately retained GPUI Entity")
            .child(format!(
                "Accepted opacity: {:.2}",
                self.context.object.as_ref().map_or(0., |o| o.style.opacity)
            ))
            .child(
                ControlButton::new("entity-opacity", "Set opacity .65").on_click(cx.listener(
                    |this, _, _, cx| {
                        let token = this.context.token.expect("inner widget");
                        let result = this.parent.update(cx, |view, cx| {
                            view.set_inner_tool_property(
                                token,
                                PropertyKey::Opacity,
                                json!(0.65),
                                cx,
                            )
                        });
                        println!(
                            "entity_edit opacity=.65 accepted={}",
                            result.is_ok_and(|r| r.is_ok())
                        );
                    },
                )),
            )
    }
}
struct EntityWidget {
    entity: Entity<HostedEntity>,
    counts: Shared,
}
impl Drop for EntityWidget {
    fn drop(&mut self) {
        self.counts.borrow_mut().live_widgets -= 1;
    }
}
impl SceneToolWidget for EntityWidget {
    fn load(&mut self, _: &ToolWidgetContext, _: &mut Context<SceneView>) {
        self.counts.borrow_mut().loads += 1;
    }
    fn update(&mut self, c: &ToolWidgetContext, cx: &mut Context<SceneView>) {
        self.counts.borrow_mut().updates += 1;
        self.entity.update(cx, |v, cx| {
            v.context = c.clone();
            cx.notify();
        });
    }
    fn render(&mut self, _: &mut Window, _: &mut Context<SceneView>) -> AnyElement {
        self.entity.clone().into_any_element()
    }
    fn unload(&mut self, reason: ToolCloseReason, _: &mut App) {
        self.counts.borrow_mut().unloads += 1;
        println!("entity_widget_unload {reason:?}");
    }
    fn destroy(&mut self, _: &mut App) {
        self.counts.borrow_mut().destroys += 1;
    }
}
fn document() -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("rect", display::Rect::new(170., 110.))
            .with_transform(Affine::translation(45., 65.))
            .with_style(Style::fill(Color::hex(0x57a4e5))),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("ellipse", display::Ellipse::new(150., 105.))
            .with_transform(Affine::translation(280., 65.))
            .with_style(Style::fill(Color::hex(0xe99b61))),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new(
            "text",
            display::Text::new(
                "Standard Text editor remains available",
                Size::new(450., 65.),
            ),
        )
        .with_transform(Affine::translation(45., 250.))
        .with_style(Style::fill(Color::WHITE)),
        None,
        None,
    )
    .unwrap();
    s
}
struct Lab {
    view: Option<Entity<SceneView>>,
    counts: Shared,
    old_token: Option<ToolSessionToken>,
    status: String,
    _subscription: Option<Subscription>,
}
impl Lab {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut s = Self {
            view: None,
            counts: Rc::default(),
            old_token: None,
            status: "Open either custom tool".into(),
            _subscription: None,
        };
        s.create(window, cx);
        s
    }
    fn create(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let counts = self.counts.clone();
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.show_grid = false;
            v.set_text_editing_enabled(true, window, cx);
            let c = counts.clone();
            v.register_inner_tool(
                "rect-style",
                "rect",
                move |context, _, _| PropertyWidget::new(context, c.clone()),
                cx,
            )
            .unwrap();
            let c = counts.clone();
            v.register_outer_tool(
                "inspector",
                move |context, _, _| PropertyWidget::new(context, c.clone()),
                cx,
            )
            .unwrap();
            let c = counts.clone();
            v.register_inner_tool(
                "ellipse-style",
                "ellipse",
                move |context, _, cx| {
                    c.borrow_mut().live_widgets += 1;
                    c.borrow_mut().live_entities += 1;
                    let parent = cx.entity().downgrade();
                    let drop_counts = c.clone();
                    let entity = cx.new(|cx| {
                        cx.on_release(move |_: &mut HostedEntity, _| {
                            drop_counts.borrow_mut().live_entities -= 1;
                        })
                        .detach();
                        HostedEntity { parent, context }
                    });
                    EntityWidget {
                        entity,
                        counts: c.clone(),
                    }
                },
                cx,
            )
            .unwrap();
            v
        });
        self._subscription = Some(cx.subscribe(&view, |_, _, event, cx| {
            if let SceneViewEvent::EditorTool(e) = event {
                println!("tool_lifecycle {e:?}");
            }
            cx.notify();
        }));
        self.view = Some(view);
        self.status = "Fresh view and registries".into();
        cx.notify();
    }
    fn action(&mut self, action: &str, window: &mut Window, cx: &mut Context<Self>) {
        if action == "Release" {
            self._subscription = None;
            self.view = None;
            self.status = "Released; Record after the next frame".into();
            cx.notify();
            return;
        }
        if action == "Recreate" {
            self.create(window, cx);
            return;
        }
        if action == "Record" {
            self.record(cx);
            cx.notify();
            return;
        }
        if action == "Close" {
            self._subscription = None;
            self.view = None;
            cx.quit();
            return;
        }
        let Some(view) = self.view.clone() else {
            self.status = "No live view".into();
            cx.notify();
            return;
        };
        let old = self.old_token;
        let result = view.update(cx, |v, cx| -> Result<Option<ToolSessionToken>, String> {
            match action {
                "Rect" | "Ellipse" => {
                    let (tool, id) = if action == "Rect" {
                        ("rect-style", "rect")
                    } else {
                        ("ellipse-style", "ellipse")
                    };
                    let h = v.editor.engine.handle(&id.into()).unwrap();
                    return v
                        .open_inner_tool(tool, h, window, cx)
                        .map(Some)
                        .map_err(|e| e.to_string());
                }
                "Outer" => {
                    v.select_outer_tool("inspector", window, cx)
                        .map_err(|e| e.to_string())?;
                }
                "Text" => {
                    let h = v.editor.engine.handle(&"text".into()).unwrap();
                    v.open_text_editor(h, window, cx)
                        .map_err(|e| e.to_string())?;
                }
                "Undo" | "Redo" => {
                    v.finish_inner_tool(window, cx);
                    v.finish_text_editor(window, cx);
                    v.execute_command(
                        if action == "Undo" {
                            SceneCommand::Undo
                        } else {
                            SceneCommand::Redo
                        },
                        cx,
                    )
                    .map_err(|e| e.to_string())?;
                }
                "Readonly" => {
                    v.set_editable(!v.is_editable(), cx);
                }
                "Disable" => {
                    v.set_editor_tools_enabled(false, window, cx)
                        .map_err(|e| e.to_string())?;
                }
                "Enable" => {
                    v.set_editor_tools_enabled(true, window, cx)
                        .map_err(|e| e.to_string())?;
                }
                "Unregister" => {
                    v.unregister_editor_tool("rect-style", window, cx)
                        .map_err(|e| e.to_string())?;
                }
                "Stale" => {
                    let token = old.ok_or("No previous token")?;
                    let before = v.editor.scene().to_json().unwrap();
                    let rejected = v
                        .set_inner_tool_property(token, PropertyKey::Opacity, json!(0.1), cx)
                        .is_err();
                    if !rejected || v.editor.scene().to_json().unwrap() != before {
                        return Err("STALE callback unexpectedly accepted".into());
                    }
                }
                _ => {}
            }
            Ok(None)
        });
        match result {
            Ok(Some(t)) => {
                if self.old_token.is_none() {
                    self.old_token = Some(t);
                }
                self.status = format!("{action}: accepted");
            }
            Ok(None) => self.status = format!("{action}: accepted"),
            Err(e) => self.status = format!("{action}: {e}"),
        };
        self.record(cx);
        cx.notify();
    }
    fn record(&self, cx: &App) {
        let state = self.view.as_ref().map(|e| {
            let v = e.read(cx);
            let a = v
                .editor
                .scene()
                .get(&"rect".into())
                .map(|o| o.object.style.opacity);
            let b = v
                .editor
                .scene()
                .get(&"ellipse".into())
                .map(|o| o.object.style.opacity);
            (
                v.editor.engine.revision(),
                v.is_editable(),
                a,
                b,
                v.custom_tool_active(),
            )
        });
        println!(
            "record status={:?} state={state:?} counts={:?}",
            self.status,
            self.counts.borrow()
        );
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let mut buttons = div().flex().flex_wrap().gap_2().p_3();
        for label in [
            "Rect",
            "Ellipse",
            "Outer",
            "Text",
            "Undo",
            "Redo",
            "Readonly",
            "Disable",
            "Enable",
            "Unregister",
            "Stale",
            "Release",
            "Recreate",
            "Record",
            "Close",
        ] {
            buttons =
                buttons.child(ControlButton::new(label, label).theme(theme).on_click(
                    cx.listener(move |this, _, window, cx| this.action(label, window, cx)),
                ));
        }
        let canvas = match &self.view {
            Some(view) => div().size_full().child(view.clone()).into_any_element(),
            None => div().p_4().child("View released").into_any_element(),
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .child(buttons)
            .child(div().flex_1().min_h_0().child(canvas))
            .child(div().p_3().text_sm().child(format!(
                "{} | {:?}",
                self.status,
                self.counts.borrow()
            )))
    }
}
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1040.), px(600.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("gpui-flow custom tool widgets");
                    cx.new(|cx| Lab::new(window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
