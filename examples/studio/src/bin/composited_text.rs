//! Host-opt-in text ink, sharing the exact native GPUI platform and font IDs.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneView,
    scene::{
        Affine, BlendMode, Color, DisplayObject, MaskMode, Scene, Shape, Size, Style, display,
    },
};
use std::sync::Arc;
fn text(content: &str, width: f32, height: f32, font: f32) -> display::Text {
    let mut t = display::Text::new(content, Size::new(width, height));
    t.font_family = "DejaVu Sans".into();
    t.font_size = font;
    t.line_height = 1.35;
    t
}
fn document() -> Scene {
    let mut s = Scene::new();
    let content = "office ffi · café e\u{301}\n你好，画布世界 👩‍💻🌍\nA wrapped line of text preserves native shaping and font fallback.\n\nLast line";
    for (id, x, composite) in [("native", 35., false), ("cpu", 505., true)] {
        s.insert(
            DisplayObject::new(id, display::Group::new())
                .with_transform(Affine::translation(x, 55.))
                .with_style(Style {
                    blend_mode: if composite {
                        BlendMode::Normal
                    } else {
                        BlendMode::PassThrough
                    },
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
        s.insert(
            DisplayObject::new(format!("{id}-text"), text(content, 420., 225., 25.))
                .with_style(Style::fill(Color::WHITE)),
            Some(id.into()),
            None,
        )
        .unwrap();
    }
    s.insert(
        DisplayObject::new("mask-domain", display::Group::new())
            .with_transform(Affine::translation(35., 325.))
            .with_style(Style {
                blend_mode: BlendMode::Normal,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new("glyph-mask", text("REAL INK", 410., 100., 65.))
            .with_mask(MaskMode::Alpha)
            .with_style(Style::fill(Color::WHITE)),
        Some("mask-domain".into()),
        None,
    )
    .unwrap();
    for (id, x, color) in [("mask-red", 0., 0xff5b80), ("mask-blue", 180., 0x45caff)] {
        s.insert(
            DisplayObject::new(id, display::Rect::new(230., 120.))
                .with_transform(Affine::translation(x, 0.))
                .with_style(Style::fill(Color::hex(color))),
            Some("mask-domain".into()),
            None,
        )
        .unwrap();
    }
    s.insert(
        DisplayObject::new("clip", display::Frame::new(420., 150.).rounded(35.))
            .with_transform(Affine::translation(505., 325.))
            .with_style(Style {
                fill: Some(Color::hex(0xffd653).into()),
                blend_mode: BlendMode::Normal,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    s.insert(
        DisplayObject::new(
            "clip-text",
            text("Clipped glyphs\n中文 + emoji 🌍", 500., 180., 40.),
        )
        .with_transform(Affine::translation(-18., 5.))
        .with_style(Style {
            fill: Some(Color::hex(0x276bfa).into()),
            blend_mode: BlendMode::Multiply,
            ..Default::default()
        }),
        Some("clip".into()),
        None,
    )
    .unwrap();
    s
}
struct Lab {
    view: Entity<SceneView>,
    bridge: Arc<GpuiTextRasterizer>,
    enabled: bool,
}
impl Lab {
    fn new(bridge: Arc<GpuiTextRasterizer>, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.show_grid = false;
            v.set_editable(false, cx);
            v.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
            v
        });
        Self {
            view,
            bridge,
            enabled: true,
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
            .child(div().h(px(56.)).px_4().flex().items_center().gap_2().child("Same-backend text")
                .child(ControlButton::new("opacity","Alpha 50% / 100%").on_click(cx.listener(|this,_,_,cx|this.edit("cpu",|o|o.style.opacity=if o.style.opacity==1.{0.5}else{1.},cx))))
                .child(ControlButton::new("align","Align").on_click(cx.listener(|this,_,_,cx|this.edit("cpu-text",|o|if let Shape::Text(t)=&mut o.shape {t.align=match t.align {display::TextAlign::Start=>display::TextAlign::Center,display::TextAlign::Center=>display::TextAlign::End,display::TextAlign::End=>display::TextAlign::Start};},cx))))
                .child(ControlButton::new("backend","Bridge on / off").on_click(cx.listener(|this,_,_,cx|{this.enabled = !this.enabled;let bridge=this.enabled.then(||this.bridge.clone());this.view.update(cx,|v,cx|v.set_text_rasterizer(bridge,cx).unwrap());cx.notify();})))
                .child(ControlButton::new("fonts","Invalidate fonts").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|v.invalidate_text_resources(cx).unwrap()))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();})))))
            .child(div().px_4().h(px(28.)).child("Left: native GPUI   |   Right: CPU domain using the same font backend"))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(66.)).px_4().text_sm().child("Bottom: glyph-alpha mask and rounded clipping with internal Multiply.\nHost opt-in; plain solid text and positive uniform scale. Transparent layers use grayscale coverage; native LCD may look slightly different."))
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
                    size(px(1000.), px(700.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Same-backend composited text");
                cx.new(|cx| Lab::new(bridge.clone(), cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
