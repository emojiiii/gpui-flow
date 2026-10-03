//! Eight bounded single-selection transform handles. Optional --trace prints
//! accepted selected-target matrices, never text contents or image bytes.
#[path = "shared/resize_scene.rs"]
mod demo_scene;
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, SceneView, SceneViewEvent,
    interaction::{SelectionMode, scene::SceneCommand},
};

use demo_scene::scene;
struct Lab {
    view: Entity<SceneView>,
    _subscription: Subscription,
}
impl Lab {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene(), cx).unwrap();
            view.set_text_editing_enabled(true, window, cx);
            view.editor.selection.select("rect".into(), false);
            view
        });
        let trace = std::env::args().any(|arg| arg == "--trace");
        let subscription = cx.subscribe(&view, move |_, view, event, cx| {
            if trace && matches!(event, SceneViewEvent::Changed) {
                let view = view.read(cx);
                for id in &view.editor.selection.ids {
                    if let Some(node) = view.editor.scene().get(id) {
                        eprintln!("resize: {id} {:?}", node.object.transform);
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
            .unwrap_or_else(|| "Select one object".into());
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground))
            .child(div().flex().items_center().gap_2().p_3()
                .children([("rect", "Rectangle"), ("rotated", "Rotated"), ("text", "Text"), ("image", "Image"), ("mixed", "Native group"), ("small", "Small")]
                    .map(|(id, label)| ControlButton::new(id, label).on_click(cx.listener(move |this, _, window, cx| {
                        this.command(SceneCommand::Select { id: id.into(), mode: SelectionMode::Replace }, window, cx);
                    }))))
                .child(ControlButton::new("undo", "Undo").on_click(cx.listener(|this, _, window, cx| this.command(SceneCommand::Undo, window, cx))))
                .child(ControlButton::new("redo", "Redo").on_click(cx.listener(|this, _, window, cx| this.command(SceneCommand::Redo, window, cx))))
                .child(ControlButton::new("readonly", "Read-only").on_click(cx.listener(|this, _, window, cx| {
                    this.view.update(cx, |view, cx| { view.finish_text_editor(window, cx); view.set_editable(!view.is_editable(), cx); });
                })))
                .child(ControlButton::new("reset", "Reset").on_click(cx.listener(|this, _, window, cx| this.command(SceneCommand::LoadScene(scene()), window, cx)))))
            .child(div().px_3().text_sm().child("Drag corners or sides · Shift: uniform · Escape: roll back · R: existing rotation shortcut · Space: pan"))
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(div().p_3().text_sm().text_color(rgb(theme.muted)).child(status))
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
                        size(px(1180.), px(740.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("gpui-flow · Eight resize handles");
                    cx.new(|cx| Lab::new(window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
