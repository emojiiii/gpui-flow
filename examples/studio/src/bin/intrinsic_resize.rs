//! Built-in intrinsic sizing policies. Run with --trace for accepted geometry.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, SceneView, SceneViewEvent,
    interaction::scene::{
        SceneCommand,
        plugins::{SceneResizePlugin, TextResizeMode},
    },
    scene::{
        Affine, Color, DisplayObject, Point as P, Scene, Shape, Size as S, Stroke, Style, display,
    },
};

fn scene() -> Scene {
    let path = display::Pen::new().move_to(P::new(0., 60.)).cubic_to(
        P::new(20., -20.),
        P::new(100., 100.),
        P::new(120., 0.),
    );
    let shapes: Vec<(&str, Shape)> = vec![
        ("Rect", display::Rect::new(120., 70.).rounded(16.).into()),
        (
            "Ellipse",
            display::Ellipse::new(120., 70.)
                .sector(0., 110.)
                .ring(0.35)
                .into(),
        ),
        (
            "Image",
            display::Image::new(
                format!("{}/assets/checker.png", env!("CARGO_MANIFEST_DIR")),
                S::new(120., 70.),
            )
            .into(),
        ),
        ("Canvas", display::Canvas::new(120., 70.).into()),
        ("Box", display::Box::new(120., 70.).rounded(10.).into()),
        ("Frame", display::Frame::new(120., 70.).rounded(10.).into()),
        (
            "Line",
            display::Line::new(P::ZERO, P::new(120., 70.)).into(),
        ),
        (
            "Polygon",
            display::Polygon::new(vec![P::ZERO, P::new(120., 20.), P::new(80., 70.)]).into(),
        ),
        ("Star", display::Star::new(5, 40., 18.).into()),
        ("Path", path.path.clone().into()),
        ("Pen", path.into()),
        ("Group", display::Group::new().into()),
    ];
    let mut scene = Scene::new();
    for (index, (name, shape)) in shapes.into_iter().enumerate() {
        let x = 50. + (index % 4) as f32 * 240.;
        let y = 60. + (index / 4) as f32 * 165.;
        let container = shape.allows_children();
        scene
            .insert(
                DisplayObject::new(name, shape)
                    .with_transform(Affine::translation(x, y))
                    .with_style(Style {
                        fill: Some(Color::hex(0x55a7ed).into()),
                        stroke: Some(Stroke::new(Color::hex(0xd9eaff), 3.)),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let mut label = DisplayObject::new(
            format!("label-{name}"),
            display::Text::new(name, S::new(150., 24.)),
        )
        .with_transform(Affine::translation(x, y - 30.))
        .with_style(Style::fill(Color::WHITE));
        label.locked = true;
        scene.insert(label, None, None).unwrap();
        if container {
            scene
                .insert(
                    DisplayObject::new(
                        format!("child-{name}"),
                        display::Rect::new(60., 35.).rounded(7.),
                    )
                    .with_transform(Affine::translation(20., 20.))
                    .with_style(Style::fill(Color::hex(0xffb76a))),
                    Some(name.into()),
                    None,
                )
                .unwrap();
        }
    }
    scene
}
struct Lab {
    view: Entity<SceneView>,
    _subscription: Subscription,
    intrinsic: bool,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene(), cx).unwrap();
            view.replace_plugin(
                SceneResizePlugin::default()
                    .with_builtin_resize(true)
                    .with_text_resize_mode(TextResizeMode::Box),
                cx,
            )
            .unwrap();
            view.editor.selection.select("Rect".into(), false);
            // This fixture demonstrates the diagonal Line's outer box. The
            // separate endpoint tool normally takes priority for Line input.
            view.set_plugin_enabled("line-endpoints", false, cx)
                .unwrap();
            view
        });
        let trace = std::env::args().any(|arg| arg == "--trace");
        let subscription = cx.subscribe(&view, move |_, view, event, cx| {
            if matches!(event, SceneViewEvent::Changed) {
                if trace {
                    let view = view.read(cx);
                    for id in &view.editor.selection.ids {
                        if let Some(node) = view.editor.scene().get(id) {
                            eprintln!(
                                "intrinsic: {id} {:?} {:?}",
                                node.object.shape.local_bounds().or_else(|| view
                                    .editor
                                    .engine
                                    .local_bounds(id)
                                    .ok()
                                    .flatten()),
                                node.object.transform
                            );
                        }
                    }
                }
                cx.notify();
            }
        });
        Self {
            view,
            _subscription: subscription,
            intrinsic: true,
        }
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let status = self
            .view
            .read(cx)
            .editor
            .selection
            .ids
            .iter()
            .next()
            .and_then(|id| self.view.read(cx).editor.scene().get(id))
            .map(|node| {
                format!(
                    "{} · geometry {:?} · matrix {:?}",
                    node.object.id,
                    node.object.shape.local_bounds().or_else(|| self
                        .view
                        .read(cx)
                        .editor
                        .engine
                        .local_bounds(&node.object.id)
                        .ok()
                        .flatten()),
                    node.object.transform
                )
            })
            .unwrap_or_else(|| "Select a component".into());
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground))
            .child(div().flex().gap_2().p_3()
                .child(ControlButton::new("mode",if self.intrinsic {"Intrinsic"} else {"Transform"}).on_click(cx.listener(|s,_,_,cx| { s.intrinsic = !s.intrinsic; s.view.update(cx,|v,cx| {v.replace_plugin(SceneResizePlugin::default().with_builtin_resize(s.intrinsic).with_text_resize_mode(TextResizeMode::Box),cx).unwrap();});cx.notify(); })))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|s,_,_,cx| {s.view.update(cx,|v,cx| {let _=v.execute_command(SceneCommand::Undo,cx);});})))
                .child(ControlButton::new("redo","Redo").on_click(cx.listener(|s,_,_,cx| {s.view.update(cx,|v,cx| {let _=v.execute_command(SceneCommand::Redo,cx);});})))
                .child(ControlButton::new("readonly","Read-only").on_click(cx.listener(|s,_,_,cx| {s.view.update(cx,|v,cx|v.set_editable(!v.is_editable(),cx));}))))
            .child(div().px_3().text_sm().child("Drag a side/corner · Shift: uniform · Escape: cancel · Box/Frame keep child geometry · Group scales content · Star stays radial"))
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(div().p_3().text_xs().child(status))
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
                        size(px(1040.), px(680.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("gpui-flow · Intrinsic component resize");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
