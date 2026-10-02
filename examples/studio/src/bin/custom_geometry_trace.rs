//! CUS-01 native probe. Two live instances share no engine, listeners or renderer registry.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneCompositing, SceneView, SceneViewEvent,
    display::{DisplayElement, DisplayRenderer, RenderObject},
    engine::{
        CustomDefinition, DirtyFlags, ElementRegistry, PropertyDescriptor, PropertyKey,
        PropertySchema, SceneEngine,
    },
    interaction::scene::SceneCommand,
    scene::{
        self, Affine, Color, Contour, DisplayObject, EventKind, HitTestOptions, ListenerOptions,
        Scene, Shape, Style, display,
    },
};
use serde_json::{Value, json};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};

const KIND: &str = "acceptance.diamond";
const SMALL: f32 = 40.;
const LARGE: f32 = 70.;

fn engine() -> SceneEngine {
    let mut registry = ElementRegistry::default();
    registry
        .register(CustomDefinition::new(
            KIND,
            PropertySchema {
                properties: vec![PropertyDescriptor::number(
                    "radius",
                    SMALL as f64,
                    10.,
                    80.,
                    DirtyFlags::GEOMETRY | DirtyFlags::HIT | DirtyFlags::PAINT,
                )],
                allow_unknown: false,
            },
            |props: &Value| {
                let r = props["radius"].as_f64().unwrap() as f32;
                Ok((
                    scene::Bounds::new(scene::Point::ZERO, scene::Size::new(r * 2., r * 2.)),
                    vec![Contour {
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
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("domain", display::Group::new()),
            None,
            None,
        )
        .unwrap();
    scene
        .insert(
            registry
                .create(KIND, "custom", json!({}))
                .unwrap()
                .with_transform(Affine::translation(64., 88.))
                .with_style(Style::fill(Color::hex(0x8b8b8b))),
            Some("domain".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("sibling", display::Rect::new(100., 55.).rounded(8.))
                .with_transform(Affine::translation(255., 100.))
                .with_style(Style::fill(Color::hex(0xc69fff))),
            Some("domain".into()),
            None,
        )
        .unwrap();
    scene
        .insert(
            DisplayObject::new("outside", display::Rect::new(230., 25.).rounded(5.))
                .with_transform(Affine::translation(64., 265.))
                .with_style(Style::fill(Color::hex(0x72bf79))),
            None,
            None,
        )
        .unwrap();
    SceneEngine::with_registry(scene, registry).unwrap()
}
fn radius(engine: &SceneEngine) -> f32 {
    let Shape::Custom(custom) = &engine.scene().get(&"custom".into()).unwrap().object.shape else {
        unreachable!()
    };
    custom.data["radius"].as_f64().map_or(SMALL, |r| r as f32)
}
fn snapshot(engine: &SceneEngine) -> Value {
    let hit = |point| {
        engine
            .hit_test(point, HitTestOptions::default())
            .unwrap()
            .map(|hit| hit.id.to_string())
    };
    json!({"revision":engine.revision(), "radius":radius(engine),
        "bounds":engine.world_bounds(&"custom".into()).unwrap(),
        "expanded_probe":hit(scene::Point::new(174.,158.)),
        "corner_probe":hit(scene::Point::new(72.,96.)),
        "domain_opacity":engine.scene().get(&"domain".into()).unwrap().object.style.opacity,
        "custom":engine.scene().get(&"custom".into()).unwrap().object})
}

#[derive(Default)]
struct PaintProbe {
    calls: AtomicUsize,
    last: Mutex<Option<Value>>,
}
struct HostDiamond {
    view: usize,
    probe: Arc<PaintProbe>,
}
impl DisplayRenderer for HostDiamond {
    fn render(&self, object: &RenderObject, _: &mut Window, _: &mut App) -> AnyElement {
        let call = self.probe.calls.fetch_add(1, Ordering::Relaxed) + 1;
        let input = json!({"custom":object.object, "screen_transform":object.screen_transform,
            "opacity":object.opacity, "clips":object.clips.len()});
        let mut last = self.probe.last.lock().unwrap();
        if last.as_ref() != Some(&input) {
            println!(
                "{}",
                json!({"renderer":self.view + 1,"call":call,"input":input})
            );
            *last = Some(input);
        }
        // Explicit native host body reuses the supported vector primitive. It
        // deliberately colors a clone; canonical fallback remains gray.
        let mut painted = object.clone();
        painted.object.style.fill =
            Some(Color::hex(if self.view == 0 { 0x45dcd1 } else { 0xffbe5f }).into());
        DisplayElement::new(painted).into_any_element()
    }
}

struct Lab {
    views: [Entity<SceneView>; 2],
    radii: [Rc<Cell<f32>>; 2],
    probes: [Arc<PaintProbe>; 2],
    last: [Value; 2],
    checks: usize,
    failures: usize,
    subscriptions: Vec<Subscription>,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let radii = std::array::from_fn(|_| Rc::new(Cell::new(SMALL)));
        let probes: [Arc<PaintProbe>; 2] = std::array::from_fn(|_| Arc::default());
        let views = std::array::from_fn(|index| {
            cx.new(|cx| {
            let mut view = SceneView::from_engine(engine(), cx);
            view.show_grid = false;
            view.set_input_tracing(true);
            // Do not let this geometry probe's clicks move its fixed hit targets.
            view.set_plugin_enabled("drag", false, cx).unwrap();
            view.renderers.register(KIND, HostDiamond {view:index, probe:probes[index].clone()});
            let sender = view.action_sender();
            let handle = view.editor.engine.handle(&"custom".into()).unwrap();
            let current = radii[index].clone();
            view.on("custom", EventKind::Click, ListenerOptions::default(), move |_| {
                let next = if current.get() == SMALL {LARGE} else {SMALL};
                let result = sender.enqueue(handle, SceneCommand::SetProperty {
                    id:"custom".into(), key:PropertyKey::Custom("radius".into()), value:json!(next),
                });
                println!("{}", json!({"click":index + 1,"requested_radius":next,"queued":result.is_ok()}));
            }).unwrap();
            view
        })
        });
        let last = views
            .each_ref()
            .map(|view| snapshot(&view.read(cx).editor.engine));
        let mut lab = Self {
            views,
            radii,
            probes,
            last,
            checks: 0,
            failures: 0,
            subscriptions: vec![],
        };
        for index in 0..2 {
            lab.subscriptions.push(cx.subscribe(&lab.views[index], move |this, _, event, cx| {
                match event {
                    SceneViewEvent::Changed => this.record(index, "document", cx),
                    SceneViewEvent::InputTrace(trace) if trace.events.iter().any(|event| matches!(event.kind, EventKind::Click | EventKind::PointerDown | EventKind::PointerUp)) => {
                            println!("{}", json!({"native_input":index+1,
                                "operation":format!("{:?}",trace.operation),
                                "events":trace.events.iter().map(|event|json!({"kind":event.kind,
                                    "target":event.target,"world":event.world_position,"local":event.local_position})).collect::<Vec<_>>(),
                                "errors":trace.errors}));
                            this.record(index, "native_input", cx);
                    }
                    _ => {}
                }
            }));
        }
        println!(
            "{}",
            json!({"initial":lab.last,"coordinates":"world; zoom=1 pan=0"})
        );
        lab
    }
    fn record(&mut self, index: usize, reason: &str, cx: &mut Context<Self>) {
        let current = self
            .views
            .each_ref()
            .map(|view| snapshot(&view.read(cx).editor.engine));
        let isolated = current[1 - index] == self.last[1 - index];
        let r = current[index]["radius"].as_f64().unwrap();
        let geometry_ok = current[index]["bounds"]["size"]["width"].as_f64() == Some(r * 2.)
            && current[index]["bounds"]["size"]["height"].as_f64() == Some(r * 2.);
        let hit_ok = current[index]["expanded_probe"]
            == if r == LARGE as f64 {
                json!("custom")
            } else {
                Value::Null
            }
            && current[index]["corner_probe"].is_null();
        self.checks += 1;
        self.failures += usize::from(!(isolated && geometry_ok && hit_ok));
        println!(
            "{}",
            json!({"check":self.checks,"view":index+1,"reason":reason,
            "isolated":isolated,"geometry_ok":geometry_ok,"hit_ok":hit_ok,"states":current,
            "renderer_calls":self.probes.each_ref().map(|p|p.calls.load(Ordering::Relaxed)),
            "diagnostics":self.views.each_ref().map(|view|format!("{:?}",view.read(cx).composite_diagnostics))})
        );
        for (radius, state) in self.radii.iter().zip(&current) {
            radius.set(state["radius"].as_f64().unwrap() as f32);
        }
        self.last = current;
        cx.notify();
    }
    fn command(
        &mut self,
        index: usize,
        command: SceneCommand,
        reason: &str,
        cx: &mut Context<Self>,
    ) {
        self.views[index].update(cx, |view, cx| {
            view.execute_command(command, cx).unwrap();
            cx.notify();
        });
        self.record(index, reason, cx);
    }
    fn panel(&self, index: usize, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.views[index].read(cx);
        let state = snapshot(&view.editor.engine);
        let registered = view.renderers.get(KIND).is_some();
        let rejected = view
            .editor
            .scene()
            .get(&"domain".into())
            .unwrap()
            .object
            .style
            .opacity
            < 1.;
        let owned = view.compositing_mode == SceneCompositing::OwnedCanvas;
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .gap_2()
            .child(format!(
                "View {} | {} | radius {} | rev {}",
                index + 1,
                if registered {
                    "HOST renderer"
                } else {
                    "gray fallback"
                },
                state["radius"],
                state["revision"]
            ))
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        ControlButton::new(format!("undo-{index}"), "Undo").on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.command(index, SceneCommand::Undo, "undo", cx)
                            },
                        )),
                    )
                    .child(
                        ControlButton::new(format!("redo-{index}"), "Redo").on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.command(index, SceneCommand::Redo, "redo", cx)
                            },
                        )),
                    )
                    .child(
                        ControlButton::new(
                            format!("renderer-{index}"),
                            if registered {
                                "Remove renderer"
                            } else {
                                "Install renderer"
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let probe = this.probes[index].clone();
                            this.views[index].update(cx, |view, cx| {
                                if !view.renderers.remove(KIND) {
                                    view.renderers
                                        .register(KIND, HostDiamond { view: index, probe });
                                }
                                cx.notify();
                            });
                            this.record(index, "renderer toggle", cx);
                        })),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap_2()
                    .child(
                        ControlButton::new(
                            format!("reject-{index}"),
                            if rejected {
                                "Restore domain"
                            } else {
                                "Reject domain"
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.command(
                                index,
                                SceneCommand::SetProperty {
                                    id: "domain".into(),
                                    key: PropertyKey::Opacity,
                                    value: json!(if rejected { 1. } else { 0.5 }),
                                },
                                "domain opacity",
                                cx,
                            );
                        })),
                    )
                    .child(
                        ControlButton::new(
                            format!("owned-{index}"),
                            if owned {
                                "Domains mode"
                            } else {
                                "Owned canvas"
                            },
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.views[index].update(cx, |view, cx| {
                                view.compositing_mode = if owned {
                                    SceneCompositing::Domains
                                } else {
                                    SceneCompositing::OwnedCanvas
                                };
                                cx.notify();
                            });
                            this.record(index, "canvas mode", cx);
                        })),
                    )
                    .child(
                        ControlButton::new(format!("sample-{index}"), "Sample").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.record(index, "sample after paint", cx)
                            }),
                        ),
                    ),
            )
            .child(div().text_sm().child(format!(
                "Bounds {} | expanded hit {}\nRenderer calls {} | errors {:?}",
                state["bounds"],
                state["expanded_probe"],
                self.probes[index].calls.load(Ordering::Relaxed),
                view.composite_diagnostics
            )))
            .child(
                div()
                    .h(px(340.))
                    .w_full()
                    .border_1()
                    .border_color(rgb(0x475569))
                    .child(self.views[index].clone()),
            )
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div().size_full().p_4().flex().flex_col().gap_3().bg(rgb(0x101722)).text_color(rgb(0xe6edf7)).font_family("DejaVu Sans")
            .child("CUS-01 | click diamond: 40 <-> 70 | cyan/amber = explicit native host renderer")
            .child("Purple sibling shares domain; green bar is outside. Bounds/hit remain canonical during rendering rejection.")
            .child(div().flex().gap_4().child(self.panel(0,cx)).child(self.panel(1,cx)))
            .child(format!("{} state checks / {} failures | physical input + paint receipts on stdout",self.checks,self.failures))
            .child("Physical probes relative to each canvas: center (104,128), expanded-only (174,158), empty corner (72,96).")
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
                        size(px(1180.), px(650.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow Custom Geometry Acceptance");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[::core::prelude::v1::test]
    fn probe_coordinates_distinguish_contour_hit_from_bounds_and_track_geometry() {
        let mut engine = engine();
        let small = snapshot(&engine);
        assert_eq!(small["radius"], json!(SMALL));
        assert!(small["expanded_probe"].is_null());
        assert!(small["corner_probe"].is_null());
        let handle = engine.handle(&"custom".into()).unwrap();
        engine
            .set_property(handle, PropertyKey::Custom("radius".into()), json!(LARGE))
            .unwrap();
        let large = snapshot(&engine);
        assert_eq!(large["expanded_probe"], json!("custom"));
        assert!(large["corner_probe"].is_null());
        assert_eq!(large["bounds"]["size"]["width"], json!(140.));
        assert_eq!(
            large["custom"]["style"]["fill"],
            small["custom"]["style"]["fill"]
        );
    }
}
