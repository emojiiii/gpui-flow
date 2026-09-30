//! Content measurement and input edits commit together, preserving normal Undo.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, SceneView, SceneViewEvent,
    layout::{AxisSizing, FlowLayout, FlowStyle, Insets},
    scene::{self, Affine, Color, DisplayObject, Scene, Style, display},
};
fn card(id: String) -> DisplayObject {
    let index = id.trim_start_matches("card-").parse::<usize>().unwrap_or(0);
    DisplayObject::new(id, display::Rect::new(140., 80.).rounded(12.)).with_style(Style::fill(
        Color::hex([0x46acaa, 0x8185da, 0xd29262, 0x698fbc][index % 4]),
    ))
}
fn document() -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new(
                "caption",
                display::Text::new(
                    "FIXED WIDTH / CONTENT HEIGHT / ONE EDIT + LAYOUT TRANSACTION",
                    scene::Size::new(1000., 28.),
                ),
            )
            .with_transform(Affine::translation(30., 10.))
            .with_style(Style::fill(Color::hex(0xbacbe1))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("frame", display::Frame::new(620., 600.).rounded(18.))
                .with_transform(Affine::translation(30., 55.))
                .with_style(Style::fill(Color::hex(0x192638))),
            None,
            None,
        )
        .unwrap();
    for i in 0..7 {
        scene
            .insert(card(format!("card-{i}")), Some("frame".into()), None)
            .unwrap();
    }
    let mut layout = FlowLayout::default();
    layout.containers.insert(
        "frame".into(),
        FlowStyle {
            height_sizing: AxisSizing::content(),
            padding: Insets::all(20.),
            gap: 16.,
            line_gap: 18.,
            wrap: true,
            ..Default::default()
        },
    );
    layout.apply_to_scene(&mut scene).unwrap();
    (scene, layout)
}
struct Lab {
    view: Entity<SceneView>,
    layout: FlowLayout,
    next: usize,
    message: String,
    _subscription: Subscription,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let (scene, layout) = document();
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene, cx).unwrap();
            view.set_editable(false, cx);
            view
        });
        let subscription = cx.subscribe(&view, |_, _, _: &SceneViewEvent, cx| cx.notify());
        Self {view,layout,next:7,message:"Resize the fixed width or add/remove cards: parent height and all positions update in the same Undo step".into(),_subscription:subscription}
    }
    fn change(
        &mut self,
        edit: impl FnOnce(&mut Scene) -> Result<(), scene::SceneError>,
        cx: &mut Context<Self>,
    ) {
        let result = self.view.update(cx, |view, cx| {
            let result = self.layout.transaction_editor(&mut view.editor, edit);
            cx.notify();
            result
        });
        self.message = match result {
            Ok(report) => format!(
                "{} measure/arrange passes; {} computed element changes in one edit",
                report.passes,
                report.changed.len()
            ),
            Err(error) => error.to_string(),
        };
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let scene = self.view.read(cx).editor.scene();
        let frame = scene.get(&"frame".into()).unwrap();
        let size = frame.object.shape.local_bounds().unwrap().size;
        let status = format!(
            "{} children | frame {:.0} × {:.0} | {}",
            frame.children.len(),
            size.width,
            size.height,
            self.message
        );
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme.background))
            .text_color(rgb(theme.foreground))
            .font_family("sans-serif")
            .child(
                div()
                    .min_h(px(60.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .bg(rgb(theme.panel))
                    .child("Content-sized layout")
                    .child(
                        ControlButton::new("narrow", "Width 360").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.change(
                                    |scene| {
                                        scene.update_object(&"frame".into(), |o| {
                                            let scene::Shape::Frame(frame) = &mut o.shape else {
                                                unreachable!()
                                            };
                                            frame.size.width = 360.;
                                        })
                                    },
                                    cx,
                                )
                            },
                        )),
                    )
                    .child(
                        ControlButton::new("wide", "Width 940").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.change(
                                    |scene| {
                                        scene.update_object(&"frame".into(), |o| {
                                            let scene::Shape::Frame(frame) = &mut o.shape else {
                                                unreachable!()
                                            };
                                            frame.size.width = 940.;
                                        })
                                    },
                                    cx,
                                )
                            },
                        )),
                    )
                    .child(ControlButton::new("add", "Add card").on_click(cx.listener(
                        |this, _, _, cx| {
                            let id = format!("card-{}", this.next);
                            this.next += 1;
                            this.change(
                                move |scene| {
                                    scene.insert(card(id), Some("frame".into()), None)?;
                                    Ok(())
                                },
                                cx,
                            );
                        },
                    )))
                    .child(
                        ControlButton::new("remove", "Remove card").on_click(cx.listener(
                            |this, _, _, cx| {
                                this.change(
                                    |scene| {
                                        if let Some(id) = scene
                                            .get(&"frame".into())
                                            .unwrap()
                                            .children
                                            .last()
                                            .cloned()
                                        {
                                            scene.remove(&id)?;
                                        }
                                        Ok(())
                                    },
                                    cx,
                                )
                            },
                        )),
                    )
                    .child(ControlButton::new("undo", "Undo").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.view.update(cx, |view, cx| {
                                view.editor.undo();
                                cx.notify();
                            });
                        },
                    )))
                    .child(ControlButton::new("redo", "Redo").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.view.update(cx, |view, cx| {
                                view.editor.redo();
                                cx.notify();
                            });
                        },
                    )))
                    .child(ControlButton::new("fit", "Fit").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.view.update(cx, |view, cx| {
                                view.fit();
                                cx.notify();
                            });
                        },
                    ))),
            )
            .child(div().flex_1().min_h_0().child(self.view.clone()))
            .child(
                div()
                    .min_h(px(40.))
                    .px_4()
                    .text_xs()
                    .text_color(rgb(theme.muted))
                    .child(status),
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
                        size(px(1160.), px(780.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · content-size layout");
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
