//! Immutable image inputs inside the existing atomic Canvas command batch.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    composite::{CanvasCommand, ImageInputs, StaticImage},
    scene::{
        Affine, Bounds as SceneBounds, Color, DisplayObject, FillRule, Point as ScenePoint, Scene,
        Shape, Size as SceneSize, Stroke, Style, display,
    },
};
fn path(s: &str) -> display::Path {
    display::Path::from_svg(s).unwrap()
}
fn fill(s: &str, color: Color) -> CanvasCommand {
    CanvasCommand::FillPath {
        path: path(s),
        paint: color.into(),
        rule: FillRule::NonZero,
    }
}
fn inputs() -> ImageInputs {
    let mut rgba = Vec::new();
    for y in 0..64 {
        for x in 0..96 {
            let color = if x < 48 && y < 32 {
                [255, 90, 75, 255]
            } else if x >= 48 && y < 32 {
                [75, 220, 175, 255]
            } else if x < 48 {
                [95, 140, 255, 255]
            } else {
                [0, 0, 0, 0]
            };
            let color = if x % 16 < 2 || y % 16 < 2 {
                [235, 240, 255, 255]
            } else {
                color
            };
            rgba.extend_from_slice(&color);
        }
    }
    let image = StaticImage::from_premultiplied_rgba(96, 64, rgba, Default::default()).unwrap();
    let mut inputs = ImageInputs::default();
    inputs.insert("sample".into(), Ok(std::sync::Arc::new(image)));
    inputs
}
fn rect(x: f32, y: f32, w: f32, h: f32) -> SceneBounds {
    SceneBounds::new(ScenePoint::new(x, y), SceneSize::new(w, h))
}
fn image(source: Option<SceneBounds>, destination: SceneBounds) -> CanvasCommand {
    CanvasCommand::DrawImage {
        image: "sample".into(),
        source,
        destination,
    }
}
fn commands(clip_mode: u8) -> Vec<CanvasCommand> {
    let mut result = Vec::new();
    if clip_mode > 0 {
        result.push(CanvasCommand::ClipPath {
            path: path("M 8 80 C 8 -15 255 -15 255 80 C 255 190 8 190 8 80 Z"),
            rule: FillRule::NonZero,
        });
    }
    if clip_mode > 1 {
        result.push(CanvasCommand::ClipPath {
            path: path("M 0 0 L 280 0 L 280 180 L 0 180 Z M 95 70 L 185 70 L 185 140 L 95 140 Z"),
            rule: FillRule::EvenOdd,
        });
    }
    result.extend([
        image(None, rect(12., 12., 120., 80.)),
        CanvasCommand::Save,
        CanvasCommand::Transform(Affine::translation(205., 58.) * Affine::rotation(22.)),
        image(Some(rect(48., 0., 48., 32.)), rect(-45., -30., 90., 60.)),
        CanvasCommand::Restore,
        CanvasCommand::SetOpacity(0.6),
        fill(
            "M 40 110 L 130 110 L 130 165 L 40 165 Z",
            Color::hex(0xffdb70),
        ),
        image(Some(rect(-24., 0., 96., 64.)), rect(50., 90., 130., 85.)),
        CanvasCommand::Save,
        CanvasCommand::Transform(Affine::translation(275., 100.) * Affine::scale(-1., 1.)),
        image(None, rect(0., 0., 90., 60.)),
        CanvasCommand::Restore,
    ]);
    result
}
fn document() -> Scene {
    let mut s = Scene::new();
    for (id, x, dpr, label) in [
        ("one", 35., 1., "Image batch · DPR 1"),
        ("two", 395., 2., "Image batch · DPR 2"),
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
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.show_grid = false;
            v.set_editable(false, cx);
            v
        });
        let mut lab = Self {
            view,
            status: String::new(),
            clip_mode: 1,
        };
        lab.paint(0, cx);
        lab
    }
    fn paint(&mut self, action: u8, cx: &mut Context<Self>) {
        let result = self.view.update(cx, |v, cx| {
            for id in ["one", "two"] {
                let h = v.editor.engine.handle(&id.into()).unwrap();
                v.edit_canvas(
                    h,
                    |s| {
                        match action {
                            0 => {
                                s.clear()?;
                                s.draw_commands_with_images(&commands(self.clip_mode), &inputs())?;
                            }
                            1 => {
                                s.draw_commands(&[
                                    CanvasCommand::SetOpacity(0.5),
                                    fill("M 0 0 L 26 0 L 26 26 L 0 26 Z", Color::WHITE),
                                ])?;
                            }
                            2 => {
                                s.draw_commands(&[
                                    fill("M 0 0 L 280 0 L 280 180 L 0 180 Z", Color::WHITE),
                                    image(None, rect(0., 0., 100., 100.)),
                                ])?;
                            }
                            _ => s.clear()?,
                        };
                        Ok(())
                    },
                    cx,
                )?;
            }
            Ok::<(), gpui_flow::resources::CanvasEditError>(())
        });
        self.status = match result {
            Ok(()) => {
                "Pixel edit accepted. Each command batch starts at identity and opacity 1.".into()
            }
            Err(e) => format!("Rejected atomically; previous pixels retained: {e}"),
        };
        cx.notify();
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
            .child(ControlButton::new("draw","Draw image batch").on_click(cx.listener(|s,_,_,cx|s.paint(0,cx))))
            .child(ControlButton::new("alpha","Overlay at origin").on_click(cx.listener(|s,_,_,cx|s.paint(1,cx))))
            .child(ControlButton::new("reject","Reject late command").on_click(cx.listener(|s,_,_,cx|s.paint(2,cx))))
            .child(ControlButton::new("safe","Safe shrink/grow").on_click(cx.listener(|s,_,_,cx|s.resize(true,cx))))
            .child(ControlButton::new("reset","Reset shrink/grow").on_click(cx.listener(|s,_,_,cx|s.resize(false,cx))))
            .child(ControlButton::new("undo","Undo config").on_click(cx.listener(|s,_,_,cx|s.view.update(cx,|v,cx|{v.editor.undo();cx.notify();}))))
            .child(ControlButton::new("clip","Cycle clip").on_click(cx.listener(|s,_,_,cx|{s.clip_mode=(s.clip_mode+1)%3;s.paint(0,cx);})))
            .child(ControlButton::new("clear","Clear").on_click(cx.listener(|s,_,_,cx|s.paint(3,cx)))))
        .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
        .child(div().h(px(96.)).px_4().text_sm().child("Clip: ellipse / ellipse + hole / none. Full frame, cropped green quadrant, transparent-source margin and mirrored alpha draw.\nSave/restore includes clipping, transform and opacity only inside a batch. DPR maps logical coordinates once.\n").child(self.status.clone()))
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
                        size(px(1100.), px(580.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Canvas image commands");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
