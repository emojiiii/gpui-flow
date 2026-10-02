//! Real overlapping layers, independent viewports/history and lifecycle controls.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, LayerId, LayerOptions, LayeredApp, LayeredAppEvent, SceneView,
    core::{Point as P, Viewport},
    interaction::{CanvasEvent, scene::SceneCommand},
    scene::{Affine, Color, DisplayObject, HitPolicy, Scene, Style, display},
};
fn scene(top: bool) -> Scene {
    let mut s = Scene::new();
    if !top {
        let mut background =
            DisplayObject::new("explicit-background", display::Rect::new(1300., 700.))
                .with_style(Style::fill(Color::hex(0x202938)));
        background.hit_policy = HitPolicy::None;
        s.insert(background, None, None).unwrap();
    }
    let (x, y, color) = if top {
        (230., 155., 0xeab85a)
    } else {
        (100., 120., 0x569beb)
    };
    s.insert(
        DisplayObject::new("card", display::Rect::new(300., 190.).rounded(20.))
            .with_transform(Affine::translation(x, y))
            .with_style(Style::fill(Color::hex(color))),
        None,
        None,
    )
    .unwrap();
    s
}
struct Lab {
    app: Entity<LayeredApp>,
    ground: LayerId,
    top: Option<LayerId>,
    detached: Option<Entity<SceneView>>,
    last: String,
    sequence: u64,
    _subscription: Subscription,
}
impl Lab {
    fn top_viewport() -> Viewport {
        Viewport {
            pan: P::new(45., 25.),
            zoom: 1.15,
            ..Default::default()
        }
    }
    fn new(cx: &mut Context<Self>) -> Self {
        let mut ground = None;
        let mut top = None;
        let app = cx.new(|cx| {
            let mut app = LayeredApp::new(cx);
            ground = Some(
                app.add_layer(
                    scene(false),
                    LayerOptions {
                        hit_background: true,
                        ..Default::default()
                    },
                    cx,
                )
                .unwrap(),
            );
            let id = app
                .add_layer(scene(true), LayerOptions::default(), cx)
                .unwrap();
            app.set_layer_viewport(id, Self::top_viewport(), cx)
                .unwrap();
            top = Some(id);
            app
        });
        let subscription = cx.subscribe(&app, |this: &mut Self, _, event, cx| {
            if let LayeredAppEvent::Input { target, input } = event {
                let notable = !matches!(input, CanvasEvent::PointerMove(_))
                    || this.app.read(cx).pointer_layer().is_some();
                if notable {
                    this.last = format!("target={:?} input={input:?}", target.map(LayerId::serial));
                    this.record("input", cx);
                    cx.notify();
                }
            }
        });
        Self {
            app,
            ground: ground.unwrap(),
            top,
            detached: None,
            last: "Click overlap: gold wins; exposed blue: ground wins".into(),
            sequence: 0,
            _subscription: subscription,
        }
    }
    fn record(&mut self, action: &str, cx: &App) {
        self.sequence += 1;
        let app = self.app.read(cx);
        let layers: Vec<_> = app.layer_ids().map(|id| {
            let v = app.layer(id).unwrap().read(cx);
            let o = v.editor.scene().get(&"card".into());
            serde_json::json!({"layer":id.serial(),"visible":app.layer_options(id).unwrap().visible,"viewport":v.editor.viewport,"card":o.map(|n|n.object.transform),"selected":v.editor.selection.ids,"error":v.interaction_error})
        }).collect();
        eprintln!(
            "{}",
            serde_json::json!({"fixture":"layered-app","seq":self.sequence,"action":action,"stack":layers,"focus":app.focused_layer().map(LayerId::serial),"pointer_owner":app.pointer_layer().map(LayerId::serial),"last":self.last})
        );
    }
    fn action(&mut self, action: u8, window: &mut Window, cx: &mut Context<Self>) {
        self.app.update(cx, |app, cx| match action {
            0 => {
                if let Some(id) = self.top {
                    let mut o = app.layer_options(id).unwrap();
                    o.visible = !o.visible;
                    app.set_layer_options(id, o, cx).unwrap();
                }
            }
            1 => {
                if let Some(id) = self.top {
                    let target = usize::from(app.layer_ids().next() == Some(id));
                    app.reorder_layer(id, target, cx).unwrap();
                }
            }
            2 => {
                if let Some(id) = self.top {
                    let current = app.layer(id).unwrap().read(cx).editor.viewport;
                    let next = if current.zoom == 1. {
                        Self::top_viewport()
                    } else {
                        Viewport::default()
                    };
                    app.set_layer_viewport(id, next, cx).unwrap();
                }
            }
            3 => {
                if let Some(id) = self.top.take() {
                    self.detached = Some(app.detach_layer(id, cx).unwrap());
                } else if let Some(view) = self.detached.take() {
                    self.top = Some(app.attach_layer(view, LayerOptions::default(), cx).unwrap());
                }
            }
            4 => {
                if let Some(id) = self.top.take() {
                    app.destroy_layer(id, cx).unwrap();
                } else if self.detached.is_none() {
                    let id = app
                        .add_layer(scene(true), LayerOptions::default(), cx)
                        .unwrap();
                    app.set_layer_viewport(id, Self::top_viewport(), cx)
                        .unwrap();
                    self.top = Some(id);
                }
            }
            5 | 6 => {
                let id = if action == 5 {
                    Some(self.ground)
                } else {
                    self.top
                };
                if let Some(id) = id
                    && let Some(h) = app
                        .layer(id)
                        .unwrap()
                        .read(cx)
                        .editor
                        .engine
                        .handle(&"card".into())
                {
                    let _ = app.focus_element(id, h, window, cx);
                }
            }
            7 | 8 => {
                let id = if action == 7 {
                    Some(self.ground)
                } else {
                    self.top
                };
                if let Some(id) = id {
                    app.layer(id).unwrap().update(cx, |v, cx| {
                        v.execute_command(SceneCommand::Undo, cx).unwrap();
                    });
                }
            }
            _ => {}
        });
        self.record(&format!("control-{action}"), cx);
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = format!(
            "{}\nGold has its own zoom/pan. Drag across blue: gold keeps capture. Hide/reorder/detach/destroy cancels gestures. Focus then Delete/Undo affects only that layer. Both cards deliberately share the ID 'card'.",
            self.last
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x171b24))
            .text_color(rgb(0xe9edf4))
            .child(
                div()
                    .h(px(54.))
                    .px_3()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(
                        ControlButton::new("hide", "Hide / show gold")
                            .on_click(cx.listener(|s, _, w, cx| s.action(0, w, cx))),
                    )
                    .child(
                        ControlButton::new("order", "Reorder gold")
                            .on_click(cx.listener(|s, _, w, cx| s.action(1, w, cx))),
                    )
                    .child(
                        ControlButton::new("viewport", "Gold viewport")
                            .on_click(cx.listener(|s, _, w, cx| s.action(2, w, cx))),
                    )
                    .child(
                        ControlButton::new("detach", "Detach / attach")
                            .on_click(cx.listener(|s, _, w, cx| s.action(3, w, cx))),
                    )
                    .child(
                        ControlButton::new("destroy", "Destroy / create")
                            .on_click(cx.listener(|s, _, w, cx| s.action(4, w, cx))),
                    ),
            )
            .child(
                div()
                    .h(px(44.))
                    .px_3()
                    .flex()
                    .gap_2()
                    .items_center()
                    .child(
                        ControlButton::new("focus-blue", "Focus blue")
                            .on_click(cx.listener(|s, _, w, cx| s.action(5, w, cx))),
                    )
                    .child(
                        ControlButton::new("focus-gold", "Focus gold")
                            .on_click(cx.listener(|s, _, w, cx| s.action(6, w, cx))),
                    )
                    .child(
                        ControlButton::new("undo-blue", "Undo blue")
                            .on_click(cx.listener(|s, _, w, cx| s.action(7, w, cx))),
                    )
                    .child(
                        ControlButton::new("undo-gold", "Undo gold")
                            .on_click(cx.listener(|s, _, w, cx| s.action(8, w, cx))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(self.app.clone()),
            )
            .child(div().h(px(100.)).px_3().py_2().text_sm().child(status))
    }
}
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1160.), px(740.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Overlapping layered app");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
