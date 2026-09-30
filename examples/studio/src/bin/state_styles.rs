//! Real SceneView paint overlays driven by routed hover/press/focus/selection.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, SceneView,
    interaction::scene::{StatePaint, StateStyles, StateTransition, StateTransitions},
    scene::{self, Affine, Color, DisplayObject, Scene, Style, display},
};
fn styles() -> StateStyles {
    StateStyles {
        selected: StatePaint::fill(Color::hex(0x3380b8)),
        focus: StatePaint::fill(Color::hex(0x9662ce)),
        hover: StatePaint::fill(Color::hex(0x239e91)),
        press: StatePaint::fill(Color::hex(0xd38b36)),
        disabled: StatePaint {
            fill: Some(Color::hex(0x586476)),
            opacity: Some(0.5),
        },
    }
}
struct Lab {
    view: Entity<SceneView>,
    disabled: bool,
    base_red: bool,
    bound: bool,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("card", display::Rect::new(500., 260.).rounded(24.))
                    .with_transform(Affine::translation(160., 140.))
                    .with_style(Style::fill(Color::hex(0x344760))),
                None,
                None,
            )
            .unwrap();
        let mut label = DisplayObject::new(
            "label",
            display::Text::new(
                "Hover / press / release / leave",
                scene::Size::new(470., 36.),
            ),
        )
        .with_transform(Affine::translation(18., 100.))
        .with_style(Style::fill(Color::WHITE));
        label.hit_policy = scene::HitPolicy::None;
        scene.insert(label, Some("card".into()), None).unwrap();
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene, cx).unwrap();
            view.set_editable(false, cx);
            view.state_styles
                .bind(
                    &view.editor.engine,
                    view.editor.engine.handle(&"card".into()).unwrap(),
                    styles(),
                )
                .unwrap();
            view.state_styles
                .set_transitions(
                    &view.editor.engine,
                    view.editor.engine.handle(&"card".into()).unwrap(),
                    StateTransitions {
                        transition: StateTransition::new(180),
                        transition_out: Some(StateTransition::new(320)),
                    },
                )
                .unwrap();
            view
        });
        Self {
            view,
            disabled: false,
            base_red: false,
            bound: true,
        }
    }
}
impl Lab {
    fn toggle_disabled(&mut self, cx: &mut Context<Self>) {
        self.disabled = !self.disabled;
        self.view.update(cx, |view, cx| {
            if self.bound {
                let handle = view.editor.engine.handle(&"card".into()).unwrap();
                view.state_styles
                    .set_disabled(&view.editor.engine, handle, self.disabled)
                    .unwrap();
            }
            cx.notify();
        });
        cx.notify();
    }
    fn edit_base(&mut self, cx: &mut Context<Self>) {
        self.base_red = !self.base_red;
        let color = Color::hex(if self.base_red { 0x9c4052 } else { 0x344760 });
        self.view.update(cx, |view, cx| {
            view.editor
                .transaction(|scene| {
                    scene.update_object(&"card".into(), |object| {
                        object.style.fill = Some(color.into())
                    })
                })
                .unwrap();
            cx.notify();
        });
    }
    fn toggle_binding(&mut self, cx: &mut Context<Self>) {
        self.bound = !self.bound;
        self.view.update(cx, |view, cx| {
            let handle = view.editor.engine.handle(&"card".into()).unwrap();
            if self.bound {
                view.state_styles
                    .bind(&view.editor.engine, handle, styles())
                    .unwrap();
                view.state_styles
                    .set_disabled(&view.editor.engine, handle, self.disabled)
                    .unwrap();
                view.state_styles
                    .set_transitions(
                        &view.editor.engine,
                        handle,
                        StateTransitions {
                            transition: StateTransition::new(180),
                            transition_out: Some(StateTransition::new(320)),
                        },
                    )
                    .unwrap();
            } else {
                view.state_styles.unbind(handle);
            }
            cx.notify();
        });
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let disabled = if self.disabled {
            "Enable visuals"
        } else {
            "Disabled visuals"
        };
        let binding = if self.bound {
            "Remove styles"
        } else {
            "Bind styles"
        };
        let header = div()
            .min_h(px(58.))
            .flex()
            .items_center()
            .px_4()
            .gap_2()
            .bg(rgb(theme.panel))
            .child("State styles")
            .child(
                ControlButton::new("focus", "Focus card").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.view.update(cx, |view, cx| {
                            let handle = view.editor.engine.handle(&"card".into()).unwrap();
                            view.focus_element(handle, window, cx).unwrap();
                        });
                    },
                )),
            )
            .child(
                ControlButton::new("blur", "Clear focus").on_click(cx.listener(
                    |this, _, window, cx| {
                        this.view
                            .update(cx, |view, cx| view.clear_element_focus(window, cx));
                    },
                )),
            )
            .child(
                ControlButton::new("disabled", disabled)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_disabled(cx))),
            )
            .child(
                ControlButton::new("base", "Edit base fill")
                    .on_click(cx.listener(|this, _, _, cx| this.edit_base(cx))),
            )
            .child(
                ControlButton::new("undo", "Undo base edit").on_click(cx.listener(
                    |this, _, _, cx| {
                        this.view.update(cx, |view, cx| {
                            view.editor.undo();
                            cx.notify();
                        });
                    },
                )),
            )
            .child(
                ControlButton::new("redo", "Redo").on_click(cx.listener(|this, _, _, cx| {
                    this.view.update(cx, |view, cx| {
                        view.editor.redo();
                        cx.notify();
                    });
                })),
            )
            .child(
                ControlButton::new("bind", binding)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_binding(cx))),
            );
        let help = "180ms entry / 320ms exit · selected blue → focus purple → hover teal → press amber; disabled gray suppresses pointer colors. Hover never adds Undo. Disabled here is visual, not an access policy.";
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .font_family("sans-serif")
            .child(header)
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(
                div()
                    .min_h(px(54.))
                    .px_4()
                    .text_sm()
                    .text_color(rgb(theme.muted))
                    .child(help),
            )
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
                        size(px(1080.), px(700.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · state styles");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
