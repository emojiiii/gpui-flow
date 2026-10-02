//! State geometry and per-state timing through the actual SceneView pipeline.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, GpuiTextRasterizer, SceneView, SceneViewEvent,
    interaction::scene::{
        StateGeometry, StateGeometryStyles, StateKey, StatePaint, StateStyles, StateTransition,
        StateTransitionOverrides, StateTransitions,
    },
    scene::{Affine, BlendMode, Color, DisplayObject, Scene, Shape, Size, Style, display},
};
use std::{collections::BTreeMap, sync::Arc};

struct Lab {
    view: Entity<SceneView>,
    wide: bool,
    disabled: bool,
    edited: bool,
}
impl Lab {
    fn new(bridge: Arc<GpuiTextRasterizer>, cx: &mut Context<Self>) -> Self {
        let mut scene = Scene::new();
        scene
            .insert(
                DisplayObject::new("card", display::Frame::new(300., 180.).rounded(24.))
                    .with_transform(Affine::translation(130., 100.))
                    .with_style(Style {
                        blend_mode: BlendMode::Normal,
                        ..Style::fill(Color::hex(0x344760))
                    }),
                None,
                None,
            )
            .unwrap();
        let mut text = DisplayObject::new(
            "label",
            display::Text::new(
                "Hover / press the card\nGeometry and hit areas move together",
                Size::new(270., 80.),
            ),
        )
        .with_transform(Affine::translation(20., 50.))
        .with_style(Style::fill(Color::WHITE));
        text.hit_policy = gpui_flow::scene::HitPolicy::None;
        scene.insert(text, Some("card".into()), None).unwrap();
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene, cx).unwrap();
            view.set_editable(false, cx);
            view.set_input_tracing(true);
            view.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
            let h = view.editor.engine.handle(&"card".into()).unwrap();
            view.state_styles
                .bind(
                    &view.editor.engine,
                    h,
                    StateStyles {
                        focus: StatePaint::fill(Color::hex(0x9662ce)),
                        hover: StatePaint::fill(Color::hex(0x239e91)),
                        press: StatePaint::fill(Color::hex(0xd38b36)),
                        disabled: StatePaint {
                            fill: Some(Color::hex(0x586476)),
                            opacity: Some(0.55),
                        },
                        ..Default::default()
                    },
                )
                .unwrap();
            view.state_styles
                .set_geometry_styles(
                    &view.editor.engine,
                    h,
                    StateGeometryStyles {
                        focus: StateGeometry {
                            height: Some(230.),
                            ..Default::default()
                        },
                        hover: StateGeometry {
                            width: Some(430.),
                            height: Some(210.),
                            ..Default::default()
                        },
                        press: StateGeometry {
                            width: Some(340.),
                            height: Some(190.),
                            ..Default::default()
                        },
                        disabled: StateGeometry {
                            width: Some(260.),
                            height: Some(150.),
                            ..Default::default()
                        },
                        ..Default::default()
                    },
                )
                .unwrap();
            view.state_styles
                .set_named_geometry(
                    &view.editor.engine,
                    h,
                    BTreeMap::from([(
                        "wide".into(),
                        StateGeometry {
                            x: Some(360.),
                            width: Some(450.),
                            height: Some(240.),
                            ..Default::default()
                        },
                    )]),
                )
                .unwrap();
            view.state_styles
                .set_transitions(
                    &view.editor.engine,
                    h,
                    StateTransitions {
                        transition: StateTransition::new(300),
                        transition_out: Some(StateTransition::new(450)),
                    },
                )
                .unwrap();
            for (key, enter, exit) in [
                (StateKey::Hover, 180, 320),
                (StateKey::Press, 70, 180),
                (StateKey::Disabled, 0, 240),
                (StateKey::Named("wide".into()), 600, 500),
            ] {
                view.state_styles
                    .set_state_transitions(
                        &view.editor.engine,
                        h,
                        key,
                        Some(StateTransitionOverrides {
                            transition: Some(StateTransition::new(enter)),
                            transition_out: Some(StateTransition::new(exit)),
                        }),
                    )
                    .unwrap();
            }
            view
        });
        cx.subscribe(&view, |_: &mut Self, view, event, cx| {
            if let SceneViewEvent::InputTrace(trace) = event
                && !matches!(trace.operation, gpui_flow::SceneInputOperation::Canvas(gpui_flow::interaction::CanvasEvent::PointerMove(_)))
            {
                let view = view.read(cx);
                let card = view.presented_geometry().and_then(|scene| scene.get(&"card".into()));
                println!("state_geometry input={:?} accepted_x={:?} accepted_width={:?} canonical_revision={} pending={} state_error={:?} composite_errors={:?} errors={:?}",
                    trace.operation, card.map(|node| node.object.transform.tx),
                    card.and_then(|node| node.object.shape.local_bounds().map(|bounds| bounds.size.width)),
                    view.editor.engine.revision(), view.compositing_pending, view.state_style_error,
                    view.composite_diagnostics, trace.errors);
            }
        }).detach();
        Self {
            view,
            wide: false,
            disabled: false,
            edited: false,
        }
    }
    fn named(&mut self, cx: &mut Context<Self>) {
        self.wide = !self.wide;
        self.view.update(cx, |view, cx| {
            let h = view.editor.engine.handle(&"card".into()).unwrap();
            view.state_styles
                .select_state(&view.editor.engine, h, self.wide.then_some("wide"))
                .unwrap();
            println!(
                "state_geometry named={} canonical_revision={}",
                self.wide,
                view.editor.engine.revision()
            );
            cx.notify();
        });
        cx.notify();
    }
    fn disabled(&mut self, cx: &mut Context<Self>) {
        self.disabled = !self.disabled;
        self.view.update(cx, |view, cx| {
            let h = view.editor.engine.handle(&"card".into()).unwrap();
            view.state_styles
                .set_disabled(&view.editor.engine, h, self.disabled)
                .unwrap();
            println!(
                "state_geometry disabled={} canonical_revision={}",
                self.disabled,
                view.editor.engine.revision()
            );
            cx.notify();
        });
        cx.notify();
    }
    fn edit(&mut self, cx: &mut Context<Self>) {
        self.edited = !self.edited;
        self.view.update(cx, |view, cx| {
            view.editor
                .transaction(|scene| {
                    scene.update_object(&"card".into(), |object| {
                        object.transform.tx = if self.edited { 180. } else { 130. };
                        if let Shape::Frame(frame) = &mut object.shape {
                            frame.size.width = if self.edited { 360. } else { 300. };
                        }
                    })
                })
                .unwrap();
            println!(
                "state_geometry base_edit canonical_revision={}",
                view.editor.engine.revision()
            );
            cx.notify();
        });
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let header = div()
            .min_h(px(60.))
            .flex()
            .items_center()
            .gap_2()
            .px_4()
            .bg(rgb(theme.panel))
            .child("State geometry")
            .child(
                ControlButton::new(
                    "wide",
                    if self.wide {
                        "Clear named"
                    } else {
                        "Named wide"
                    },
                )
                .on_click(cx.listener(|this, _, _, cx| this.named(cx))),
            )
            .child(
                ControlButton::new("disabled", if self.disabled { "Enable" } else { "Disable" })
                    .on_click(cx.listener(|this, _, _, cx| this.disabled(cx))),
            )
            .child(ControlButton::new("focus", "Focus").on_click(cx.listener(
                |this, _, window, cx| {
                    this.view.update(cx, |view, cx| {
                        let h = view.editor.engine.handle(&"card".into()).unwrap();
                        view.focus_element(h, window, cx).unwrap();
                    })
                },
            )))
            .child(ControlButton::new("blur", "Blur").on_click(cx.listener(
                |this, _, window, cx| {
                    this.view
                        .update(cx, |view, cx| view.clear_element_focus(window, cx))
                },
            )))
            .child(
                ControlButton::new("edit", "Edit base")
                    .on_click(cx.listener(|this, _, _, cx| this.edit(cx))),
            )
            .child(
                ControlButton::new("undo", "Undo").on_click(cx.listener(|this, _, _, cx| {
                    this.view.update(cx, |view, cx| {
                        view.editor.undo();
                        cx.notify();
                    })
                })),
            )
            .child(
                ControlButton::new("redo", "Redo").on_click(cx.listener(|this, _, _, cx| {
                    this.view.update(cx, |view, cx| {
                        view.editor.redo();
                        cx.notify();
                    })
                })),
            );
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground)).font_family("sans-serif")
            .child(header).child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(div().min_h(px(62.)).px_4().text_sm().child("Hover: 180/320ms · Press: 70/180ms · Named: 600/500ms · Disabled: snap/240ms. Click in the expanded or relocated region. Edit base while active, clear state, then Undo/Redo. Runtime state never creates an Undo step."))
    }
}
fn main() {
    let (app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(false));
    app.with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1100.), px(700.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · state geometry");
                gpui_kit::component::Theme::change(
                    gpui_kit::component::ThemeMode::Dark,
                    Some(window),
                    cx,
                );
                cx.new(|cx| Lab::new(bridge, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
