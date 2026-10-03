//! Compact native tour of the optional Flow Path/Skew/Pivot extension.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, SceneView, SceneViewEvent,
    interaction::{
        SelectionMode,
        scene::{GeometryToolMode, SceneCommand},
    },
    scene::{Affine, Color, DisplayObject, Scene, Style, display as d},
};

fn scene() -> Scene {
    let mut scene = Scene::new();
    let path =
        d::Path::from_svg("M0 70 Q70 -10 140 70 C170 20 220 20 250 90 L200 160 L30 150 Z").unwrap();
    scene
        .insert(
            DisplayObject::new("curve", path.clone())
                .with_transform(Affine::translation(130., 110.))
                .with_style(Style {
                    fill: Some(Color::hex(0x5476b6).into()),
                    ..Style::stroke(Color::hex(0xb6d5ff), 3.)
                }),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("pen", d::Pen { path })
                .with_transform(
                    Affine::translation(740., 170.)
                        * Affine::rotation(15.)
                        * Affine::scale(-1., 0.8),
                )
                .with_style(Style::stroke(Color::hex(0xffb267), 4.)),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("box", d::Rect::new(170., 110.))
                .with_transform(Affine::translation(420., 420.))
                .with_style(Style::fill(Color::hex(0x6cbaaa))),
            None,
            None,
        )
        .unwrap();
    scene
}
struct Lab {
    view: Entity<SceneView>,
    _subscription: Subscription,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene(), cx).unwrap();
            view.editor.selection.select("curve".into(), false);
            view.set_geometry_tool(Some(GeometryToolMode::Path), cx)
                .unwrap();
            view
        });
        let trace = std::env::args().any(|arg| arg == "--trace");
        let subscription = cx.subscribe(&view, move |_, view, event, cx| {
            if trace && matches!(event, SceneViewEvent::Changed) {
                let view = view.read(cx);
                if let Some(id) = view.editor.selection.ids.iter().next()
                    && let Some(node) = view.editor.scene().get(id)
                {
                    eprintln!(
                        "geometry {id}: pivot {:?}; transform {:?}; shape {:?}",
                        node.object.pivot, node.object.transform, node.object.shape
                    );
                }
            }
            cx.notify();
        });
        Self {
            view,
            _subscription: subscription,
        }
    }
    fn command(&mut self, command: SceneCommand, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            let _ = view.execute_command(command, cx);
        });
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let view = self.view.read(cx);
        let status = view
            .editor
            .selection
            .ids
            .iter()
            .next()
            .and_then(|id| view.editor.scene().get(id))
            .map(|node| {
                let t = node.object.transform;
                format!(
                    "{} · {:?} · pivot {:?} · affine [{:.2}, {:.2}, {:.2}, {:.2}]",
                    node.object.id,
                    view.geometry_tool(),
                    node.object.pivot,
                    t.a,
                    t.b,
                    t.c,
                    t.d
                )
            })
            .unwrap_or_else(|| "Select a shape".into());
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground))
            .child(div().flex().items_center().gap_2().p_3()
                .children([("path", "Path", Some(GeometryToolMode::Path)), ("skew", "Skew", Some(GeometryToolMode::Skew)), ("pivot", "Pivot", Some(GeometryToolMode::Pivot)), ("outer", "Outer", None)]
                    .into_iter().map(|(id, label, mode)| ControlButton::new(id, label).on_click(cx.listener(move |this, _, _, cx| {
                        this.view.update(cx, |view, cx| { view.set_geometry_tool(mode, cx).unwrap(); });
                    }))))
                .children([("curve", "Curve"), ("pen", "Leaf Pen"), ("box", "Box")].into_iter().map(|(id, label)| ControlButton::new(id, label).on_click(cx.listener(move |this, _, _, cx| {
                    this.command(SceneCommand::Select { id: id.into(), mode: SelectionMode::Replace }, cx);
                }))))
                .child(ControlButton::new("undo", "Undo").on_click(cx.listener(|this, _, _, cx| this.command(SceneCommand::Undo, cx))))
                .child(ControlButton::new("redo", "Redo").on_click(cx.listener(|this, _, _, cx| this.command(SceneCommand::Redo, cx))))
                .child(ControlButton::new("readonly", "Read-only").on_click(cx.listener(|this, _, _, cx| this.view.update(cx, |view, cx| view.set_editable(!view.is_editable(), cx)))))
                .child(ControlButton::new("reset", "Reset").on_click(cx.listener(|this, _, _, cx| {
                    this.command(SceneCommand::LoadScene(scene()), cx);
                    this.command(SceneCommand::Select { id: "curve".into(), mode: SelectionMode::Replace }, cx);
                }))))
            .child(div().px_3().text_sm().child("Path: yellow curve controls, blue endpoints · Skew: green levers · Pivot: pink anchor then blue rotation handle · Shift: local axis · Escape: cancel"))
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(div().p_3().text_sm().child(status))
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
                        size(px(1180.), px(760.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("gpui-flow · Geometry tools");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
