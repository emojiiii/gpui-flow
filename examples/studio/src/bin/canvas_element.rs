//! A first-class imperative Canvas inside SceneView, not whole-view OwnedCanvas.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView,
    composite::{CompositeLimits, ImageInputs, TextInputs, capture_subtree},
    scene::{
        self, Affine, Color, DisplayObject, Point as ScenePoint, Scene, Shape, Size as SceneSize,
        Stroke, Style, display,
    },
};
fn document() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("source", display::Star::new(5, 70., 30.))
                .with_transform(Affine::translation(50., 70.))
                .with_style(Style {
                    fill: Some(Color::hex(0xffb84d).into()),
                    stroke: Some(Stroke::new(Color::hex(0xffe6b0), 3.)),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
    let mut c = display::Canvas::new(360., 240.).with_corners([40., 0., 35., 0.]);
    c.safe_resize = true;
    scene
        .insert(
            DisplayObject::new("canvas", c)
                .with_transform(Affine::translation(290., 65.))
                .with_style(Style {
                    fill: Some(Color::hex(0x162438).into()),
                    stroke: Some(Stroke::new(Color::hex(0x4dc7ff), 4.)),
                    ..Default::default()
                }),
            None,
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
        let view = cx.new(|cx| {
            let mut v = SceneView::new(document(), cx).unwrap();
            v.show_grid = false;
            v.set_editable(false, cx);
            v
        });
        let mut lab = Self { view };
        lab.draw(cx);
        lab
    }
    fn draw(&mut self, cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            let source = capture_subtree(
                v.editor.scene(),
                &"source".into(),
                CompositeLimits::default(),
            )
            .unwrap();
            let handle = v.editor.engine.handle(&"canvas".into()).unwrap();
            v.edit_canvas(
                handle,
                |surface| {
                    surface.clear()?;
                    surface.fill_rect(
                        scene::Bounds::new(ScenePoint::new(20., 20.), SceneSize::new(300., 180.)),
                        Color::hex(0x28596a),
                    )?;
                    surface.draw(
                        &source,
                        Affine::translation(45., 30.),
                        &ImageInputs::default(),
                        &TextInputs::default(),
                    )?;
                    surface.draw(
                        &source,
                        Affine::translation(220., 100.) * Affine::scale(0.65, 0.65),
                        &ImageInputs::default(),
                        &TextInputs::default(),
                    )
                },
                cx,
            )
            .unwrap();
        });
    }
    fn resize(&mut self, safe: bool, cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            v.editor
                .transaction(|scene| {
                    scene.update_object(&"canvas".into(), |o| {
                        if let Shape::Canvas(c) = &mut o.shape {
                            c.safe_resize = safe;
                            c.size.width = if c.size.width > 200. { 180. } else { 360. };
                        }
                    })
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
                .child(ControlButton::new("draw","Draw subtree").on_click(cx.listener(|this,_,_,cx|this.draw(cx))))
                .child(ControlButton::new("clear","Clear pixels").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|{let h=v.editor.engine.handle(&"canvas".into()).unwrap();v.edit_canvas(h,|s|s.clear(),cx).unwrap();}))))
                .child(ControlButton::new("safe","Safe shrink/grow").on_click(cx.listener(|this,_,_,cx|this.resize(true,cx))))
                .child(ControlButton::new("reset","Reset shrink/grow").on_click(cx.listener(|this,_,_,cx|this.resize(false,cx))))
                .child(ControlButton::new("undo","Undo config").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();})))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(88.)).px_4().text_sm().child("Left: retained source star. Right: one Canvas with its own pixels, independent corners and border.\nDraw copies the source at two local poses; Clear removes only pixels. Safe resize crops without scaling; Reset resize clears.\nUndo restores configuration only. Imperative pixel content is deliberately outside document history."))
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
                        size(px(820.), px(540.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Canvas element");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
