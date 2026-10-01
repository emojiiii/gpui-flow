//! Bounded inner shadows: original leaf alpha, ordered layers and late clipping.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneView,
    scene::{Affine, Color, DisplayObject, Scene, Shadow, Shape, Size, Stroke, Style, display},
};
fn layers() -> Vec<Shadow> {
    vec![
        Shadow::new(12., 8., 4., Color::rgba(255, 65, 120, 220)),
        Shadow::new(-8., -5., 0., Color::rgba(255, 215, 30, 180)),
    ]
}
fn document() -> Scene {
    let mut s = Scene::new();
    for (id, x, shape) in [
        (
            "rounded",
            70.,
            Shape::Rect(display::Rect::new(150., 110.).rounded(24.)),
        ),
        (
            "ellipse",
            330.,
            Shape::Ellipse(display::Ellipse::circle(60.)),
        ),
        (
            "text",
            600.,
            Shape::Text({
                let mut t = display::Text::new("Inner ink", Size::new(240., 80.));
                t.font_size = 48.;
                t
            }),
        ),
    ] {
        s.insert(
            DisplayObject::new(id, shape)
                .with_transform(Affine::translation(x, 95.))
                .with_style(Style {
                    fill: Some(Color::rgb(80, 180, 245).into()),
                    stroke: (id == "rounded").then(|| Stroke::new(Color::WHITE, 6.)),
                    shadow: Some(Shadow::new(15., 20., 2., Color::rgba(0, 0, 0, 180))),
                    inner_shadow_layers: layers(),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
    }
    s.insert(
        DisplayObject::new("frame", display::Frame::new(100., 120.).rounded(16.))
            .with_transform(Affine::translation(390., 265.))
            .with_style(Style {
                fill: None,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("clipped", display::Ellipse::circle(65.)).with_style(Style {
            fill: Some(Color::rgb(60, 220, 155).into()),
            inner_shadow_layers: layers(),
            ..Default::default()
        }),
        Some("frame".into()),
        None,
    )
    .unwrap();
    s
}
struct Lab {
    view: Entity<SceneView>,
}
impl Lab {
    fn edit(&mut self, id: &str, edit: impl FnOnce(&mut DisplayObject), cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            v.editor
                .transaction(|s| s.update_object(&id.into(), edit))
                .unwrap();
            cx.notify();
        });
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(rgb(0x171c27)).text_color(rgb(0xffffff))
            .child(div().h(px(50.)).px_4().flex().items_center().gap_2().child("Bounded inner shadows")
                .child(ControlButton::new("layers","Layers on / off").on_click(cx.listener(|this,_,_,cx|this.edit("rounded",|o|if o.style.inner_shadow_layers.is_empty(){o.style.inner_shadow_layers=layers();}else{o.style.inner_shadow_layers.clear();},cx))))
                .child(ControlButton::new("order","Swap colors").on_click(cx.listener(|this,_,_,cx|this.edit("ellipse",|o|o.style.inner_shadow_layers.reverse(),cx))))
                .child(ControlButton::new("offset","Offset").on_click(cx.listener(|this,_,_,cx|this.edit("ellipse",|o|{let s=&mut o.style.inner_shadow_layers[0];s.x=if s.x>0.{-24.}else{24.};},cx))))
                .child(ControlButton::new("blur","Text blur").on_click(cx.listener(|this,_,_,cx|this.edit("text",|o|{let s=&mut o.style.inner_shadow_layers[0];s.blur=if s.blur==0.{4.}else{0.};},cx)))))
            .child(div().h(px(48.)).px_4().flex().items_center().gap_2()
                .child(ControlButton::new("alpha","Source alpha").on_click(cx.listener(|this,_,_,cx|this.edit("ellipse",|o|{o.style.fill=Some(if o.style.fill==Some(Color::rgb(80,180,245).into()){Color::rgba(80,180,245,128)}else{Color::rgb(80,180,245)}.into());},cx))))
                .child(ControlButton::new("opacity","Object opacity").on_click(cx.listener(|this,_,_,cx|this.edit("rounded",|o|o.style.opacity=if o.style.opacity==1.{0.5}else{1.},cx))))
                .child(ControlButton::new("clip","Frame clip").on_click(cx.listener(|this,_,_,cx|this.edit("frame",|o|if let Shape::Frame(f)=&mut o.shape{f.clip_children = !f.clip_children;},cx))))
                .child(ControlButton::new("reject","Unsupported frame").on_click(cx.listener(|this,_,_,cx|this.edit("frame",|o|if o.style.inner_shadow_layers.is_empty(){o.style.inner_shadow_layers=layers();}else{o.style.inner_shadow_layers.clear();},cx))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();})))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(65.)).px_4().text_sm().child("Fill, inner layers, then white stroke. Black outer shadow samples original ink independently. Text follows glyph alpha.\nLower ellipse is clipped by a Frame. A Frame inner-shadow declaration explicitly rejects. Toggle back to recover.\nSelection remains geometric. Pan/zoom and Undo use the same retained Scene."))
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
                    size(px(1150.), px(680.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Bounded inner shadows");
                cx.new(|cx| Lab {
                    view: cx.new(|cx| {
                        let mut v = SceneView::new(document(), cx).unwrap();
                        v.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
                        v
                    }),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
