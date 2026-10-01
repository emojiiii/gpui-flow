//! Opt-in plain Text content sessions using the pinned GPUI Kit Textarea.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, GpuiTextMeasurer, SceneView, SceneViewEvent,
    interaction::scene::SceneCommand,
    layout::{AxisSizing, FlowDirection, FlowLayout, FlowStyle, Insets, TextSizing},
    scene::{Affine, Color, DisplayObject, Scene, Size, Style, display},
};
use std::sync::Arc;

struct Lab {
    view: Entity<SceneView>,
    trace: bool,
    _subscription: Subscription,
}
impl Lab {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("page", display::Frame::new(510., 300.))
                    .with_transform(Affine::translation(48., 48.))
                    .with_style(Style::fill(Color::hex(0x172131))),
                None,
                None,
            )
            .unwrap();
        let mut layout = FlowLayout::default();
        layout.containers.insert(
            "page".into(),
            FlowStyle {
                direction: FlowDirection::Column,
                height_sizing: AxisSizing::content(),
                padding: Insets::all(24.),
                gap: 20.,
                ..Default::default()
            },
        );
        for (id, content) in [
            ("title", "Double-click to edit this retained Text"),
            (
                "body",
                "Multiline content / 中文 / 👩‍💻\nThe panel edits content. The canvas shows measured layout.\nDone or blur creates one document undo step; Escape rolls back.",
            ),
        ] {
            let mut text = display::Text::new(content, Size::new(460., 30.));
            text.font_size = if id == "title" { 24. } else { 18. };
            scene
                .insert(
                    DisplayObject::new(id, text).with_style(Style::fill(Color::hex(0xe6edf7))),
                    Some("page".into()),
                    None,
                )
                .unwrap();
            layout.texts.insert(id.into(), TextSizing::AutoHeight);
        }
        let measured =
            layout.with_text_measurer(Arc::new(GpuiTextMeasurer::new(cx.text_system().clone())));
        measured.apply_to_scene(&mut scene).unwrap();
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene, cx).unwrap();
            measured.install_reactive(&mut view.editor.engine).unwrap();
            view.set_text_editing_enabled(true, window, cx);
            view
        });
        let trace = std::env::args().any(|arg| arg == "--trace");
        let subscription = cx.subscribe(&view, move |_, _, event, _| {
            if trace {
                match event {
                    SceneViewEvent::EditorTool(event) => eprintln!("tool: {event:?}"),
                    SceneViewEvent::Changed => eprintln!("document changed"),
                    _ => {}
                }
            }
        });
        Self {
            view,
            trace,
            _subscription: subscription,
        }
    }
    fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        if self.trace {
            eprintln!("host open: {id}");
        }
        self.view.update(cx, |view, cx| {
            if let Some(target) = view.editor.engine.handle(&id.into())
                && let Err(error) = view.open_text_editor(target, window, cx)
            {
                view.interaction_error = Some(error.to_string());
                cx.notify();
            }
        });
    }
    fn history(&mut self, redo: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            view.finish_text_editor(window, cx);
            let command = if redo {
                SceneCommand::Redo
            } else {
                SceneCommand::Undo
            };
            let _ = view.execute_command(command, cx);
        });
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground))
            .child(div().flex().items_center().gap_2().p_3()
                .child(ControlButton::new("edit-title", "Edit title").on_click(cx.listener(|this, _, window, cx| this.open("title", window, cx))))
                .child(ControlButton::new("edit-body", "Edit body").on_click(cx.listener(|this, _, window, cx| this.open("body", window, cx))))
                .child(ControlButton::new("undo", "Undo session").on_click(cx.listener(|this, _, window, cx| this.history(false, window, cx))))
                .child(ControlButton::new("redo", "Redo").on_click(cx.listener(|this, _, window, cx| this.history(true, window, cx))))
                .child(ControlButton::new("readonly", "Toggle read-only").on_click(cx.listener(|this, _, window, cx| {
                    this.view.update(cx, |view, cx| {
                        view.finish_text_editor(window, cx);
                        view.set_editable(!view.is_editable(), cx);
                    });
                }))))
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(div().p_3().text_sm().text_color(rgb(theme.muted))
                .child("Inside text: native selection, paste and undo · Outside: canvas pan / zoom / selection"))
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
                        size(px(1120.), px(720.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("gpui-flow · Text content sessions");
                    cx.new(|cx| Lab::new(window, cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
