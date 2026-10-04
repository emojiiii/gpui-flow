//! Real CPU domains uploaded through SceneView; no screenshot/readback emulation.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView, SceneViewEvent,
    scene::{
        Affine, BlendMode, Color, DisplayObject, EraserMode, EventKind, MaskMode, Scene, Shape,
        Size, Style, display,
    },
};
fn document() -> Scene {
    let mut s = Scene::new();
    for (id, label, x) in [
        ("opacity", "Whole-group opacity 50%", 50.),
        ("blend", "Multiply on internal yellow", 365.),
        ("mask", "Mask modes: ellipse", 680.),
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
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.show_grid = false;
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
            _events: events,
            last_hit: "Click the masked content to inspect the routed target".into(),
        }
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
                .child(div().text_sm().child(status)))
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
                        size(px(1040.), px(620.)),
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
