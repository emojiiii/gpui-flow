//! Real CPU domains uploaded through SceneView; no screenshot/readback emulation.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView, SceneViewEvent,
    display::{CustomRasterOutput, CustomRasterRequest, CustomRasterizer},
    scene::{
        Affine, BlendMode, Bounds as SceneBounds, Color, Contour, DisplayObject, EraserMode,
        EventKind, MaskMode, Point as ScenePoint, Scene, Shape, Size, Style, display,
    },
};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
fn cpu_custom(size: f32) -> Shape {
    display::Custom::new(
        "cpu-holes",
        SceneBounds::new(ScenePoint::ZERO, Size::new(size, size)),
        vec![Contour {
            closed: true,
            points: vec![
                ScenePoint::ZERO,
                ScenePoint::new(size, 0.),
                ScenePoint::new(size, size),
                ScenePoint::new(0., size),
            ],
        }],
    )
    .into()
}
struct HoleRasterizer(Arc<AtomicBool>);
impl CustomRasterizer for HoleRasterizer {
    fn rasterize(
        &self,
        request: CustomRasterRequest<'_>,
    ) -> Result<Option<CustomRasterOutput>, String> {
        let Shape::Custom(custom) = &request.object.shape else {
            return Err("expected Custom".into());
        };
        let Some(color) = request.object.style.fills().find_map(|p| {
            if let gpui_flow::scene::Paint::Solid { color } = p {
                Some(*color)
            } else {
                None
            }
        }) else {
            return Ok(None);
        };
        let scale = request.world_transform.max_scale()
            * request.viewport.world_to_screen.max_scale()
            * request.viewport.device_scale;
        let width = (custom.bounds.size.width * scale).ceil().max(1.) as u32;
        let height = (custom.bounds.size.height * scale).ceil().max(1.) as u32;
        let pixels = u64::from(width) * u64::from(height);
        if width > request.limits.max_dimension
            || height > request.limits.max_dimension
            || pixels > request.max_pixel_work
            || pixels.saturating_mul(4) > request.max_output_bytes
        {
            return Err("host raster budget".into());
        }
        let mut rgba = Vec::new();
        rgba.try_reserve_exact(pixels as usize * 4)
            .map_err(|_| "allocation")?;
        let center = if self.0.load(Ordering::Acquire) {
            0.65
        } else {
            0.35
        };
        let a = u16::from(color.a);
        let ink = [
            (u16::from(color.r) * a / 255) as u8,
            (u16::from(color.g) * a / 255) as u8,
            (u16::from(color.b) * a / 255) as u8,
            color.a,
        ];
        for y in 0..height {
            if request.cancellation.is_cancelled() {
                return Err("cancelled".into());
            }
            for x in 0..width {
                let dx = x as f32 / width as f32 - center;
                let dy = y as f32 / height as f32 - 0.5;
                rgba.extend_from_slice(if dx * dx + dy * dy < 0.18 * 0.18 {
                    &[0; 4]
                } else {
                    &ink
                });
            }
        }
        Ok(Some(CustomRasterOutput {
            width,
            height,
            premul_rgba: rgba,
            local_bounds: custom.bounds,
            pixel_work: pixels,
        }))
    }
}
fn custom_diamond(size: f32) -> Shape {
    display::Custom::new(
        "path-contour",
        SceneBounds::new(ScenePoint::ZERO, Size::new(size, size)),
        vec![Contour {
            points: vec![
                ScenePoint::new(size * 0.5, 0.),
                ScenePoint::new(size, size * 0.5),
                ScenePoint::new(size * 0.5, size),
                ScenePoint::new(0., size * 0.5),
            ],
            closed: true,
        }],
    )
    .into()
}
fn document() -> Scene {
    let mut s = Scene::new();
    for (id, label, x) in [
        ("opacity", "Whole-group opacity 50%", 50.),
        ("blend", "Multiply on internal yellow", 365.),
        ("mask", "Mask modes: ellipse / Custom", 680.),
    ] {
        let mut group = DisplayObject::new(id, display::Group::new())
            .with_transform(Affine::translation(x, 90.));
        group.style.blend_mode = BlendMode::Normal;
        if id == "opacity" {
            group.style.opacity = 0.5;
        }
        s.insert(group, None, None).unwrap();
        let mut text = display::Text::new(label, Size::new(290., 35.));
        text.font_size = 18.;
        s.insert(
            DisplayObject::new(format!("{id}-label"), text)
                .with_transform(Affine::translation(x, 40.))
                .with_style(Style::fill(Color::WHITE)),
            None,
            None,
        )
        .unwrap();
    }
    let mut add = |object: DisplayObject, parent: &str| {
        s.insert(object, Some(parent.into()), None).unwrap();
    };
    for (id, parent, x, y, color) in [
        ("op-red", "opacity", 0., 0., 0xff4050),
        ("op-blue", "opacity", 65., 55., 0x4060ff),
        ("background", "blend", 0., 0., 0xffdd33),
    ] {
        add(
            DisplayObject::new(id, display::Rect::new(200., 200.))
                .with_transform(Affine::translation(x, y))
                .with_style(Style::fill(Color::hex(color))),
            parent,
        );
    }
    let mut blue = DisplayObject::new("blend-blue", display::Ellipse::new(200., 200.))
        .with_transform(Affine::translation(55., 55.))
        .with_style(Style::fill(Color::hex(0x4060ff)));
    blue.style.blend_mode = BlendMode::Multiply;
    add(blue, "blend");
    add(
        DisplayObject::new("ellipse-mask", display::Ellipse::new(240., 240.))
            .with_mask(MaskMode::Alpha)
            .with_style(Style::fill(Color::hex(0xff3030))),
        "mask",
    );
    add(
        DisplayObject::new("masked-red", display::Rect::new(205., 255.))
            .with_transform(Affine::translation(50., 0.))
            .with_style(Style::fill(Color::hex(0xff4050))),
        "mask",
    );
    add(
        DisplayObject::new("masked-blue", display::Rect::new(145., 255.))
            .with_transform(Affine::translation(110., 0.))
            .with_style(Style::fill(Color::hex(0x4060ff))),
        "mask",
    );
    let mut pixel_cut = DisplayObject::new("cpu-pixel-cut", cpu_custom(180.))
        .with_transform(Affine::translation(55., 35.))
        .with_style(Style::fill(Color::WHITE))
        .with_eraser(EraserMode::Pixel);
    pixel_cut.visible = false;
    add(pixel_cut, "mask");
    let mut path_cut = DisplayObject::new("path-cut", display::Group::new())
        .with_transform(Affine::translation(70., 60.))
        .with_eraser(EraserMode::Path);
    path_cut.visible = false;
    add(path_cut, "mask");
    for (id, x) in [("clip-a", 0.), ("clip-b", 40.)] {
        add(
            DisplayObject::new(id, display::Ellipse::new(90., 90.))
                .with_transform(Affine::translation(x, 0.)),
            "path-cut",
        );
    }
    for (id, shape, x, y) in [
        (
            "image-cut",
            Shape::from(display::Image::new("geometry-only", Size::new(30., 30.))),
            65.,
            165.,
        ),
        (
            "canvas-cut",
            display::Canvas::new(30., 30.).into(),
            170.,
            35.,
        ),
        (
            "box-cut",
            display::Box::new(30., 30.).rounded(7.).into(),
            170.,
            170.,
        ),
        (
            "frame-cut",
            display::Frame::new(30., 30.).rounded(7.).into(),
            75.,
            25.,
        ),
        ("custom-cut", custom_diamond(30.), 170., 100.),
    ] {
        let mut cut = DisplayObject::new(id, shape)
            .with_transform(Affine::translation(x, y))
            .with_eraser(EraserMode::Path);
        cut.visible = false;
        add(cut, "mask");
    }
    s
}
struct Lab {
    view: Entity<SceneView>,
    _events: Subscription,
    last_hit: String,
    cpu_enabled: bool,
    cpu_shift: Arc<AtomicBool>,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let cpu_shift = Arc::new(AtomicBool::new(false));
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.show_grid = false;
            v.set_custom_rasterizer(
                "cpu-holes",
                Some(Arc::new(HoleRasterizer(cpu_shift.clone()))),
                cx,
            );
            v.set_editable(false, cx);
            v
        });
        let events = cx.subscribe(&view, |this, _, event, cx| {
            if let SceneViewEvent::Pointer(event) = event
                && event.kind == EventKind::PointerDown
            {
                this.last_hit = format!("Last pointer target: {}", event.target);
                eprintln!(
                    "pointer target={} world={:?} local={:?}",
                    event.target, event.world_position, event.local_position
                );
                cx.notify();
            }
        });
        Self {
            view,
            cpu_enabled: true,
            cpu_shift,
            _events: events,
            last_hit: "Click the masked content to inspect the routed target".into(),
        }
    }
    fn cpu_mode(&mut self, pixel: bool, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            view.editor
                .transaction(|scene| {
                    scene.update_object(&"ellipse-mask".into(), |o| {
                        o.shape = cpu_custom(240.);
                        o.style.fill = Some(Color::WHITE.into());
                        o.style.opacity = 1.;
                        o.visible = !pixel;
                        o.mask = if pixel {
                            MaskMode::None
                        } else {
                            MaskMode::Alpha
                        };
                    })?;
                    scene.update_object(&"cpu-pixel-cut".into(), |o| o.visible = pixel)?;
                    Ok(())
                })
                .unwrap();
            eprintln!(
                "Custom CPU mode={}",
                if pixel { "Pixel eraser" } else { "Alpha mask" }
            );
            cx.notify();
        });
        cx.notify();
    }
    fn toggle_cpu(&mut self, cx: &mut Context<Self>) {
        self.cpu_enabled = !self.cpu_enabled;
        let producer = self
            .cpu_enabled
            .then(|| Arc::new(HoleRasterizer(self.cpu_shift.clone())) as Arc<dyn CustomRasterizer>);
        self.view.update(cx, |v, cx| {
            v.set_custom_rasterizer("cpu-holes", producer, cx)
        });
        eprintln!("Custom CPU producer enabled={}", self.cpu_enabled);
        cx.notify();
    }
    fn invalidate_cpu(&mut self, cx: &mut Context<Self>) {
        self.cpu_shift.fetch_xor(true, Ordering::AcqRel);
        self.view.update(cx, |v, cx| {
            v.invalidate_custom_rasterizer("cpu-holes", cx);
        });
        eprintln!("Custom CPU aperture shifted; producer invalidated");
        cx.notify();
    }
    fn toggle_erasers(&mut self, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            let visible = !view
                .editor
                .scene()
                .get(&"path-cut".into())
                .unwrap()
                .object
                .visible;
            view.editor
                .transaction(|scene| {
                    for id in [
                        "path-cut",
                        "image-cut",
                        "canvas-cut",
                        "box-cut",
                        "frame-cut",
                        "custom-cut",
                    ] {
                        scene.update_object(&id.into(), |object| object.visible = visible)?;
                    }
                    Ok(())
                })
                .unwrap();
            eprintln!("geometric path erasers visible={visible}");
            cx.notify();
        });
        cx.notify();
    }
    fn toggle_custom_mask(&mut self, cx: &mut Context<Self>) {
        self.edit(
            "ellipse-mask",
            |object| {
                let custom = matches!(object.shape, Shape::Custom(_));
                object.shape = if custom {
                    display::Ellipse::new(240., 240.).into()
                } else {
                    custom_diamond(240.)
                };
                object.mask = if custom {
                    MaskMode::Alpha
                } else {
                    MaskMode::Path
                };
            },
            cx,
        );
    }
    fn edit(&mut self, id: &str, f: impl FnOnce(&mut DisplayObject), cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            v.editor
                .transaction(|s| s.update_object(&id.into(), f))
                .unwrap();
            let mask = &v.editor.scene().get(&"ellipse-mask".into()).unwrap().object;
            eprintln!(
                "mask {:?} opacity={} visible={} painted={}",
                mask.mask,
                mask.style.opacity,
                mask.visible,
                mask.style.fill.is_some()
            );
            cx.notify();
        });
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mask = &self
            .view
            .read(cx)
            .editor
            .scene()
            .get(&"ellipse-mask".into())
            .unwrap()
            .object;
        let status = format!(
            "{:?} · opacity {} · visible {} · painted {}",
            mask.mask,
            mask.style.opacity,
            mask.visible,
            mask.style.fill.is_some()
        );
        let group = &self
            .view
            .read(cx)
            .editor
            .scene()
            .get(&"mask".into())
            .unwrap()
            .object;
        let width = self
            .view
            .read(cx)
            .editor
            .engine
            .local_bounds(&"mask".into())
            .unwrap()
            .map_or(0., |b| b.size.width);
        let hit_status = format!(
            "hitChildren={} · group width={} · {}",
            group.hit_children, width, self.last_hit
        );
        div().size_full().flex().flex_col().bg(rgb(0x161b25)).text_color(rgb(0xffffff))
            .child(div().h(px(58.)).px_4().flex().items_center().gap_2().child("Vector compositing")
                .child(ControlButton::new("opacity","Opacity 50% / 100%").on_click(cx.listener(|this,_,_,cx|this.edit("opacity",|o|o.style.opacity=if o.style.opacity==0.5 {1.} else {0.5},cx))))
                .child(ControlButton::new("blend","Multiply / Screen").on_click(cx.listener(|this,_,_,cx|this.edit("blend-blue",|o|o.style.blend_mode=if o.style.blend_mode==BlendMode::Multiply {BlendMode::Screen} else {BlendMode::Multiply},cx))))
                .child(ControlButton::new("mask","Mask alpha 100% / 50%").on_click(cx.listener(|this,_,_,cx|this.edit("ellipse-mask",|o|o.style.opacity=if o.style.opacity==1. {0.5} else {1.},cx))))
                .child(ControlButton::new("path-erasers","Path erasers").on_click(cx.listener(|this,_,_,cx|this.toggle_erasers(cx))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|{this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();});cx.notify();}))))
            .child(div().h(px(48.)).px_4().flex().items_center().gap_2()
                .child(ControlButton::new("mask-mode", "Next mask mode").on_click(cx.listener(|this,_,_,cx|this.edit("ellipse-mask",|o|o.mask=match o.mask {MaskMode::Alpha=>MaskMode::Grayscale,MaskMode::Grayscale=>MaskMode::Path,MaskMode::Path=>MaskMode::Clipping,MaskMode::Clipping=>MaskMode::ClippingPath,_=>MaskMode::Alpha},cx))))
                .child(ControlButton::new("mask-fill", "Paint / no paint").on_click(cx.listener(|this,_,_,cx|this.edit("ellipse-mask",|o|o.style.fill=if o.style.fill.is_some(){None}else{Some(Color::hex(0xff3030).into())},cx))))
                .child(ControlButton::new("mask-visible", "Show / hide marker").on_click(cx.listener(|this,_,_,cx|this.edit("ellipse-mask",|o|o.visible = !o.visible,cx))))
                .child(ControlButton::new("hit-children", "Hit children").on_click(cx.listener(|this,_,_,cx|this.edit("mask",|o|o.hit_children = !o.hit_children,cx))))
                .child(ControlButton::new("custom-mask", "Custom Path mask").on_click(cx.listener(|this,_,_,cx|this.toggle_custom_mask(cx))))
                .child(div().text_sm().child(status)))
            .child(div().h(px(44.)).px_4().flex().items_center().gap_2()
                .child(ControlButton::new("cpu-alpha", "CPU Alpha holes").on_click(cx.listener(|this,_,_,cx|this.cpu_mode(false,cx))))
                .child(ControlButton::new("cpu-pixel", "CPU Pixel holes").on_click(cx.listener(|this,_,_,cx|this.cpu_mode(true,cx))))
                .child(ControlButton::new("cpu-enabled", "CPU on / off").on_click(cx.listener(|this,_,_,cx|this.toggle_cpu(cx))))
                .child(ControlButton::new("cpu-invalidate", "Shift + invalidate").on_click(cx.listener(|this,_,_,cx|this.invalidate_cpu(cx))))
                .child(div().text_sm().child(format!("CPU bridge={} · {:?}",self.cpu_enabled,self.view.read(cx).composite_diagnostics))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(64.)).px_4().flex().flex_col().text_sm().child(hit_status).child("Alpha/Grayscale use paint; Path uses geometry. Hide the sole marker to restore ordinary content bounds. Undo restores edits."))
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
                        size(px(1160.), px(690.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · CPU compositing");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
