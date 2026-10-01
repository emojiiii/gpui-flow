//! Bounded container inner shadows with explicit paint-order and alpha semantics.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneView,
    scene::{
        Affine, BlendMode, Color, ContainerPaintOrder, DisplayObject, MaskMode, Scene, Shadow,
        Shape, Size, Stroke, StrokeAlignment, Style, display,
    },
};
fn effect() -> Option<Shadow> {
    Some(Shadow::new(20., 24., 0., Color::rgba(0, 0, 0, 180)))
}
fn layers() -> Vec<Shadow> {
    vec![
        Shadow::new(10., 7., 2., Color::rgba(255, 65, 120, 230)),
        Shadow::new(-7., -4., 0., Color::rgba(255, 215, 30, 170)),
    ]
}
fn document() -> Scene {
    let mut s = Scene::new();
    for (id, x, shape) in [
        ("group", 65., Shape::Group(display::Group::new())),
        (
            "box",
            340.,
            Shape::Box(
                display::Box::new(130., 100.)
                    .rounded(20.)
                    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
            ),
        ),
        (
            "frame",
            640.,
            Shape::Frame(
                display::Frame::new(130., 100.)
                    .rounded(20.)
                    .with_paint_order(ContainerPaintOrder::StrokeAfterChildren),
            ),
        ),
    ] {
        s.insert(
            DisplayObject::new(id, shape)
                .with_transform(Affine::translation(x, 100.))
                .with_style(Style {
                    fill: None,
                    stroke: (id != "group").then(|| {
                        Stroke::new(Color::hex(0x40c8ff), 8.)
                            .with_alignment(StrokeAlignment::Outside)
                    }),
                    shadow: effect(),
                    inner_shadow_layers: layers(),
                    ..Default::default()
                }),
            None,
            None,
        )
        .unwrap();
        for (suffix, x, y) in [("a", 15., 15.), ("b", 85., 65.)] {
            s.insert(
                DisplayObject::new(format!("{id}-{suffix}"), display::Ellipse::circle(30.))
                    .with_transform(Affine::translation(x, y))
                    .with_style(Style {
                        fill: Some(Color::rgba(90, 230, 175, 220).into()),
                        shadow: (id == "group")
                            .then(|| Shadow::new(-14., 10., 0., Color::rgba(45, 150, 240, 160))),
                        ..Default::default()
                    }),
                Some(id.into()),
                None,
            )
            .unwrap();
        }
    }
    for (id, x, label) in [
        ("g-label", 45., "Group: inner tint on child alpha + gaps"),
        ("b-label", 340., "Box: visible overflow + stroke"),
        ("f-label", 625., "Frame: rounded child clip"),
    ] {
        let mut t = display::Text::new(label, Size::new(280., 32.));
        t.font_size = 15.;
        s.insert(
            DisplayObject::new(id, t)
                .with_transform(Affine::translation(x, 35.))
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
}
impl Lab {
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
        div().size_full().flex().flex_col().bg(rgb(0x171c27)).text_color(rgb(0xffffff))
            .child(div().h(px(58.)).px_4().flex().items_center().gap_2().child("Container inner")
                .child(ControlButton::new("shadow","Outer on / off").on_click(cx.listener(|this,_,_,cx|this.edit("group",|o|o.style.shadow=if o.style.shadow.is_some(){None}else{effect()},cx))))
                .child(ControlButton::new("blur","Inner blur").on_click(cx.listener(|this,_,_,cx|this.edit("box",|o|{let s=&mut o.style.inner_shadow_layers[0];s.blur=if s.blur==0.{2.}else{0.};},cx))))
                .child(ControlButton::new("opacity","Group alpha").on_click(cx.listener(|this,_,_,cx|this.edit("group",|o|o.style.opacity=if o.style.opacity==1.{0.5}else{1.},cx))))
                .child(ControlButton::new("clip","Frame clip").on_click(cx.listener(|this,_,_,cx|this.edit("frame",|o|if let Shape::Frame(f)=&mut o.shape {f.clip_children = !f.clip_children;},cx))))
                .child(ControlButton::new("phase","Box order").on_click(cx.listener(|this,_,_,cx|this.edit("box",|o|if let Shape::Box(b)=&mut o.shape {b.paint_order=if b.paint_order==ContainerPaintOrder::BeforeChildren {ContainerPaintOrder::StrokeAfterChildren}else{ContainerPaintOrder::BeforeChildren};},cx))))
                .child(ControlButton::new("unsafe","Child blend").on_click(cx.listener(|this,_,_,cx|this.edit("group-a",|o|o.style.blend_mode=if o.style.blend_mode==BlendMode::Multiply {BlendMode::PassThrough}else{BlendMode::Multiply},cx))))
                .child(ControlButton::new("isolate","Group isolation").on_click(cx.listener(|this,_,_,cx|this.edit("group",|o|o.style.blend_mode=if o.style.blend_mode==BlendMode::Normal {BlendMode::PassThrough}else{BlendMode::Normal},cx))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|v,cx|{v.editor.undo();cx.notify();})))))
            .child(div().h(px(46.)).px_4().flex().items_center().gap_2()
                .child(ControlButton::new("inner", "Group inner on / off").on_click(cx.listener(|this,_,_,cx|this.edit("group",|o|if o.style.inner_shadow_layers.is_empty(){o.style.inner_shadow_layers=layers();}else{o.style.inner_shadow_layers.clear();},cx))))
                .child(ControlButton::new("alpha", "Child alpha").on_click(cx.listener(|this,_,_,cx|this.edit("group-a",|o|o.style.opacity=if o.style.opacity==1.{0.5}else{1.},cx))))
                .child(ControlButton::new("reverse", "Reverse inner layers").on_click(cx.listener(|this,_,_,cx|this.edit("frame",|o|o.style.inner_shadow_layers.reverse(),cx))))
                .child(ControlButton::new("mask", "Group sibling mask").on_click(cx.listener(|this,_,_,cx|this.edit("group-a",|o|o.mask=if o.mask==MaskMode::None{MaskMode::Alpha}else{MaskMode::None},cx)))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(85.)).px_4().text_sm().child("Inner layers use complete subtree alpha, including child effects; empty gaps stay empty. Own black outer shadows sample independently.\nBox order moves inner + stroke before/after children. BeforeChildren can hide tint beneath opaque children. Frame clip changes the source.\nUnsafe child Multiply rejects at full Group opacity; explicit Group isolation restores it. Mask toggles to an empty nonoverlapping run. Pan/zoom and Undo remain geometric."))
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
                    size(px(1250.), px(590.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Container inner shadows");
                cx.new(|cx| Lab {
                    view: cx.new(|cx| {
                        let mut v = SceneView::new(document(), cx).unwrap();
                        v.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
                        v
                    }),
                })
            },
        )
        .unwrap();
        cx.activate(true);
    });
}

#[cfg(test)]
mod tests {
    use super::document;
    use gpui_flow::composite::{
        CompositeLimits, PixelBudget, RasterViewport, SceneUnit, rasterize, scene_units,
    };
    use gpui_flow::scene::{Affine, Bounds as SceneBounds, Point as ScenePoint, Size};
    use std::sync::atomic::AtomicBool;

    #[test]
    fn default_lab_three_domains_fit_one_shared_budget() {
        let scene = document();
        let bounds = SceneBounds::new(ScenePoint::ZERO, Size::new(1250., 360.));
        let viewport = RasterViewport {
            world_to_screen: Affine::IDENTITY,
            canvas_bounds: bounds,
            content_mask: bounds,
            device_scale: 1.,
        };
        let mut budget = PixelBudget::new(CompositeLimits::default(), 0).unwrap();
        let mut frames = Vec::new();
        for unit in scene_units(&scene, CompositeLimits::default()).unwrap() {
            match unit {
                SceneUnit::Domain(source) => {
                    frames.push(
                        rasterize(&source, viewport, &mut budget, &AtomicBool::new(false))
                            .unwrap()
                            .unwrap(),
                    );
                }
                SceneUnit::Rejected(issue) => panic!("{issue:?}"),
                _ => {}
            }
        }
        assert_eq!(frames.len(), 3);
    }
}
