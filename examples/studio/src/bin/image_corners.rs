//! Exact Image corners across GPUI/CPU, fit modes, ancestor clips and animation.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    scene::{
        Affine, Color, DisplayObject, Point as ScenePoint, Scene, Shape, Size as SceneSize, Stroke,
        Style, display,
    },
};
const STATIC: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/composite-alpha.svg");
const ANIMATED: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/assets/composite-animation.gif"
);
fn document() -> Scene {
    let mut s = Scene::new();
    for (index, fit, label) in [
        (0, display::ImageFit::Cover, "Cover · native when exact"),
        (1, display::ImageFit::Fill, "Fill · native when exact"),
        (
            2,
            display::ImageFit::Contain,
            "Contain · exact CPU geometry",
        ),
    ] {
        let x = 35. + index as f32 * 235.;
        let id = format!("frame-{index}");
        let mut frame = display::Frame::new(190., 190.).with_corners([0., 65., 0., 45.]);
        frame.clip_children = false;
        s.insert(
            DisplayObject::new(id.clone(), frame)
                .with_transform(Affine::translation(x, 65.))
                .with_style(Style {
                    fill: Some(Color::hex(0x243447).into()),
                    stroke: Some(Stroke::new(Color::hex(0x71869a), 1.)),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
        let mut image = display::Image::new(STATIC, SceneSize::new(190., 190.))
            .with_corners([80., 0., 55., 0.]);
        image.fit = fit;
        s.insert(
            DisplayObject::new(format!("image-{index}"), image),
            Some(id.into()),
            None,
        )
        .unwrap();
        let mut text = display::Text::new(label, SceneSize::new(225., 35.));
        text.font_size = 15.;
        s.insert(
            DisplayObject::new(format!("label-{index}"), text)
                .with_transform(Affine::translation(x, 20.))
                .with_style(Style::fill(Color::WHITE)),
            None,
            None,
        )
        .unwrap();
    }
    s
}
struct Lab {
    view: Entity<SceneView>,
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
        }
    }
    fn edit(&mut self, action: u8, cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            v.editor
                .transaction(|s| {
                    for i in 0..3 {
                        let id = format!("image-{i}");
                        match action {
                            0 => s.update_object(&id.into(), |o| {
                                if let Shape::Image(image) = &mut o.shape {
                                    image.corner_radii = Some(
                                        if image.effective_corners().0[0] > 0. {
                                            [0., 80., 0., 55.]
                                        } else {
                                            [80., 0., 55., 0.]
                                        }
                                        .into(),
                                    );
                                }
                            })?,
                            1 => s.update_object(&format!("frame-{i}").into(), |o| {
                                if let Shape::Frame(f) = &mut o.shape {
                                    f.clip_children = !f.clip_children;
                                }
                            })?,
                            2 => s.update_object(&id.into(), |o| {
                                o.transform = if o.transform == Affine::IDENTITY {
                                    Affine::rotation(18.).around(ScenePoint::new(95., 95.))
                                } else {
                                    Affine::IDENTITY
                                }
                            })?,
                            _ => s.update_object(&id.into(), |o| {
                                if let Shape::Image(image) = &mut o.shape {
                                    image.source = if image.source == STATIC {
                                        ANIMATED
                                    } else {
                                        STATIC
                                    }
                                    .into();
                                }
                            })?,
                        }
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
        div().size_full().flex().flex_col().bg(rgb(0x0d1421)).text_color(rgb(0xe6edf5)).child(div().h(px(56.)).px_4().flex().items_center().gap_3()
.child(ControlButton::new("corners","Swap corners").on_click(cx.listener(|this,_,_,cx|this.edit(0,cx))))
.child(ControlButton::new("clip","Ancestor clip").on_click(cx.listener(|this,_,_,cx|this.edit(1,cx))))
.child(ControlButton::new("rotate","Rotate image").on_click(cx.listener(|this,_,_,cx|this.edit(2,cx))))
.child(ControlButton::new("animate","Static / animated").on_click(cx.listener(|this,_,_,cx|this.edit(3,cx))))
.child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();})))))
.child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
.child(div().h(px(85.)).px_4().text_sm().child("Each Image has independent corners; Contain keeps its letterbox. Cover/Fill use native GPUI when exact.\nRounded ancestor clipping or rotation automatically selects a bounded CPU image domain. Static and animated assets use the existing resource pipeline.\nSwap corners, toggle clip/rotation/source, then Undo. No bounding-box fallback."))
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
                        size(px(800.), px(520.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Image corners");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
