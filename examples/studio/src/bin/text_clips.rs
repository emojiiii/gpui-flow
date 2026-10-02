//! Exact ancestor clips for real glyphs, without an unrelated blend/effect boundary.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneView,
    scene::{Affine, Color, DisplayObject, Scene, Shape, Size as SceneSize, Style, display},
};
use std::sync::Arc;
fn document() -> Scene {
    let mut s = Scene::new();
    for (i, label) in [
        (0, "Rectangle · native"),
        (1, "Rounded · CPU glyphs"),
        (2, "Affine clip · upright glyphs"),
    ] {
        let x = 35. + i as f32 * 285.;
        let mut title = display::Text::new(label, SceneSize::new(280., 30.));
        title.font_size = 16.;
        s.insert(
            DisplayObject::new(format!("title-{i}"), title)
                .with_transform(Affine::translation(x, 20.))
                .with_style(Style::fill(Color::WHITE)),
            None,
            None,
        )
        .unwrap();
        let local = if i == 2 {
            Affine::rotation(10.) * Affine::scale(1.05, 0.9)
        } else {
            Affine::IDENTITY
        };
        let frame = if i == 0 {
            display::Frame::new(250., 190.)
        } else {
            display::Frame::new(250., 190.).with_corners([70., 0., 60., 0.])
        };
        s.insert(
            DisplayObject::new(format!("frame-{i}"), frame)
                .with_transform(Affine::translation(x, 80.) * local)
                .with_style(Style::fill(Color::hex(0x203a52))),
            None,
            None,
        )
        .unwrap();
        let mut inner = display::Box::new(250., 190.)
            .clipped(true)
            .with_corners([0., 70., 0., 0.]);
        if i == 0 {
            inner.corner_radii = None;
        }
        s.insert(
            DisplayObject::new(format!("inner-{i}"), inner).with_style(Style {
                fill: None,
                ..Default::default()
            }),
            Some(format!("frame-{i}").into()),
            None,
        )
        .unwrap();
        let mut text = display::Text::new(
            "ABCD EFGH IJKL\nMNOP QRST UVWX\nYZ 0123456789\nffi café glyphs\nABCD EFGH IJKL\nMNOP QRST UVWX",
            SceneSize::new(250., 190.),
        );
        text.font_family = "DejaVu Sans".into();
        text.font_size = 27.;
        text.line_height = 1.1;
        text.wrap = false;
        s.insert(
            DisplayObject::new(format!("text-{i}"), text)
                .with_transform(local.inverse().unwrap())
                .with_style(Style::fill(Color::hex(0xffd87f))),
            Some(format!("inner-{i}").into()),
            None,
        )
        .unwrap();
    }
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
    fn edit(&mut self, action: u8, cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            v.editor
                .transaction(|s| {
                    for i in 0..3 {
                        match action {
                            0 if i > 0 => s.update_object(&format!("frame-{i}").into(), |o| {
                                if let Shape::Frame(f) = &mut o.shape {
                                    f.corner_radii = Some(
                                        if f.corner_radii.unwrap_or_default().0[0] > 0. {
                                            [0., 70., 0., 60.]
                                        } else {
                                            [70., 0., 60., 0.]
                                        }
                                        .into(),
                                    );
                                }
                            })?,
                            1 => {
                                s.update_object(&format!("frame-{i}").into(), |o| {
                                    if let Shape::Frame(f) = &mut o.shape {
                                        f.clip_children = !f.clip_children;
                                    }
                                })?;
                                s.update_object(&format!("inner-{i}").into(), |o| {
                                    if let Shape::Box(b) = &mut o.shape {
                                        b.clip_children = !b.clip_children;
                                    }
                                })?;
                            }
                            2 => s.update_object(&format!("text-{i}").into(), |o| {
                                if let Shape::Text(t) = &mut o.shape {
                                    t.padding = if t.padding == [0.; 4] {
                                        [20.; 4]
                                    } else {
                                        [0.; 4]
                                    };
                                }
                            })?,
                            _ => {}
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
        div().size_full().flex().flex_col().bg(rgb(0x0d1421)).text_color(rgb(0xe6edf5))
        .child(div().h(px(56.)).px_4().flex().items_center().gap_3()
            .child(ControlButton::new("corners","Swap corners").on_click(cx.listener(|this,_,_,cx|this.edit(0,cx))))
            .child(ControlButton::new("clip","Toggle clips").on_click(cx.listener(|this,_,_,cx|this.edit(1,cx))))
            .child(ControlButton::new("padding","Toggle padding").on_click(cx.listener(|this,_,_,cx|this.edit(2,cx))))
            .child(ControlButton::new("bridge",if self.enabled{"Disable glyph bridge"}else{"Enable glyph bridge"}).on_click(cx.listener(|this,_,_,cx|{this.enabled = !this.enabled;this.view.update(cx,|v,cx|v.set_text_rasterizer(this.enabled.then(||this.bridge.clone()),cx).unwrap());})))
            .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();})))))
        .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
        .child(div().h(px(75.)).px_4().text_sm().child("All containers use normal pass-through paint. Rounded and affine ancestor clips automatically create bounded Text domains.\nThe right Text stays upright through the inverse parent matrix. Missing bridge shows an explicit error. Swap / padding / clip edits participate in Undo."))
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
                    size(px(960.), px(550.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Text ancestor clips");
                cx.new(|cx| Lab::new(bridge, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
