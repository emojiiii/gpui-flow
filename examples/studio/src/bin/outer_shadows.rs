//! Bounded real-alpha outer shadows through the same retained SceneView path.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneView,
    scene::{
        Affine, Color, DisplayObject, Scene, Shadow, Shape, Size, Stroke, StrokeAlignment, Style,
        display,
    },
};
use std::sync::Arc;
const ALPHA: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets/composite-alpha.svg");
fn shadow(color: Color) -> Option<Shadow> {
    Some(Shadow::new(14., 12., 6., color))
}
fn document() -> Scene {
    let mut s = Scene::new();
    s.insert(
        DisplayObject::new("vector", display::Rect::new(90., 70.).rounded(14.))
            .with_transform(Affine::translation(65., 85.))
            .with_style(Style {
                fill: Some(Color::rgba(60, 205, 210, 190).into()),
                stroke: Some(
                    Stroke::new(Color::WHITE, 3.).with_alignment(StrokeAlignment::Outside),
                ),
                shadow: shadow(Color::hex(0xff496c)),
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("image", display::Image::new(ALPHA, Size::new(80., 80.)))
            .with_transform(Affine::translation(280., 80.))
            .with_style(Style {
                shadow: shadow(Color::hex(0x3ddfaf)),
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    let mut text = display::Text::new("REAL INK", Size::new(170., 55.));
    text.font_size = 30.;
    text.font_family = "DejaVu Sans".into();
    s.insert(
        DisplayObject::new("text", text)
            .with_transform(Affine::translation(460., 95.))
            .with_style(Style {
                fill: Some(Color::WHITE.into()),
                shadow: shadow(Color::hex(0x42a5ff)),
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("clip", display::Frame::new(105., 105.).rounded(24.))
            .with_transform(Affine::translation(730., 65.))
            .with_style(Style::fill(Color::hex(0x55576c))),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("clipped", display::Ellipse::circle(32.))
            .with_transform(Affine::translation(38., 20.))
            .with_style(Style {
                fill: Some(Color::hex(0xffd66b).into()),
                shadow: shadow(Color::hex(0x111111)),
                ..Default::default()
            }),
        Some("clip".into()),
        None,
    )
    .unwrap();
    for (id, label, x) in [
        ("v-label", "Fill + aligned stroke", 40.),
        ("i-label", "Image alpha holes", 245.),
        ("t-label", "Native glyph alpha", 450.),
        ("c-label", "Rounded ancestor clip", 695.),
    ] {
        let mut t = display::Text::new(label, Size::new(215., 30.));
        t.font_size = 15.;
        s.insert(
            DisplayObject::new(id, t)
                .with_transform(Affine::translation(x, 25.))
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
    bridge: Arc<GpuiTextRasterizer>,
    text_enabled: bool,
}
impl Lab {
    fn new(bridge: Arc<GpuiTextRasterizer>, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
            v
        });
        Self {
            view,
            bridge,
            text_enabled: true,
        }
    }
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
        div().size_full().flex().flex_col().bg(rgb(0x161b25)).text_color(rgb(0xffffff))
            .child(div().h(px(58.)).px_4().flex().items_center().gap_2().child("Outer shadows")
                .child(ControlButton::new("blur","Blur 6 / 0").on_click(cx.listener(|this,_,_,cx|this.edit("vector",|o|{let s=o.style.shadow.as_mut().unwrap();s.blur=if s.blur==0.{6.}else{0.};},cx))))
                .child(ControlButton::new("offset","Flip offset").on_click(cx.listener(|this,_,_,cx|this.edit("vector",|o|o.style.shadow.as_mut().unwrap().x *= -1.,cx))))
                .child(ControlButton::new("opacity","Alpha 50 / 100").on_click(cx.listener(|this,_,_,cx|this.edit("vector",|o|o.style.opacity=if o.style.opacity==1.{0.5}else{1.},cx))))
                .child(ControlButton::new("clip","Clip on / off").on_click(cx.listener(|this,_,_,cx|this.edit("clip",|o|if let Shape::Frame(f)=&mut o.shape{f.clip_children = !f.clip_children;},cx))))
                .child(ControlButton::new("transform","Reject / restore").on_click(cx.listener(|this,_,_,cx|this.edit("vector",|o|o.transform.a=if o.transform.a==1.{1.4}else{1.},cx))))
                .child(ControlButton::new("bridge","Text bridge").on_click(cx.listener(|this,_,_,cx|{this.text_enabled = !this.text_enabled;let bridge=this.text_enabled.then(||this.bridge.clone());this.view.update(cx,|v,cx|v.set_text_rasterizer(bridge,cx).unwrap());cx.notify();})))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();})))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(86.)).px_4().text_sm().child("One alpha-derived outer shadow per leaf. Blur = 2 sigma; bounded CPU Gaussian, no browser-byte equivalence.\nClick the shape to select: shadow pixels do not enlarge selection or hit geometry. Drag, pan/zoom and Undo use the same Scene.\nGroup/Box/Frame shadows have a separate group_shadows lab. Non-uniform transforms and inner/spread/box/scale-fixed shadows remain unsupported."))
    }
}
fn main() {
    let platform = gpui_kit::platform::current_platform(false);
    let (app, bridge) = GpuiTextRasterizer::application(platform);
    app.with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(1100.), px(480.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Bounded outer shadows");
                cx.new(|cx| Lab::new(bridge.clone(), cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
