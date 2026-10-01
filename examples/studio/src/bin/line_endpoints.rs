//! True straight-Line endpoint editing. Optional --trace prints accepted geometry
//! and transforms; no Text contents or image bytes.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, SceneView, SceneViewEvent,
    interaction::{SelectionMode, scene::SceneCommand},
    scene::{Affine, Color, DisplayObject, Point as ScenePoint, Scene, Size, Style, display},
};

fn scene() -> Scene {
    let mut scene = Scene::new();
    for (id, start, end, transform, parent) in [
        (
            "horizontal",
            ScenePoint::new(80., 110.),
            ScenePoint::new(300., 110.),
            Affine::IDENTITY,
            None,
        ),
        (
            "vertical",
            ScenePoint::new(410., 80.),
            ScenePoint::new(410., 230.),
            Affine::IDENTITY,
            None,
        ),
        (
            "diagonal",
            ScenePoint::new(600., 80.),
            ScenePoint::new(790., 210.),
            Affine::IDENTITY,
            None,
        ),
    ] {
        scene
            .insert(
                DisplayObject::new(id, display::Line::new(start, end))
                    .with_transform(transform)
                    .with_style(Style::stroke(Color::hex(0x55a7ed), 5.)),
                parent,
                None,
            )
            .unwrap();
    }
    scene
        .insert(
            DisplayObject::new("parent", display::Group::new()).with_transform(
                Affine::translation(450., 390.) * Affine::rotation(25.) * Affine::scale(-1.5, 0.7),
            ),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new(
                "affine",
                display::Line::new(ScenePoint::new(-70., -35.), ScenePoint::new(70., 35.)),
            )
            .with_transform(Affine::skew(15., 0.))
            .with_style(Style::stroke(Color::hex(0x9c7aea), 5.)),
            Some("parent".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new(
                "path",
                display::Path::new(vec![
                    display::PathCommand::MoveTo {
                        to: ScenePoint::new(70., 350.),
                    },
                    display::PathCommand::QuadraticTo {
                        control: ScenePoint::new(190., 270.),
                        to: ScenePoint::new(270., 440.),
                    },
                ]),
            )
            .with_style(Style::stroke(Color::hex(0xf4b66c), 5.)),
            None,
            None,
        )
        .unwrap();
    let mut text = display::Text::new(
        "Double-click to edit\nLine handles suspend",
        Size::new(280., 80.),
    );
    text.font_size = 22.;
    scene
        .insert(
            DisplayObject::new("text", text)
                .with_transform(Affine::translation(680., 340.))
                .with_style(Style::fill(Color::hex(0xf4dc94))),
            None,
            None,
        )
        .unwrap();
    let mut locked = DisplayObject::new(
        "locked",
        display::Line::new(ScenePoint::new(80., 510.), ScenePoint::new(300., 510.)),
    )
    .with_style(Style::stroke(Color::hex(0x7c839a), 5.));
    locked.locked = true;
    scene.insert(locked, None, None).unwrap();
    scene
}
struct Lab {
    view: Entity<SceneView>,
    _subscription: Subscription,
}
impl Lab {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene(), cx).unwrap();
            view.set_text_editing_enabled(true, window, cx);
            view.editor.selection.select("horizontal".into(), false);
            view
        });
        let trace = std::env::args().any(|arg| arg == "--trace");
        let subscription = cx.subscribe(&view, move |_, view, event, cx| {
            if trace && matches!(event, SceneViewEvent::Changed) {
                let view = view.read(cx);
                for id in &view.editor.selection.ids {
                    if let Some(node) = view.editor.scene().get(id)
                        && let gpui_flow::scene::Shape::Line(line) = &node.object.shape
                    {
                        eprintln!(
                            "line: {id} {:?} -> {:?}; transform {:?}",
                            line.start, line.end, node.object.transform
                        );
                    }
                }
            }
            cx.notify();
        });
        Self {
            view,
            _subscription: subscription,
        }
    }
    fn command(&mut self, command: SceneCommand, window: &mut Window, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            view.finish_text_editor(window, cx);
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
            .and_then(|id| {
                view.editor.scene().get(id).map(|node| {
                    let t = node.object.transform;
                    format!(
                        "{id}: [{:.3}, {:.3}, {:.3}, {:.3}] + ({:.2}, {:.2}){}",
                        t.a,
                        t.b,
                        t.c,
                        t.d,
                        t.tx,
                        t.ty,
                        if view.is_editable() {
                            ""
                        } else {
                            " · read-only"
                        }
                    )
                })
            })
            .unwrap_or_else(|| "Select objects".into());
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground))
            .child(div().flex().items_center().gap_2().p_3()
                .children([
                    ("horizontal", "Horizontal", vec!["horizontal"]),
                    ("vertical", "Vertical", vec!["vertical"]),
                    ("affine", "Affine", vec!["affine"]),
                    ("path", "Curve", vec!["path"]),
                    ("pair", "Pair", vec!["horizontal", "diagonal"]),
                    ("locked", "Locked", vec!["locked"]),
                    ("text", "Text", vec!["text"]),
                ].into_iter().map(|(id, label, ids)| ControlButton::new(id, label).on_click(cx.listener(move |this, _, window, cx| {
                    for (i, id) in ids.iter().enumerate() {
                        this.command(SceneCommand::Select { id: (*id).into(), mode: if i == 0 { SelectionMode::Replace } else { SelectionMode::Add } }, window, cx);
                    }
                }))))
                .child(ControlButton::new("undo", "Undo").on_click(cx.listener(|this, _, window, cx| this.command(SceneCommand::Undo, window, cx))))
                .child(ControlButton::new("redo", "Redo").on_click(cx.listener(|this, _, window, cx| this.command(SceneCommand::Redo, window, cx))))
                .child(ControlButton::new("readonly", "Read-only").on_click(cx.listener(|this, _, window, cx| {
                    this.view.update(cx, |view, cx| { view.finish_text_editor(window, cx); view.set_editable(!view.is_editable(), cx); });
                })))
                .child(ControlButton::new("zoom", "Zoom 1x/1.5x").on_click(cx.listener(|this, _, window, cx| {
                    let zoom = if this.view.read(cx).editor.viewport.zoom < 1.25 { 1.5 } else { 1. };
                    this.command(SceneCommand::Zoom { position: ScenePoint::new(350., 220.), zoom }, window, cx);
                })))
                .child(ControlButton::new("reset", "Reset").on_click(cx.listener(|this, _, window, cx| this.command(SceneCommand::LoadScene(scene()), window, cx)))))
            .child(div().px_3().text_sm().child("Two squares: true Line endpoints · Shift: dominant local drag axis · Curve: box transforms only · Escape: roll back · Space: pan"))
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(div().p_3().text_sm().text_color(rgb(theme.muted)).child(format!("{} selected · zoom {:.2} · {}", view.editor.selection.ids.len(), view.editor.viewport.zoom, status)))
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
                        size(px(1280.), px(780.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("gpui-flow · Line endpoints");
                    cx.new(|cx| Lab::new(window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
