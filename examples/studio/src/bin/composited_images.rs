//! Static GPUI assets inside actual CPU isolation/mask/blend domains.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    scene::{
        Affine, BlendMode, Color, DisplayObject, MaskMode, Scene, Shape, Size, Style, display,
    },
};
const ALPHA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/composite-alpha.svg");
const CHECKER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/checker.png");
const MISSING: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/intentionally-missing.png"
);
fn document() -> Scene {
    let mut s = Scene::new();
    for (i, fit, label) in [
        (0, display::ImageFit::Contain, "Contain + group alpha 50%"),
        (1, display::ImageFit::Cover, "Cover + multiply on yellow"),
        (2, display::ImageFit::Fill, "Fill + ellipse alpha mask"),
    ] {
        let id = format!("group-{i}");
        let mut group = DisplayObject::new(id.clone(), display::Group::new())
            .with_transform(Affine::translation(40. + i as f32 * 300., 110.));
        group.style.blend_mode = BlendMode::Normal;
        if i == 0 {
            group.style.opacity = 0.5;
        }
        s.insert(group, None, None).unwrap();
        s.insert(
            DisplayObject::new(
                format!("label-{i}"),
                display::Text::new(label, Size::new(280., 35.)),
            )
            .with_transform(Affine::translation(40. + i as f32 * 300., 55.))
            .with_style(Style::fill(Color::WHITE)),
            None,
            None,
        )
        .unwrap();
        if i == 1 {
            s.insert(
                DisplayObject::new("backdrop", display::Rect::new(240., 240.))
                    .with_style(Style::fill(Color::hex(0xffdd33))),
                Some(id.clone().into()),
                None,
            )
            .unwrap();
        }
        if i == 2 {
            s.insert(
                DisplayObject::new("mask", display::Ellipse::new(240., 240.))
                    .with_mask(MaskMode::Alpha)
                    .with_style(Style::fill(Color::WHITE)),
                Some(id.clone().into()),
                None,
            )
            .unwrap();
        }
        let mut image = display::Image::new(ALPHA, Size::new(240., 240.));
        image.fit = fit;
        let mut object = DisplayObject::new(format!("image-{i}"), image);
        if i == 1 {
            object.style.blend_mode = BlendMode::Multiply;
        }
        s.insert(object, Some(id.into()), None).unwrap();
    }
    s
}
struct Lab {
    view: Entity<SceneView>,
    denied: bool,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            view: cx.new(|cx| {
                let mut v = SceneView::new(document(), cx).unwrap();
                v.show_grid = false;
                v.set_editable(false, cx);
                v
            }),
            denied: false,
        }
    }
    fn edit_images(&mut self, f: impl Fn(&mut display::Image), cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            v.editor
                .transaction(|s| {
                    for i in 0..3 {
                        s.update_object(&format!("image-{i}").into(), |o| {
                            if let Shape::Image(i) = &mut o.shape {
                                f(i);
                            }
                        })?;
                    }
                    Ok(())
                })
                .unwrap();
            cx.notify();
        });
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(0x161b25))
            .text_color(rgb(0xffffff))
            .child(
                div()
                    .h(px(58.))
                    .px_4()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child("Static image compositing")
                    .child(ControlButton::new("source", "SVG / PNG").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.edit_images(
                                |image| image.source = if image.source == ALPHA { CHECKER.into() } else { ALPHA.into() },
                                cx,
                            );
                        },
                    )))
                    .child(ControlButton::new("missing", "Missing / restore").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.edit_images(
                                |image| image.source = if image.source == MISSING { ALPHA.into() } else { MISSING.into() },
                                cx,
                            );
                        },
                    )))
                    .child(ControlButton::new("auth", "Deny / authorize").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.denied = !this.denied;
                            let denied = this.denied;
                            this.view.update(cx, |view, cx| {
                                view.set_image_resolver(
                                    std::rc::Rc::new(move |source| {
                                        if denied { None } else { gpui_flow::resources::local_image_resource(source) }
                                    }),
                                    cx,
                                ).unwrap();
                            });
                        },
                    )))
                    .child(ControlButton::new("blend", "Multiply / Screen").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.view.update(cx, |view, cx| {
                                view.editor.transaction(|scene| {
                                    scene.update_object(&"image-1".into(), |object| {
                                        object.style.blend_mode = if object.style.blend_mode == BlendMode::Multiply {
                                            BlendMode::Screen
                                        } else {
                                            BlendMode::Multiply
                                        };
                                    })
                                }).unwrap();
                                cx.notify();
                            });
                        },
                    )))
                    .child(ControlButton::new("undo", "Undo").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.view.update(cx, |view, cx| {
                                view.editor.undo();
                                cx.notify();
                            });
                        },
                    ))),
            )
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(64.)).px_4().text_sm().child(
                "GPUI loader/cache supplies static pixels. Transparent bottom half keeps its alpha through group/mask/blend.\nToggle source quickly (A/B/A), fail/recover, revoke/restore, Undo and pan/zoom. Text stays native outside domains.",
            ))
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
                        size(px(1000.), px(600.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · static image compositing");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
