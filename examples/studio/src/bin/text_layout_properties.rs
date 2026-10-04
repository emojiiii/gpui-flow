//! Compact native gate for spacing, indentation, auto-size anchors and history.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextMeasurer, GpuiTextRasterizer, SceneView,
    layout::{AlignItems as FlowAlign, FlowLayout, FlowStyle, Insets, TextSizing},
    scene::{Affine, Color, DisplayObject, Scene, Shape, Size as SceneSize, Style, display},
};
use std::sync::Arc;

fn insert(scene: &mut Scene, id: &str, shape: impl Into<Shape>, x: f32, y: f32, color: u32) {
    let object = DisplayObject::new(id, shape)
        .with_transform(Affine::translation(x, y))
        .with_style(Style::fill(Color::hex(color)));
    scene.insert(object, None, None).unwrap();
}
fn label(scene: &mut Scene, id: &str, content: &str, x: f32, y: f32) {
    let mut text = display::Text::new(content, SceneSize::new(350., 30.));
    text.font_size = 17.;
    text.wrap = false;
    insert(scene, id, text, x, y, 0xd7e1ef);
}
fn document() -> (Scene, FlowLayout) {
    let mut scene = Scene::new();
    for (id, title, x, y) in [
        ("paragraph-title", "Spacing + first-row indent", 35., 24.),
        ("anchor-title", "Auto-size anchor", 420., 24.),
        ("origin-label", "Fixed local origin (+)", 470., 380.),
    ] {
        label(&mut scene, id, title, x, y);
    }
    let background = display::Rect::new(290., 225.);
    insert(&mut scene, "box", background, 35., 70., 0x17273b);
    let mut paragraph = display::Text::new(
        "First paragraph wraps into another visual row.\nSecond paragraph is indented too.",
        SceneSize::new(290., 225.),
    );
    paragraph.font_family = "DejaVu Sans".into();
    paragraph.font_size = 19.;
    paragraph.line_height = 1.4;
    paragraph.padding = [12.; 4];
    paragraph.para_spacing = 10.;
    insert(&mut scene, "paragraph", paragraph, 35., 70., 0xf4d38e);
    for (id, x, y, width, height) in [
        ("origin-x", 540., 209., 100., 2.),
        ("origin-y", 589., 160., 2., 100.),
    ] {
        let shape = display::Rect::new(width, height);
        insert(&mut scene, id, shape, x, y, 0x537fb3);
    }
    let mut anchor = display::Text::new("Center\nlabel", SceneSize::new(240., 80.));
    anchor.font_family = "DejaVu Sans".into();
    anchor.font_size = 22.;
    anchor.line_height = 1.4;
    anchor.padding = [7., 5., 11., 17.];
    anchor.wrap = false;
    anchor.align = display::TextAlign::Center;
    anchor.vertical_align = display::TextVerticalAlign::Middle;
    insert(&mut scene, "anchor", anchor, 590., 210., 0x91e3cf);
    let mut layout = FlowLayout::default();
    layout.texts.insert("anchor".into(), TextSizing::Natural);
    label(
        &mut scene,
        "baseline-title",
        "Native baseline: X / axis / Y-reverse",
        35.,
        425.,
    );
    insert(
        &mut scene,
        "baseline-row",
        display::Frame::new(760., 140.),
        35.,
        460.,
        0x17273b,
    );
    layout.containers.insert(
        "baseline-row".into(),
        FlowStyle {
            align_items: FlowAlign::Baseline,
            gap: 80.,
            padding: Insets::all(16.),
            ..Default::default()
        },
    );
    for (id, mode, color) in [
        ("baseline-x", display::TextWritingMode::X, 0xf4d38e),
        ("baseline-axis", display::TextWritingMode::Y, 0x91e3cf),
        (
            "baseline-reverse",
            display::TextWritingMode::YReverse,
            0xb7b5ff,
        ),
    ] {
        let mut text = display::Text::new("Ag", SceneSize::new(120., 80.));
        text.font_family = "DejaVu Sans".into();
        text.font_size = 26.;
        text.line_height = 1.4;
        text.padding = [6., 8., 6., 8.];
        text.wrap = false;
        text.writing_mode = mode;
        scene
            .insert(
                DisplayObject::new(id, text).with_style(Style::fill(Color::hex(color))),
                Some("baseline-row".into()),
                None,
            )
            .unwrap();
        layout.texts.insert(id.into(), TextSizing::Natural);
    }
    (scene, layout)
}
fn text<'a>(scene: &'a Scene, id: &str) -> &'a display::Text {
    let Shape::Text(text) = &scene.get(&id.into()).unwrap().object.shape else {
        unreachable!()
    };
    text
}
#[derive(Clone, Copy)]
enum Action {
    Toggle,
    Mode,
    Formatting,
    Undo,
    Redo,
    Reject,
}
struct Lab {
    view: Entity<SceneView>,
    status: &'static str,
}
impl Lab {
    fn new(bridge: Arc<GpuiTextRasterizer>, cx: &mut Context<Self>) -> Self {
        let (mut scene, layout) = document();
        let measured =
            layout.with_text_measurer(Arc::new(GpuiTextMeasurer::new(cx.text_system().clone())));
        measured.apply_to_scene(&mut scene).unwrap();
        let view = cx.new(|cx| {
            let mut view = SceneView::new(scene, cx).unwrap();
            view.show_grid = false;
            view.set_editable(false, cx);
            view.set_text_rasterizer(Some(bridge), cx).unwrap();
            measured.install_reactive(&mut view.editor.engine).unwrap();
            view.editor.selection.select("anchor".into(), false);
            view
        });
        Self {
            view,
            status: "Ready: apply layout, Undo, Redo, then reject -1",
        }
    }
    fn action(&mut self, action: Action, cx: &mut Context<Self>) {
        self.status = self.view.update(cx, |view, cx| {
            let status = match action {
                Action::Toggle => {
                    let enabled = !text(view.editor.scene(), "anchor").auto_size_align;
                    view.editor
                        .transaction(|scene| {
                            for id in ["paragraph", "anchor"] {
                                scene.update_object(&id.into(), |object| {
                                    let Shape::Text(text) = &mut object.shape else {
                                        unreachable!()
                                    };
                                    text.letter_spacing = if enabled { 2. } else { 0. };
                                    text.word_spacing = if enabled { 4. } else { 0. };
                                    if id == "paragraph" {
                                        text.para_indent = if enabled { 28. } else { 0. };
                                    }
                                    if id == "anchor" {
                                        text.auto_size_align = enabled;
                                    }
                                })?;
                            }
                            Ok(())
                        })
                        .unwrap();
                    if enabled {
                        "Applied: spacing, paragraph indent and auto-size alignment"
                    } else {
                        "Reset to the original layout"
                    }
                }
                Action::Mode => {
                    use display::TextWritingMode::*;
                    view.editor
                        .transaction(|scene| {
                            for id in ["paragraph", "anchor", "baseline-axis"] {
                                scene.update_object(&id.into(), |object| {
                                    let Shape::Text(text) = &mut object.shape else {
                                        unreachable!()
                                    };
                                    text.writing_mode = match text.writing_mode {
                                        X => Y,
                                        Y => YReverse,
                                        YReverse => XReverse,
                                        XReverse => X,
                                    };
                                })?;
                            }
                            Ok(())
                        })
                        .unwrap();
                    "Changed axis: upright native clusters, Natural bounds reflowed"
                }
                Action::Formatting => {
                    let enabled = text(view.editor.scene(), "paragraph").overflow
                        != display::TextOverflow::Ellipsis;
                    view.editor
                        .transaction(|scene| {
                            for id in ["paragraph", "anchor"] {
                                scene.update_object(&id.into(), |object| {
                                    let Shape::Text(text) = &mut object.shape else {
                                        unreachable!()
                                    };
                                    text.overflow = if enabled {
                                        display::TextOverflow::Ellipsis
                                    } else {
                                        display::TextOverflow::Clip
                                    };
                                    text.decoration = if enabled {
                                        display::TextDecoration::UnderlineStrikethrough
                                    } else {
                                        display::TextDecoration::None
                                    };
                                    if id == "paragraph" {
                                        text.wrap = !enabled;
                                    }
                                })?;
                            }
                            Ok(())
                        })
                        .unwrap();
                    if enabled {
                        "Ellipsis + underline/strike; paragraph unwrapped to expose overflow"
                    } else {
                        "Clip + no decoration; paragraph wrapping restored"
                    }
                }
                Action::Undo => {
                    view.editor.undo();
                    "Undo"
                }
                Action::Redo => {
                    view.editor.redo();
                    "Redo"
                }
                Action::Reject => {
                    let before = view.editor.scene().clone();
                    let revision = view.editor.engine.revision();
                    let rejected = view.editor.transaction(|scene| {
                        scene.update_object(&"paragraph".into(), |object| {
                            let Shape::Text(text) = &mut object.shape else {
                                unreachable!()
                            };
                            text.letter_spacing = -1.;
                        })
                    });
                    assert!(rejected.is_err());
                    assert_eq!(view.editor.scene(), &before);
                    assert_eq!(view.editor.engine.revision(), revision);
                    "Rejected -1: scene and revision are unchanged"
                }
            };
            cx.notify();
            status
        });
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let scene = self.view.read(cx).editor.scene();
        let paragraph = text(scene, "paragraph");
        let anchor = text(scene, "anchor");
        let values = format!(
            "Mode {:?} · {:?} · {:?} · letters {} · words {} · indent {} · auto align {} · origin ({:.1}, {:.1})",
            paragraph.writing_mode,
            paragraph.overflow,
            paragraph.decoration,
            paragraph.letter_spacing,
            paragraph.word_spacing,
            paragraph.para_indent,
            anchor.auto_size_align,
            anchor.layout_origin.x,
            anchor.layout_origin.y
        );
        let mut buttons = div().h(px(56.)).px_4().flex().items_center().gap_3();
        for (id, title, action) in [
            ("toggle", "Apply / reset layout", Action::Toggle),
            ("mode", "Next writing mode", Action::Mode),
            ("formatting", "Ellipsis + lines", Action::Formatting),
            ("undo", "Undo", Action::Undo),
            ("redo", "Redo", Action::Redo),
            ("reject", "Reject -1", Action::Reject),
        ] {
            buttons = buttons.child(
                ControlButton::new(id, title)
                    .on_click(cx.listener(move |this, _, _, cx| this.action(action, cx))),
            );
        }
        div().size_full().flex().flex_col().bg(rgb(0x0d1421)).text_color(rgb(0xe6edf5))
            .child(buttons)
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(90.)).px_4().text_sm().child(self.status).child(div().child(values))
                .child(div().child("Only paragraph first rows indent. The selected bounds follow the fixed origin crosshair. Y columns advance left to right; reverse changes cluster order, and glyphs stay upright.")))
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
                    size(px(900.), px(550.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · Fixed text layout properties");
                cx.new(|cx| Lab::new(bridge, cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
}
