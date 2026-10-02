//! Same-App glyphs and measurement inside atomic Canvas command batches.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneView,
    composite::{CanvasCommand, ImageInputs},
    scene::{
        Affine, Color, DisplayObject, FillRule, Point as ScenePoint, Scene, Shape,
        Size as SceneSize, Stroke, Style, display,
    },
};
fn path(s: &str) -> display::Path {
    display::Path::from_svg(s).unwrap()
}
fn text() -> display::Text {
    let mut t = display::Text::new(
        "Canvas ffi café · 漢字\nReal glyphs and end ellipsis in a fixed box",
        SceneSize::new(250., 80.),
    );
    t.font_family = "DejaVu Sans".into();
    t.font_size = 23.;
    t.padding = [5.; 4];
    t.overflow = display::TextOverflow::Ellipsis;
    t.decoration = display::TextDecoration::Underline;
    t
}
fn commands(mode: u8) -> Vec<CanvasCommand> {
    let mut commands = Vec::new();
    if mode > 0 {
        commands.push(CanvasCommand::ClipPath {
            path: path("M 5 90 C 5 -30 275 -30 275 90 C 275 220 5 220 5 90 Z"),
            rule: FillRule::NonZero,
        });
    }
    if mode > 1 {
        commands.push(CanvasCommand::ClipPath {
            path: path("M 0 0 L 280 0 L 280 180 L 0 180 Z M 90 70 L 150 70 L 150 150 L 90 150 Z"),
            rule: FillRule::EvenOdd,
        });
    }
    let mut italic = text();
    italic.content = "Affine words\nffi café".into();
    italic.italic = true;
    italic.font_size = 20.;
    italic.size = SceneSize::new(175., 70.);
    commands.extend([
        CanvasCommand::FillTextBox {
            text: text(),
            position: ScenePoint::new(10., 10.),
            color: Color::hex(0xffdc73),
        },
        CanvasCommand::Save,
        CanvasCommand::Transform(Affine::translation(58., 92.) * Affine::rotation(12.)),
        CanvasCommand::SetOpacity(0.65),
        CanvasCommand::FillTextBox {
            text: italic,
            position: ScenePoint::ZERO,
            color: Color::hex(0x78d6ff),
        },
        CanvasCommand::Restore,
    ]);
    commands
}
fn document() -> Scene {
    let mut s = Scene::new();
    for (id, x, dpr, label) in [
        ("one", 35., 1., "Text batch · DPR 1"),
        ("two", 395., 2., "Text batch · DPR 2"),
    ] {
        let mut c = display::Canvas::new(280., 180.).with_corners([18., 0., 18., 0.]);
        c.pixel_ratio = dpr;
        c.safe_resize = true;
        s.insert(
            DisplayObject::new(id, c)
                .with_transform(Affine::translation(x, 65.))
                .with_style(Style {
                    fill: Some(Color::hex(0x182b42).into()),
                    stroke: Some(Stroke::new(Color::hex(0x439cc3), 2.)),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
        let mut t = display::Text::new(label, SceneSize::new(340., 35.));
        t.font_size = 18.;
        s.insert(
            DisplayObject::new(format!("label-{id}"), t)
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
    status: String,
    clip_mode: u8,
    bridge: std::sync::Arc<GpuiTextRasterizer>,
    enabled: bool,
}
impl Lab {
    fn new(bridge: std::sync::Arc<GpuiTextRasterizer>, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.show_grid = false;
            v.set_editable(false, cx);
            v.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
            v
        });
        let mut lab = Self {
            view,
            status: String::new(),
            clip_mode: 0,
            bridge,
            enabled: true,
        };
        lab.paint(0, cx);
        lab
    }
    fn paint(&mut self, action: u8, cx: &mut Context<Self>) {
        let mut measured = String::new();
        let result = self.view.update(cx, |v, cx| {
            let t = text();
            let request = gpui_flow::text::TextMeasureRequest {
                content: t.content,
                font_family: t.font_family,
                font_size: t.font_size,
                font_weight: t.font_weight,
                italic: t.italic,
                line_height: t.font_size * t.line_height,
                wrap_width: Some(240.),
            };
            measured = match v.measure_canvas_text(&request, cx) {
                Ok(m) => format!(
                    "Shaped metrics: {:.1} x {:.1}, {} rows. ",
                    m.width, m.height, m.lines
                ),
                Err(e) => format!("Measurement error: {e}. "),
            };
            for id in ["one", "two"] {
                let h = v.editor.engine.handle(&id.into()).unwrap();
                if action == 3 {
                    v.edit_canvas(h, |s| s.clear(), cx)?;
                    continue;
                }
                if action == 0 && self.enabled {
                    v.edit_canvas(h, |s| s.clear(), cx)?;
                }
                let mut batch = commands(self.clip_mode);
                if action == 2 {
                    batch.push(CanvasCommand::Restore);
                }
                v.draw_canvas_commands(h, &batch, &ImageInputs::default(), cx)?;
            }
            Ok::<(), gpui_flow::resources::CanvasEditError>(())
        });
        self.status = measured
            + &match result {
                Ok(()) => "Text batch accepted".into(),
                Err(e) => format!("Rejected; current pixels retained: {e}"),
            };
        cx.notify();
    }
    fn backend(&mut self, cx: &mut Context<Self>) {
        self.enabled = !self.enabled;
        self.view.update(cx, |v, cx| {
            v.set_text_rasterizer(self.enabled.then(|| self.bridge.clone()), cx)
                .unwrap()
        });
        self.paint(0, cx);
    }
    fn resize(&mut self, safe: bool, cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            v.editor
                .transaction(|s| {
                    for id in ["one", "two"] {
                        s.update_object(&id.into(), |o| {
                            if let Shape::Canvas(c) = &mut o.shape {
                                c.safe_resize = safe;
                                c.size.width = if c.size.width > 200. { 150. } else { 280. };
                            }
                        })?;
                    }
                    Ok(())
                })
                .unwrap();
            cx.notify();
        });
        self.status = if safe {
            "Safe physical-pixel crop; growing does not resurrect the cropped side."
        } else {
            "Reset resize clears pixels. Undo restores configuration, not commands."
        }
        .into();
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(rgb(0x0d1421)).text_color(rgb(0xe6edf5))
        .child(div().h(px(56.)).px_4().flex().items_center().gap_3()
            .child(ControlButton::new("draw","Draw text batch").on_click(cx.listener(|s,_,_,cx|s.paint(0,cx))))
            .child(ControlButton::new("alpha","Font bridge").on_click(cx.listener(|s,_,_,cx|s.backend(cx))))
            .child(ControlButton::new("reject","Reject late command").on_click(cx.listener(|s,_,_,cx|s.paint(2,cx))))
            .child(ControlButton::new("safe","Safe shrink/grow").on_click(cx.listener(|s,_,_,cx|s.resize(true,cx))))
            .child(ControlButton::new("reset","Reset shrink/grow").on_click(cx.listener(|s,_,_,cx|s.resize(false,cx))))
            .child(ControlButton::new("undo","Undo config").on_click(cx.listener(|s,_,_,cx|s.view.update(cx,|v,cx|{v.editor.undo();cx.notify();}))))
            .child(ControlButton::new("clip","Cycle clip").on_click(cx.listener(|s,_,_,cx|{s.clip_mode=(s.clip_mode+1)%3;s.paint(0,cx);})))
            .child(ControlButton::new("clear","Clear").on_click(cx.listener(|s,_,_,cx|s.paint(3,cx)))))
        .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
        .child(div().h(px(96.)).px_4().text_sm().child("Clip: none / curved / curved + hole. Actual same-App glyphs, Unicode, ellipsis, underline and rotated italic text.\nSave/restore includes clipping, transform and opacity only inside a batch. DPR maps logical coordinates once.\n").child(self.status.clone()))
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
                    size(px(1100.), px(580.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Canvas text commands");
                cx.new(|cx| Lab::new(bridge, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
