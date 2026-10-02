//! Physical RGBA exchange and raw-surface PNG export, separate from scene style.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    composite::{CanvasPixels, CompositeError, CompositeLimits},
    scene::{Affine, Color, DisplayObject, Scene, Shape, Size as SceneSize, Style, display},
};
fn document() -> Scene {
    let mut s = Scene::new();
    let mut canvas = display::Canvas::new(240., 160.).with_corners([35., 0., 30., 0.]);
    canvas.pixel_ratio = 2.;
    s.insert(
        DisplayObject::new("canvas", canvas)
            .with_transform(Affine::translation(45., 70.))
            .with_style(Style {
                fill: None,
                ..Default::default()
            }),
        None,
        None,
    )
    .unwrap();
    for (id, x, label) in [
        ("left", 45., "Owned Canvas · DPR 2"),
        ("right", 375., "PNG decoded by GPUI · raw pixels"),
    ] {
        let mut t = display::Text::new(label, SceneSize::new(330., 35.));
        t.font_size = 17.;
        s.insert(
            DisplayObject::new(id, t)
                .with_transform(Affine::translation(x, 20.))
                .with_style(Style::fill(Color::WHITE)),
            None,
            None,
        )
        .unwrap();
    }
    s
}
fn pattern() -> CanvasPixels {
    let mut bytes = Vec::with_capacity(480 * 320 * 4);
    for y in 0..320 {
        for x in 0..480 {
            let rgba = if x > 360 && y < 100 {
                [0, 0, 0, 0]
            } else if (x / 80 + y / 80) % 2 == 0 {
                [255, 130, 40, 160]
            } else {
                [20, 220, 230, 255]
            };
            bytes.extend(rgba);
        }
    }
    CanvasPixels::from_rgba(480, 320, bytes, CompositeLimits::default()).unwrap()
}
struct Lab {
    view: Entity<SceneView>,
    status: String,
    exports: u32,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.show_grid = true;
            v.set_editable(false, cx);
            v
        });
        let mut lab = Self {
            view,
            status: String::new(),
            exports: 0,
        };
        lab.edit(0, cx);
        lab
    }
    fn edit(&mut self, action: u8, cx: &mut Context<Self>) {
        let result = self.view.update(cx, |v, cx| {
            let h = v.editor.engine.handle(&"canvas".into()).unwrap();
            v.edit_canvas(
                h,
                |s| match action {
                    0 => s.write_pixels(&pattern(), 0, 0).map(|_| ()),
                    1 => {
                        let mut region = s.read_pixels(80, 80, 80, 80)?;
                        for p in region.rgba_mut().as_chunks_mut::<4>().0 {
                            p[0] = 255 - p[0];
                            p[1] = 255 - p[1];
                            p[2] = 255 - p[2];
                        }
                        s.write_pixels(&region, 200, 160).map(|_| ())
                    }
                    2 => s.clear(),
                    _ => {
                        s.write_pixels(&pattern(), 0, 0)?;
                        Err(CompositeError::InvalidImage)
                    }
                },
                cx,
            )
        });
        self.status = match result {
            Ok(()) => {
                "Pixel transaction published. Existing PNG preview is a frozen snapshot.".into()
            }
            Err(e) => format!("Rejected atomically: {e}"),
        };
        cx.notify();
    }
    fn export(&mut self, cx: &mut Context<Self>) {
        let view = self.view.read(cx);
        let h = view.editor.engine.handle(&"canvas".into()).unwrap();
        let Some(snapshot) = view
            .resources()
            .and_then(|r| r.canvas_snapshot(&view.editor.engine, h))
        else {
            self.status = "Canvas not ready".into();
            cx.notify();
            return;
        };
        let result = (|| -> Result<(String, usize), String> {
            let png = snapshot
                .encode_png(CompositeLimits::default())
                .map_err(|e| e.to_string())?;
            let directory = std::env::var_os("FLOW_CANVAS_EXPORT_DIR")
                .map(std::path::PathBuf::from)
                .unwrap_or_else(|| std::env::temp_dir().join("gpui-flow-canvas-pixels"));
            std::fs::create_dir_all(&directory).map_err(|e| e.to_string())?;
            let path = directory.join(format!(
                "surface-{}-{}.png",
                std::process::id(),
                self.exports
            ));
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
                .map_err(|e| e.to_string())?;
            std::io::Write::write_all(&mut file, &png).map_err(|e| e.to_string())?;
            Ok((path.to_string_lossy().into_owned(), png.len()))
        })();
        match result {
            Ok((path, bytes)) => {
                self.exports += 1;
                let message = format!(
                    "Exported {}×{} physical pixels, {bytes} bytes: {path}",
                    snapshot.width(),
                    snapshot.height()
                );
                self.view.update(cx, |v, cx| {
                    v.editor
                        .transaction(|s| {
                            let mut image =
                                display::Image::new(path.clone(), SceneSize::new(240., 160.));
                            image.fit = display::ImageFit::Fill;
                            if s.get(&"preview".into()).is_some() {
                                s.update_object(&"preview".into(), |o| {
                                    o.shape = Shape::Image(image)
                                })?;
                            } else {
                                s.insert(
                                    DisplayObject::new("preview", image)
                                        .with_transform(Affine::translation(375., 70.))
                                        .with_style(Style {
                                            fill: None,
                                            ..Default::default()
                                        }),
                                    None,
                                    None,
                                )?;
                            }
                            Ok(())
                        })
                        .unwrap();
                    cx.notify();
                });
                self.status = message;
            }
            Err(error) => self.status = format!("Export failed: {error}"),
        }
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().flex().flex_col().bg(rgb(0x0d1421)).text_color(rgb(0xe6edf5))
        .child(div().h(px(56.)).px_4().flex().items_center().gap_3()
            .child(ControlButton::new("pattern","Write RGBA pattern").on_click(cx.listener(|this,_,_,cx|this.edit(0,cx))))
            .child(ControlButton::new("copy","Read / invert / write").on_click(cx.listener(|this,_,_,cx|this.edit(1,cx))))
            .child(ControlButton::new("export","Export PNG preview").on_click(cx.listener(|this,_,_,cx|this.export(cx))))
            .child(ControlButton::new("clear","Clear surface").on_click(cx.listener(|this,_,_,cx|this.edit(2,cx))))
            .child(ControlButton::new("reject","Reject batch").on_click(cx.listener(|this,_,_,cx|this.edit(3,cx)))))
        .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
        .child(div().h(px(100.)).px_4().text_sm().child("Pixel reads/writes use physical coordinates; logical drawing/DPR and scene corners do not change their metric.\nPNG contains only raw surface alpha, so its corners differ from the Canvas component mask. Preview is immutable until exported again.\n").child(self.status.clone()))
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
                        size(px(900.), px(520.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Canvas RGBA and PNG");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
