//! Visible interaction/plugin workbench. All behavior is shared library code.
use gpui::{prelude::*, *};
use gpui_flow::{ControlButton, FlowCanvas, FlowOptions, FlowTheme, core::*, interaction::*};

struct MinimumSize {
    minimum: f32,
}
impl CanvasPlugin for MinimumSize {
    fn id(&self) -> &str {
        "example-min-size"
    }
    fn priority(&self) -> i32 {
        50
    }
    fn filter_command(
        &mut self,
        command: &mut CanvasCommand,
        _: &PluginContext<'_>,
        _: &mut Vec<CanvasEffect>,
    ) -> Result<(), String> {
        if let CanvasCommand::ResizeNode { size, .. } = command {
            size.width = size.width.max(self.minimum);
            size.height = size.height.max(self.minimum * 0.6);
        }
        Ok(())
    }
}
struct InteractionDemo {
    canvas: Entity<FlowCanvas>,
    enlarged_minimum: bool,
}
impl InteractionDemo {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut graph = Graph::new();
        let mut source = Node::new(
            "source",
            "Number source",
            gpui_flow::core::Point::new(70., 100.),
        )
        .with_kind("text")
        .with_port(Port::output("out", "Number", DataType::Number));
        source.data = serde_json::json!({"text":"Drag the right port. Release near the Number input to connect."});
        let mut target = Node::new(
            "target",
            "Number target",
            gpui_flow::core::Point::new(450., 70.),
        )
        .with_kind("text")
        .with_port(Port::input("in", "Number", DataType::Number));
        target.data =
            serde_json::json!({"text":"Compatible target. Undo removes the created connection."});
        let mut wrong = Node::new(
            "wrong",
            "String target (incompatible)",
            gpui_flow::core::Point::new(450., 330.),
        )
        .with_kind("text")
        .with_port(Port::input("in", "String", DataType::String));
        wrong.data =
            serde_json::json!({"text":"Drag Number here: rejected highlight, no edge on release."});
        let mut locked = Node::new(
            "locked",
            "Locked node",
            gpui_flow::core::Point::new(70., 390.),
        )
        .with_kind("text");
        locked.locked = true;
        locked.data =
            serde_json::json!({"text":"Selectable, but move/resize/delete stay blocked."});
        for node in [source, target, wrong, locked] {
            graph.add_node(node).unwrap();
        }
        let canvas = cx.new(|cx| {
            let mut view = FlowCanvas::new(graph, cx).unwrap();
            view.options = FlowOptions::canvas_only();
            view.options.show_status = true;
            view.install_plugin(MinimumSize { minimum: 180. }, cx)
                .unwrap();
            view
        });
        Self {
            canvas,
            enlarged_minimum: false,
        }
    }
}
impl Render for InteractionDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let editable = self.canvas.read(cx).is_editable();
        let mut controls = div().flex().flex_wrap().gap_2().child(
            ControlButton::new(
                "editable",
                if editable {
                    "Editable: on"
                } else {
                    "Read-only"
                },
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.canvas.update(cx, |canvas, cx| {
                    canvas.set_editable(!canvas.is_editable(), cx)
                });
                cx.notify();
            })),
        );
        for id in [
            "selection",
            "drag",
            "resize",
            "connection",
            "snapping",
            "pan",
            "zoom",
        ] {
            let enabled = self.canvas.read(cx).interactions.is_enabled(id);
            controls = controls.child(
                ControlButton::new(id, format!("{id}: {}", if enabled { "on" } else { "off" }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.canvas.update(cx, |canvas, cx| {
                            let enabled = canvas.interactions.is_enabled(id);
                            let _ = canvas.set_plugin_enabled(id, !enabled, cx);
                        });
                        cx.notify();
                    })),
            );
        }
        controls = controls.child(
            ControlButton::new(
                "replace-minimum",
                if self.enlarged_minimum {
                    "Custom min: 300"
                } else {
                    "Custom min: 180"
                },
            )
            .on_click(cx.listener(|this, _, _, cx| {
                this.enlarged_minimum = !this.enlarged_minimum;
                let minimum = if this.enlarged_minimum { 300. } else { 180. };
                this.canvas.update(cx, |canvas, cx| {
                    canvas.replace_plugin(MinimumSize { minimum }, cx).unwrap();
                });
                cx.notify();
            })),
        );
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground)).font_family("sans-serif")
            .child(div().p_4().flex().flex_col().gap_3().child("gpui-flow · Interaction plugins").child(controls).child(div().text_xs().text_color(rgb(theme.muted)).child("Drag a port → compatible target snaps; release empty cancels. Escape / focus loss / middle-button interruption roll back. Select a node for the square resize handle. Ctrl+Z undo. Toggle plugins independently; custom minimum-size middleware is replaceable.")))
            .child(div().flex_1().min_h_0().child(self.canvas.clone()))
    }
}
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            let bounds = Bounds::centered(None, size(px(1280.), px(860.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Interaction plugins");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(InteractionDemo::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
