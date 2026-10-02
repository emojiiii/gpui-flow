//! Compare native and CPU glyphs using one Text inner-box contract and backend.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneView,
    scene::{
        Affine, BlendMode, Color, DisplayObject, Scene, Shape, Size as SceneSize, Stroke, Style,
        display::{self, TextOverflow, TextVerticalAlign},
    },
};
use std::sync::Arc;
const NORMAL: &str = "A long fixed-box title with ffi and café. Native and CPU paths share actual glyph shaping, wrapping and end ellipsis.\nA second paragraph must not disappear silently.";
fn document() -> Scene {
    let mut scene = Scene::new();
    for (id, x, blend, label) in [
        ("native", 35., BlendMode::PassThrough, "Native glyph path"),
        (
            "cpu",
            365.,
            BlendMode::Normal,
            "CPU glyph path, same backend",
        ),
    ] {
        let mut title = display::Text::new(label, SceneSize::new(270., 30.));
        title.font_size = 17.;
        title.wrap = false;
        scene
            .insert(
                DisplayObject::new(format!("{id}-title"), title)
                    .with_transform(Affine::translation(x, 25.))
                    .with_style(Style::fill(Color::WHITE)),
                None,
                None,
            )
            .unwrap();
        scene
            .insert(
                DisplayObject::new(format!("{id}-box"), display::Rect::new(280., 130.))
                    .with_transform(Affine::translation(x, 70.))
                    .with_style(Style {
                        fill: Some(Color::hex(0x142133).into()),
                        stroke: Some(Stroke::new(Color::hex(0x4176a5), 2.)),
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
        let mut text = display::Text::new(NORMAL, SceneSize::new(280., 130.));
        text.font_family = "DejaVu Sans".into();
        text.font_size = 22.;
        text.overflow = TextOverflow::Ellipsis;
        text.line_height = 1.4;
        text.padding = [12.; 4];
        scene
            .insert(
                DisplayObject::new(id, text)
                    .with_transform(Affine::translation(x, 70.))
                    .with_style(Style {
                        fill: Some(Color::hex(0xf2d289).into()),
                        blend_mode: blend,
                        ..Default::default()
                    }),
                None,
                None,
            )
            .unwrap();
    }
    scene
}
struct Lab {
    view: Entity<SceneView>,
}
impl Lab {
    fn new(bridge: Arc<GpuiTextRasterizer>, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.show_grid = false;
            v.set_editable(false, cx);
            v.set_text_rasterizer(Some(bridge), cx).unwrap();
            v
        });
        Self { view }
    }
    fn edit(&mut self, action: u8, cx: &mut Context<Self>) {
        self.view.update(cx,|v,cx|{
            v.editor.transaction(|s|{for id in ["native","cpu"] {
                s.update_object(&id.into(),|o|if let Shape::Text(t)=&mut o.shape{match action {
                    0=>t.overflow=if t.overflow==TextOverflow::Clip{TextOverflow::Ellipsis}else{TextOverflow::Clip},
                    1=>t.wrap=!t.wrap,
                    2=>t.size.height=if t.size.height<150.{230.}else{130.},
                    3=>t.vertical_align=match t.vertical_align{TextVerticalAlign::Top=>TextVerticalAlign::Middle,TextVerticalAlign::Middle=>TextVerticalAlign::Bottom,TextVerticalAlign::Bottom=>TextVerticalAlign::Top},
                    _=>t.content=if t.content==NORMAL{"Cafe\u{301} 👩‍👩‍👧‍👦 🇨🇦 ffi 漢字 · repeated Unicode text that should end at a grapheme boundary.\nMore content remains in the document.".into()}else{NORMAL.into()},
                }})?;
                if action==2 {s.update_object(&format!("{id}-box").into(),|o|if let Shape::Rect(r)=&mut o.shape{r.size.height=if r.size.height<150.{230.}else{130.};})?;}
            }Ok(())}).unwrap();cx.notify();
        });
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(rgb(0x0d1421)).text_color(rgb(0xe6edf5))
        .child(div().h(px(56.)).px_4().flex().items_center().gap_3()
            .child(ControlButton::new("ellipsis","Clip / ellipsis").on_click(cx.listener(|this,_,_,cx|this.edit(0,cx))))
            .child(ControlButton::new("wrap","Wrap / nowrap").on_click(cx.listener(|this,_,_,cx|this.edit(1,cx))))
            .child(ControlButton::new("height","Short / tall").on_click(cx.listener(|this,_,_,cx|this.edit(2,cx))))
            .child(ControlButton::new("align","Vertical align").on_click(cx.listener(|this,_,_,cx|this.edit(3,cx))))
            .child(ControlButton::new("unicode","Unicode / prose").on_click(cx.listener(|this,_,_,cx|this.edit(4,cx))))
            .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();})))))
        .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
        .child(div().h(px(75.)).px_4().text_sm().child("Opt-in end ellipsis uses the inner padded box. Native and CPU share GPUI rows and grapheme-safe truncation.\nOriginal content and intrinsic measurement remain intact; vertical overflow stays top-aligned. Toggle policies / height / Unicode and Undo."))
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
                    size(px(860.), px(550.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Text end ellipsis");
                cx.new(|cx| Lab::new(bridge, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
