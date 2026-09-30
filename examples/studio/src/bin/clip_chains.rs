//! Nested native Text/Image clipping under real clip=true rectangular Frames.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, FlowTheme, SceneView,
    scene::{self, Affine, Color, DisplayObject, Scene, Shape, Size, Style, display},
};
fn document() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("outer", display::Frame::new(560., 380.))
                .with_transform(Affine::translation(90., 60.))
                .with_style(Style::fill(Color::hex(0x21314b))),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("inner", display::Frame::new(440., 310.))
                .with_transform(Affine::translation(300., 65.) * Affine::scale(1.1, 1.1))
                .with_style(Style::fill(Color::hex(0x375675))),
            Some("outer".into()),
            None,
        )
        .unwrap();
    let mut text = display::Text::new(
        "Nested Frame clip\n中文 / emoji 👩‍💻\nText keeps its complete alignment box",
        Size::new(500., 100.),
    );
    text.font_size = 25.;
    text.align = display::TextAlign::Center;
    scene
        .insert(
            DisplayObject::new("text", text)
                .with_transform(Affine::translation(-30., 15.))
                .with_style(Style::fill(Color::WHITE)),
            Some("inner".into()),
            None,
        )
        .unwrap();
    let mut image = display::Image::new(
        concat!(env!("CARGO_MANIFEST_DIR"), "/assets/checker.png"),
        Size::new(500., 210.),
    );
    image.fit = display::ImageFit::Cover;
    scene
        .insert(
            DisplayObject::new("image", image).with_transform(Affine::translation(-60., 135.)),
            Some("inner".into()),
            None,
        )
        .unwrap();
    scene
}
struct Lab {
    view: Entity<SceneView>,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        Self {
            view: cx.new(|cx| {
                let mut v = SceneView::new(document(), cx).unwrap();
                v.set_editable(false, cx);
                v
            }),
        }
    }
    fn edit(
        &mut self,
        f: impl FnOnce(&mut Scene) -> Result<(), scene::SceneError>,
        cx: &mut Context<Self>,
    ) {
        self.view.update(cx, |v, cx| {
            v.editor.transaction(f).unwrap();
            cx.notify();
        });
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground))
            .child(div().h(px(64.)).flex().items_center().px_4().gap_2().child("Nested rectangular Frame clips")
                .child(ControlButton::new("move","Move inner / empty").on_click(cx.listener(|this,_,_,cx|this.edit(|s|s.update_object(&"inner".into(),|o|{o.transform.tx=if o.transform.tx<550.{620.}else{250.};}),cx))))
                .child(ControlButton::new("clip","Outer clip on/off").on_click(cx.listener(|this,_,_,cx|this.edit(|s|s.update_object(&"outer".into(),|o|{let Shape::Frame(f)=&mut o.shape else{unreachable!()};f.clip_children = !f.clip_children;}),cx))))
                .child(ControlButton::new("fit","Cover / contain / fill").on_click(cx.listener(|this,_,_,cx|this.edit(|s|s.update_object(&"image".into(),|o|{let Shape::Image(i)=&mut o.shape else{unreachable!()};i.fit=match i.fit{display::ImageFit::Cover=>display::ImageFit::Contain,display::ImageFit::Contain=>display::ImageFit::Fill,display::ImageFit::Fill=>display::ImageFit::Cover};}),cx))))
                .child(ControlButton::new("zoom","Zoom 0.5 / 1").on_click(cx.listener(|this,_,_,cx|{this.view.update(cx,|v,cx|{v.editor.viewport.zoom=if v.editor.viewport.zoom==1.{0.5}else{1.};cx.notify();});cx.notify();})))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|{this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();});cx.notify();}))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(42.)).px_4().child("Text and image keep their full boxes; ancestor clips only hide pixels. No rounded/path masks in this slice."))
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
                    window.set_window_title("gpui-flow · nested native clips");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
