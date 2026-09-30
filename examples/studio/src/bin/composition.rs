//! Independent components next to an embedded, host-configured flow canvas.
//! Run with: cargo run --locked -p flow-studio --bin composition
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowCanvas, FlowOptions, FlowTheme, Handle, Inspector, NodeFrame,
    NodeRenderContext, NodeRenderer, OutputNode, TextNode, Toolbar, badge, builtin_node,
    core::{self, DataType, Graph, Node, Port, PortDirection, PortRef},
};

struct ResultBody;
impl NodeRenderer for ResultBody {
    fn render(
        &self,
        node: &Node,
        context: NodeRenderContext,
        _: &mut Window,
        _: &mut App,
    ) -> AnyElement {
        OutputNode::new("This custom body is registered by the host.")
            .theme(context.theme)
            .zoom(context.zoom)
            .content(badge(
                format!("{} input port · arbitrary GPUI slot", node.ports.len()),
                context.theme.accent,
            ))
            .into_any_element()
    }
}

struct CompositionDemo {
    canvas: Entity<FlowCanvas>,
    selected: bool,
    connections_requested: usize,
}
impl CompositionDemo {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut graph = Graph::new();
        let mut prompt = builtin_node("text", "prompt", core::Point::new(60., 110.));
        prompt.data["text"] = "This FlowCanvas lives inside an application-owned shell.".into();
        graph.add_node(prompt).expect("valid text node");
        let mut result = Node::new(
            "result",
            "Host result renderer",
            core::Point::new(420., 110.),
        )
        .with_kind("host-result");
        result.size = core::Size::new(280., 210.);
        result.ports = vec![Port::input("in", "Text", DataType::String)];
        graph.add_node(result).expect("valid result node");
        graph
            .connect(
                "prompt-result",
                PortRef::new("prompt", "out"),
                PortRef::new("result", "in"),
            )
            .expect("compatible text connection");
        let canvas = cx.new(|cx| {
            let mut canvas = FlowCanvas::new(graph, cx).expect("valid graph");
            canvas.options = FlowOptions::canvas_only();
            canvas.registry.register("host-result", ResultBody);
            canvas
        });
        Self {
            canvas,
            selected: false,
            connections_requested: 0,
        }
    }
}
impl Render for CompositionDemo {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let standalone = NodeFrame::new("standalone-card", "Independent NodeFrame")
            .size(core::Size::new(284., 212.))
            .selected(self.selected)
            .theme(theme)
            .color(0xa8b5ff)
            .body(
                TextNode::new("This card has no FlowCanvas or Editor. Click its title or output handle; the host owns the state.")
                    .theme(theme),
            )
            .on_header_mouse_down(cx.listener(|this, _, _, cx| {
                cx.stop_propagation();
                this.selected = !this.selected;
                cx.notify();
            }))
            .child(
                Handle::new("independent-out", PortDirection::Output, DataType::String)
                    .anchor(core::Point::new(284., 92.))
                    .label("Text")
                    .theme(theme)
                    .active(self.connections_requested > 0)
                    .on_connect(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.connections_requested += 1;
                        cx.notify();
                    })),
            );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .font_family("sans-serif")
            .child(
                Toolbar::new("host-toolbar")
                    .theme(theme)
                    .leading(div().text_lg().child("gpui-flow · component composition"))
                    .child(
                        ControlButton::new("reset-host-state", "Reset host state")
                            .theme(theme)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.selected = false;
                                this.connections_requested = 0;
                                cx.notify();
                            })),
                    )
                    .child(
                        ControlButton::new("reset-viewport", "Reset canvas viewport")
                            .theme(theme)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.canvas.update(cx, |canvas, cx| {
                                    canvas.editor.viewport = core::Viewport::default();
                                    cx.notify();
                                });
                            })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        div()
                            .w(px(330.))
                            .flex_shrink_0()
                            .p_5()
                            .flex()
                            .flex_col()
                            .gap_4()
                            .border_r_1()
                            .border_color(rgb(theme.border))
                            .child(badge("STANDALONE COMPONENT", theme.accent))
                            .child(standalone)
                            .child(
                                div().text_sm().text_color(rgb(theme.muted)).child(
                                    "The right-hand canvas uses the same NodeFrame and Handle components. Its custom result body comes from a NodeRegistry entry.",
                                ),
                            ),
                    )
                    .child(div().flex_1().min_w_0().child(self.canvas.clone()))
                    .child(
                        Inspector::new("host-inspector")
                            .title("HOST STATE")
                            .theme(theme)
                            .field("Standalone card selected", self.selected.to_string())
                            .field("Handle requests", self.connections_requested.to_string())
                            .field("Canvas chrome", "FlowOptions::canvas_only()")
                            .field("Registered kind", "host-result")
                            .child(
                                div().text_xs().text_color(rgb(theme.muted)).child(
                                    "Handle requests here are just callback counts. The host decides how to create graph connections.",
                                ),
                            ),
                    ),
            )
    }
}

fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx: &mut App| {
            gpui_kit::init(cx);
            let bounds = Bounds::centered(None, size(px(1440.), px(820.)), cx);
            cx.open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · independent components");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(CompositionDemo::new)
                },
            )
            .expect("open composition window");
            cx.activate(true);
        });
}
