//! Retained display primitives gallery. No node graph or agent runtime is required.
use gpui::{prelude::*, *};
use gpui_flow::scene::{
    self as scene, Affine, Color, DisplayObject, EventKind, ListenerOptions, Scene, Style,
    display as shapes,
};
use gpui_flow::{ControlButton, FlowTheme, SceneView, SceneViewEvent};
use std::{cell::RefCell, rc::Rc};
fn gallery_scene() -> Scene {
    let mut scene = Scene::default();
    let p = scene::Point::new;
    let label = |id: &str, text: &str, x: f32, y: f32| {
        DisplayObject::new(id, shapes::Text::new(text, scene::Size::new(250., 35.)))
            .with_transform(Affine::translation(x, y))
            .with_style(Style::fill(Color::hex(0x9cafc9)))
    };
    let mut insert = |o| scene.insert(o, None, None).unwrap();
    insert(label(
        "heading",
        "DISPLAY PRIMITIVES · retained scene",
        35.,
        15.,
    ));
    insert(
        DisplayObject::new("rectangle", shapes::Rect::new(170., 105.).rounded(18.))
            .with_transform(Affine::translation(35., 75.))
            .with_style(Style::fill(Color::hex(0x63e2bf))),
    );
    insert(label(
        "rect-label",
        "Rect · fill / rounded corners",
        35.,
        195.,
    ));
    insert(
        DisplayObject::new("ellipse", shapes::Ellipse::new(160., 105.))
            .with_transform(Affine::translation(260., 75.))
            .with_style(Style {
                fill: Some(Color::hex(0xa8b5ff).into()),
                stroke: Some(scene::Stroke::new(Color::hex(0xe6edf7), 3.)),
                ..Default::default()
            }),
    );
    insert(label(
        "ellipse-label",
        "Ellipse · stroke + fill",
        260.,
        195.,
    ));
    insert(
        DisplayObject::new(
            "polygon",
            shapes::Polygon::new(vec![
                p(0., 95.),
                p(35., 5.),
                p(115., 0.),
                p(160., 65.),
                p(90., 105.),
            ]),
        )
        .with_transform(Affine::translation(480., 75.))
        .with_style(Style::fill(Color::hex(0xf5b971))),
    );
    insert(label(
        "polygon-label",
        "Polygon · local geometry",
        480.,
        195.,
    ));
    insert(
        DisplayObject::new("star", shapes::Star::new(6, 60., 28.))
            .with_transform(Affine::translation(725., 65.) * Affine::rotation(12.))
            .with_style(Style::fill(Color::hex(0xe19cff))),
    );
    insert(label("star-label", "Star · rotation", 700., 195.));
    let pen = shapes::Pen::new()
        .move_to(p(0., 90.))
        .cubic_to(p(30., -30.), p(120., 130.), p(165., 15.))
        .quadratic_to(p(200., 80.), p(245., 40.));
    insert(
        DisplayObject::new("pen", pen)
            .with_transform(Affine::translation(35., 280.))
            .with_style(Style::stroke(Color::hex(0x72cbf4), 7.)),
    );
    insert(label(
        "pen-label",
        "Pen / Path · Bezier geometry",
        35.,
        400.,
    ));
    insert(
        DisplayObject::new(
            "svg-arc",
            shapes::Path::from_svg("M0 80 A70 35 -25 0 1 135 20 S150 80 120 90").unwrap(),
        )
        .with_transform(Affine::translation(325., 280.))
        .with_style(Style::stroke(Color::hex(0x63e2bf), 6.)),
    );
    insert(label("line-label", "SVG Path · A/S curves", 310., 400.));
    insert(
        DisplayObject::new(
            "image",
            shapes::Image::new(
                concat!(env!("CARGO_MANIFEST_DIR"), "/assets/checker.png"),
                scene::Size::new(160., 100.),
            ),
        )
        .with_transform(Affine::translation(510., 280.)),
    );
    insert(label(
        "image-label",
        "Image · local PNG resource",
        490.,
        400.,
    ));
    let mut text = shapes::Text::new(
        "Text 文本\nNative font shaping",
        scene::Size::new(180., 105.),
    );
    text.font_size = 22.;
    insert(
        DisplayObject::new("text", text)
            .with_transform(Affine::translation(725., 285.))
            .with_style(Style::fill(Color::hex(0xe6edf7))),
    );
    insert(label(
        "text-label",
        "Text · content / font / wrap",
        700.,
        400.,
    ));
    insert(
        DisplayObject::new("frame", shapes::Frame::new(320., 170.).rounded(24.))
            .with_transform(Affine::translation(35., 485.))
            .with_style(Style::fill(Color::hex(0x1c2940))),
    );
    insert(label(
        "frame-label",
        "Frame · nested rounded clipping",
        35.,
        675.,
    ));
    insert(
        DisplayObject::new("group", shapes::Group::new())
            .with_transform(Affine::translation(485., 525.) * Affine::rotation(-14.)),
    );
    insert(label(
        "group-label",
        "Group · inherited transform + opacity",
        450.,
        675.,
    ));
    scene
        .insert(
            DisplayObject::new("clipped-rect", shapes::Rect::new(300., 95.))
                .with_transform(Affine::translation(-25., 20.) * Affine::rotation(25.))
                .with_style(Style::fill(Color::hex(0x63e2bf))),
            Some("frame".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("clipped-star", shapes::Star::new(5, 100., 40.))
                .with_transform(Affine::translation(210., 20.))
                .with_style(Style::fill(Color::hex(0xa8b5ff))),
            Some("frame".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("group-rect", shapes::Rect::new(180., 110.).rounded(15.))
                .with_style(Style::fill(Color::hex(0xf5b971))),
            Some("group".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("group-ellipse", shapes::Ellipse::new(130., 85.))
                .with_transform(Affine::translation(125., 30.))
                .with_style(Style {
                    opacity: 0.7,
                    ..Style::fill(Color::hex(0xe19cff))
                }),
            Some("group".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("box", shapes::Box::new(165., 175.).rounded(12.))
                .with_transform(Affine::translation(770., 480.))
                .with_style(Style::fill(Color::hex(0x1c2940))),
            None,
            None,
        )
        .unwrap();
    let custom = shapes::Custom::new(
        "demo.diamond",
        scene::Bounds::new(p(0., 0.), scene::Size::new(100., 100.)),
        vec![scene::Contour {
            points: vec![p(50., 0.), p(100., 50.), p(50., 100.), p(0., 50.)],
            closed: true,
        }],
    );
    scene
        .insert(
            DisplayObject::new("custom", custom.with_data(serde_json::json!({})))
                .with_transform(Affine::translation(32., 28.))
                .with_style(Style::fill(Color::hex(0x72cbf4))),
            Some("box".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            label("custom-label", "Custom factory · click to grow", 770., 675.),
            None,
            None,
        )
        .unwrap();
    scene
}
fn install_custom_listener(view: &mut SceneView) {
    let sender = view.action_sender();
    let handle = view.editor.engine.handle(&"custom".into()).unwrap();
    let mut grown = false;
    view.on(
        "custom",
        EventKind::Click,
        ListenerOptions::default(),
        move |_| {
            grown = !grown;
            let _ = sender.enqueue(
                handle,
                gpui_flow::interaction::scene::SceneCommand::SetProperty {
                    id: "custom".into(),
                    key: gpui_flow::engine::PropertyKey::Custom("radius".into()),
                    value: serde_json::json!(if grown { 70. } else { 50. }),
                },
            );
        },
    )
    .unwrap();
}
struct Gallery {
    view: Entity<SceneView>,
    events: Rc<RefCell<Vec<String>>>,
    _subscription: Subscription,
}
impl Gallery {
    fn new(cx: &mut Context<Self>) -> Self {
        let events = Rc::new(RefCell::new(Vec::new()));
        let log = events.clone();
        let view = cx.new(|cx| {
            let mut registry = gpui_flow::engine::ElementRegistry::with_builtins();
            registry
                .register(gpui_flow::engine::CustomDefinition::new(
                    "demo.diamond",
                    gpui_flow::engine::PropertySchema {
                        properties: vec![gpui_flow::engine::PropertyDescriptor::number(
                            "radius",
                            50.,
                            10.,
                            80.,
                            gpui_flow::engine::DirtyFlags::GEOMETRY
                                | gpui_flow::engine::DirtyFlags::HIT
                                | gpui_flow::engine::DirtyFlags::PAINT,
                        )],
                        allow_unknown: false,
                    },
                    |props: &serde_json::Value| {
                        let r = props["radius"].as_f64().unwrap() as f32;
                        Ok((
                            scene::Bounds::new(
                                scene::Point::ZERO,
                                scene::Size::new(r * 2., r * 2.),
                            ),
                            vec![scene::Contour {
                                points: vec![
                                    scene::Point::new(r, 0.),
                                    scene::Point::new(r * 2., r),
                                    scene::Point::new(r, r * 2.),
                                    scene::Point::new(0., r),
                                ],
                                closed: true,
                            }],
                        ))
                    },
                ))
                .unwrap();
            let engine =
                gpui_flow::engine::SceneEngine::with_registry(gallery_scene(), registry).unwrap();
            let mut view = SceneView::from_engine(engine, cx);
            install_custom_listener(&mut view);
            view.on(
                "frame",
                EventKind::PointerDown,
                ListenerOptions {
                    capture: true,
                    once: false,
                },
                move |event| {
                    let mut log = log.borrow_mut();
                    log.push(format!(
                        "{:?} · {} → {}",
                        event.phase, event.current_target, event.target
                    ));
                    if log.len() > 4 {
                        log.remove(0);
                    }
                },
            )
            .unwrap();
            view
        });
        let subscription = cx.subscribe(&view, |_, _, _: &SceneViewEvent, cx| cx.notify());
        Self {
            view,
            events,
            _subscription: subscription,
        }
    }
}
impl Render for Gallery {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = FlowTheme::default();
        let selected = self
            .view
            .read(cx)
            .editor
            .selection
            .ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let diagnostics = self.view.read(cx).diagnostics.len();
        div().size_full().flex().flex_col().bg(rgb(theme.background)).text_color(rgb(theme.foreground)).font_family("sans-serif")
        .child(div().h(px(64.)).flex_shrink_0().px_5().flex().items_center().justify_between().bg(rgb(theme.panel)).child(div().text_lg().child("◈ gpui-flow · display gallery"))
            .child(div().flex().gap_2()
                .child(ControlButton::new("edit-policy",if self.view.read(cx).is_editable(){"Editable"}else{"Read-only"}).on_click(cx.listener(|this,_,_,cx|{this.view.update(cx,|view,cx|view.set_editable(!view.is_editable(),cx));cx.notify();})))
                .child(ControlButton::new("drag-plugin",if self.view.read(cx).interactions.is_enabled("drag"){"Drag:on"}else{"Drag:off"}).on_click(cx.listener(|this,_,_,cx|{this.view.update(cx,|view,cx|{let enabled=view.interactions.is_enabled("drag");let _=view.set_plugin_enabled("drag",!enabled,cx);});cx.notify();})))
                .child(ControlButton::new("fit","Fit").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|view,cx|{view.fit();cx.notify();}))))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|view,cx|{let _=view.execute_command(gpui_flow::interaction::scene::SceneCommand::Undo,cx);cx.notify();}))))
                .child(ControlButton::new("redo","Redo").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|view,cx|{let _=view.execute_command(gpui_flow::interaction::scene::SceneCommand::Redo,cx);cx.notify();}))))
                .child(ControlButton::new("rotate","Rotate +15°").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|view,cx|{let _=view.execute_command(gpui_flow::interaction::scene::SceneCommand::RotateSelection { degrees:15. },cx);cx.notify();}))))
                .child(ControlButton::new("reset","Reset gallery").on_click(cx.listener(|this,_,_,cx|this.view.update(cx,|view,cx|{if view.set_scene(gallery_scene(),cx).is_ok(){install_custom_listener(view);}}))))))
        .child(div().flex().flex_1().min_h_0().child(div().flex_1().child(self.view.clone())).child(div().w(px(230.)).flex_shrink_0().p_4().bg(rgb(theme.panel)).flex().flex_col().gap_4()
            .child("SCENE EDITOR / 场景编辑")
            .child(div().text_sm().text_color(rgb(theme.muted)).child("Click / Shift-click: select\nDrag: move\nCorner handle: resize\nCtrl-click: parent\nBackground drag: marquee\nSpace/middle: pan\nCtrl-wheel: zoom\nR: rotate, +/-: scale\nF: fit, Delete: remove\nCtrl-Z/Y: undo/redo"))
            .child(div().text_xs().text_color(rgb(theme.accent)).child(format!("Selected: {selected}\nBackend diagnostics: {diagnostics}\nEngine revision: {}\nDirty objects: {}",self.view.read(cx).editor.engine.revision(),self.view.read(cx).editor.engine.last_change().dirty.len())))
            .child("CAPTURE EVENTS")
            .child(div().text_xs().text_color(rgb(theme.muted)).child(self.events.borrow().join("\n")))))
        .child(div().h(px(28.)).px_4().text_xs().text_color(rgb(theme.muted)).child("Independent primitives → retained scene tree → editor controller → GPUI adapter"))
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
                        size(px(1440.), px(980.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow Primitives Gallery");
                    gpui_kit::component::Theme::change(
                        gpui_kit::component::ThemeMode::Dark,
                        Some(window),
                        cx,
                    );
                    cx.new(Gallery::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
