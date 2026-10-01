//! Original GIF fixture, shared playback in native and composited routes.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    scene::{
        Affine, BlendMode, Color, DisplayObject, MaskMode, Scene, Shape, Size, Style, display,
    },
};
use std::time::Duration;
const ANIMATED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/composite-animation.gif"
);
const STATIC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/composite-alpha.svg");
fn document() -> Scene {
    let mut scene = Scene::new();
    for (i, label) in [
        (0, "Native selected frame"),
        (1, "Group alpha 50%"),
        (2, "Multiply on yellow"),
        (3, "Animated alpha mask"),
    ] {
        let x = 30. + i as f32 * 240.;
        scene
            .insert(
                DisplayObject::new(
                    format!("label-{i}"),
                    display::Text::new(label, Size::new(220., 30.)),
                )
                .with_transform(Affine::translation(x, 35.))
                .with_style(Style::fill(Color::WHITE)),
                None,
                None,
            )
            .unwrap();
        let parent = if i == 0 {
            None
        } else {
            let mut group = DisplayObject::new(format!("group-{i}"), display::Group::new())
                .with_transform(Affine::translation(x, 100.));
            group.style.blend_mode = BlendMode::Normal;
            if i == 1 {
                group.style.opacity = 0.5;
            }
            scene.insert(group, None, None).unwrap();
            Some(format!("group-{i}").into())
        };
        if i == 2 {
            scene
                .insert(
                    DisplayObject::new("backdrop", display::Rect::new(210., 180.))
                        .with_style(Style::fill(Color::hex(0xffdc55))),
                    parent.clone(),
                    None,
                )
                .unwrap();
        }
        let mut image = display::Image::new(ANIMATED, Size::new(210., 180.));
        image.fit = if i == 2 {
            display::ImageFit::Cover
        } else {
            display::ImageFit::Contain
        };
        let mut object = DisplayObject::new(format!("image-{i}"), image);
        if i == 0 {
            object.transform = Affine::translation(x, 100.);
        }
        if i == 2 {
            object.style.blend_mode = BlendMode::Multiply;
        }
        if i == 3 {
            object.mask = MaskMode::Alpha;
        }
        scene.insert(object, parent.clone(), None).unwrap();
        if i == 3 {
            scene
                .insert(
                    DisplayObject::new("mask-content", display::Rect::new(210., 180.))
                        .with_style(Style::fill(Color::hex(0x61d5ff))),
                    parent,
                    None,
                )
                .unwrap();
        }
    }
    scene
}
struct Lab {
    view: Entity<SceneView>,
    paused: bool,
    hidden: bool,
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
            paused: false,
            hidden: false,
            denied: false,
        }
    }
    fn seek(&mut self, ms: u64, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            for i in 0..4 {
                let handle = view
                    .editor
                    .engine
                    .handle(&format!("image-{i}").into())
                    .unwrap();
                view.seek_image(handle, Duration::from_millis(ms), cx);
            }
        });
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(rgb(0x11151d)).text_color(rgb(0xffffff))
        .child(div().h(px(66.)).px_3().flex().items_center().gap_2()
            .child("Animated image parity")
            .child(ControlButton::new("play",if self.paused {"Play"} else {"Pause"}).on_click(cx.listener(|this,_,_,cx| {
                this.paused = !this.paused;
                this.view.update(cx,|view,cx| for i in 0..4 {
                    let handle=view.editor.engine.handle(&format!("image-{i}").into()).unwrap();
                    view.set_image_playing(handle,!this.paused,cx);
                });cx.notify();
            })))
            .child(ControlButton::new("seek0","Seek red").on_click(cx.listener(|this,_,_,cx|this.seek(0,cx))))
            .child(ControlButton::new("seek1","Seek green").on_click(cx.listener(|this,_,_,cx|this.seek(250,cx))))
            .child(ControlButton::new("seek2","Seek blue").on_click(cx.listener(|this,_,_,cx|this.seek(700,cx))))
            .child(ControlButton::new("hide","Hide / show").on_click(cx.listener(|this,_,_,cx| {
                this.hidden = !this.hidden;
                this.view.update(cx,|view,cx| {view.editor.transaction(|s| {
                    for id in ["image-0","group-1","group-2","group-3"] { s.update_object(&id.into(),|o|o.visible = !this.hidden)?; } Ok(())
                }).unwrap();cx.notify();});
            })))
            .child(ControlButton::new("source","Static / animated").on_click(cx.listener(|this,_,_,cx| {
                this.view.update(cx,|view,cx| {view.editor.transaction(|s| {
                    for i in 0..4 {s.update_object(&format!("image-{i}").into(),|o| {if let Shape::Image(image)=&mut o.shape {
                        image.source=if image.source==ANIMATED { STATIC.into() } else { ANIMATED.into() };
                    }})?;} Ok(())
                }).unwrap();cx.notify();});this.paused=false;cx.notify();
            })))
            .child(ControlButton::new("deny","Deny / allow").on_click(cx.listener(|this,_,_,cx| {
                this.denied = !this.denied;let denied=this.denied;
                this.view.update(cx,|view,cx|view.set_image_resolver(std::rc::Rc::new(move|source|if denied {None} else {gpui_flow::resources::local_image_resource(source)}),cx).unwrap());
                this.paused=false;cx.notify();
            }))))
        .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
        .child(div().h(px(60.)).px_4().text_sm().child("Real GIF: red 200ms → green 400ms → blue 600ms. Native and CPU use the same per-instance selected frame.\nPause then seek; hide/show; switch source A/B/A; deny/allow. Hidden or inactive windows freeze playback."))
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
                        size(px(1040.), px(540.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · animated image compositing");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
