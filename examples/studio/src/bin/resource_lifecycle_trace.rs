//! Controlled native lifecycle fixture. No URLs, file decoder, timers, or
//! fabricated ViewReady events: SceneView consumes the public ImageCache API.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneView, SceneViewEvent,
    engine::{ResourceState, ResourceTicket, ResourceViewMilestone},
    scene::{Affine, Color, DisplayObject, Scene, Shape, Size as SceneSize, Style, display},
};
use serde_json::json;
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, BTreeSet, VecDeque},
    rc::Rc,
    sync::Arc,
};

type ImageResult = Result<Arc<RenderImage>, ImageCacheError>;
struct Job {
    name: String,
    result: Option<ImageResult>,
    subscribers: BTreeSet<EntityId>,
}
#[derive(Default)]
struct FixtureStore {
    next: u64,
    jobs: BTreeMap<u64, Job>,
    current: BTreeMap<String, u64>,
}
impl FixtureStore {
    fn start(&mut self, name: &str) -> u64 {
        self.next += 1;
        let id = self.next;
        self.current.insert(name.into(), id);
        self.jobs.insert(
            id,
            Job {
                name: name.into(),
                result: None,
                subscribers: BTreeSet::new(),
            },
        );
        id
    }
    fn resolve(&self, name: &str) -> Option<Resource> {
        if name == "denied" {
            return None;
        }
        if name == "failure" {
            return Some(Resource::Embedded("failure".into()));
        }
        self.current
            .get(name)
            .map(|id| Resource::Embedded(id.to_string().into()))
    }
    fn settle(&mut self, newest: bool, failed: bool) -> Option<(u64, String, Vec<EntityId>)> {
        let id = if newest {
            self.jobs
                .iter()
                .rev()
                .find(|(_, j)| j.result.is_none())
                .map(|(id, _)| *id)
        } else {
            self.jobs
                .iter()
                .find(|(_, j)| j.result.is_none())
                .map(|(id, _)| *id)
        }?;
        let job = self.jobs.get_mut(&id).unwrap();
        job.result = Some(if failed {
            Err(ImageCacheError::Asset("controlled producer failure".into()))
        } else {
            // GPUI RenderImage consumes straight BGRA, not encoded bytes.
            let bgra = if job.name == "A" {
                [95, 190, 55, 255]
            } else {
                [220, 110, 200, 255]
            };
            let pixels = image::RgbaImage::from_fn(240, 120, |x, y| {
                if (x / 20 + y / 20) % 2 == 0 {
                    image::Rgba(bgra)
                } else {
                    image::Rgba([240, 240, 240, 255])
                }
            });
            Ok(Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])))
        });
        Some((
            id,
            job.name.clone(),
            std::mem::take(&mut job.subscribers).into_iter().collect(),
        ))
    }
}
struct FixtureCache(Rc<RefCell<FixtureStore>>);
impl ImageCache for FixtureCache {
    fn load(
        &mut self,
        resource: &Resource,
        window: &mut Window,
        _: &mut App,
    ) -> Option<ImageResult> {
        let Resource::Embedded(key) = resource else {
            return Some(Err(ImageCacheError::Asset(
                "fixture only permits embedded jobs".into(),
            )));
        };
        if key.as_ref() == "failure" {
            return Some(Err(ImageCacheError::Asset(
                "deliberate failed fixture".into(),
            )));
        }
        let mut store = self.0.borrow_mut();
        let job = key
            .parse::<u64>()
            .ok()
            .and_then(|id| store.jobs.get_mut(&id))?;
        if job.result.is_none() {
            job.subscribers.insert(window.current_view());
        }
        job.result.clone()
    }
}
fn hero(source: &str) -> DisplayObject {
    DisplayObject::new(
        "hero",
        display::Image::new(source, SceneSize::new(360., 180.)),
    )
    .with_transform(Affine::translation(40., 60.))
}
fn document() -> Scene {
    let mut scene = Scene::new();
    for object in [
        hero("A"),
        DisplayObject::new(
            "failure",
            display::Image::new("failure", SceneSize::new(100., 80.)),
        )
        .with_transform(Affine::translation(450., 70.)),
        DisplayObject::new(
            "denied",
            display::Image::new("denied", SceneSize::new(100., 80.)),
        )
        .with_transform(Affine::translation(580., 70.)),
        DisplayObject::new(
            "label",
            display::Text::new(
                "A = green checker / B = purple checker",
                SceneSize::new(640., 36.),
            ),
        )
        .with_transform(Affine::translation(40., 265.))
        .with_style(Style::fill(Color::hex(0x223344))),
    ] {
        scene.insert(object, None, None).unwrap();
    }
    scene
}
struct Lab {
    view: Option<Entity<SceneView>>,
    subscription: Option<Subscription>,
    cache: Entity<FixtureCache>,
    store: Rc<RefCell<FixtureStore>>,
    paints: Rc<Cell<u64>>,
    lines: VecDeque<String>,
    sequence: u64,
    ready: BTreeSet<(u64, u64)>,
    view_ready: BTreeSet<(u64, u64)>,
    complete: BTreeSet<(u64, u64, u64)>,
    terminal: BTreeSet<ResourceTicket>,
    violations: usize,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let store = Rc::new(RefCell::new(FixtureStore::default()));
        let cache = cx.new(|_| FixtureCache(store.clone()));
        cx.observe_release(&cache, |_, cache, cx| {
            for job in cache.0.borrow().jobs.values() {
                if let Some(Ok(image)) = &job.result {
                    cx.drop_image(image.clone(), None);
                }
            }
        })
        .detach();
        let mut lab = Self {
            view: None,
            subscription: None,
            cache,
            store,
            paints: Rc::new(Cell::new(0)),
            lines: VecDeque::new(),
            sequence: 0,
            ready: BTreeSet::new(),
            view_ready: BTreeSet::new(),
            complete: BTreeSet::new(),
            terminal: BTreeSet::new(),
            violations: 0,
        };
        lab.mount(cx);
        lab
    }
    fn log(&mut self, kind: &str, detail: String) {
        self.sequence += 1;
        eprintln!(
            "{}",
            json!({"fixture":"resource-lifecycle", "seq":self.sequence, "kind":kind,
            "paint_witnesses":self.paints.get(), "violations":self.violations, "detail":detail})
        );
        self.lines
            .push_back(format!("{:03} {kind}: {detail}", self.sequence));
        while self.lines.len() > 12 {
            self.lines.pop_front();
        }
    }
    fn check(&mut self, okay: bool, detail: &str) {
        if !okay {
            self.violations += 1;
            self.log("VIOLATION", detail.into());
        }
    }
    fn mount(&mut self, cx: &mut Context<Self>) {
        if self.view.is_some() {
            return;
        }
        self.paints = Rc::new(Cell::new(0));
        let id = self.store.borrow_mut().start("A");
        let store = self.store.clone();
        let cache = self.cache.clone();
        let view = cx.new(|cx| {
            let mut view = SceneView::new(document(), cx).unwrap();
            view.show_grid = false;
            view.set_editable(false, cx);
            view.set_image_resolver(Rc::new(move |source| store.borrow().resolve(source)), cx)
                .unwrap();
            view.set_image_cache(Some(cache.into()), cx).unwrap();
            view
        });
        self.subscription = Some(cx.subscribe(&view, |this, view, event, cx| {
            let engine = &view.read(cx).editor.engine;
            let engine_id = engine.engine_id();
            match event {
                SceneViewEvent::Resource(event) => {
                    if event.state.is_settled() {
                        let first = this.terminal.insert(event.ticket);
                        this.check(first, "terminal event repeated for identical ticket");
                    }
                    if event.state == ResourceState::Loaded {
                        let current = view
                            .read(cx)
                            .resources()
                            .and_then(|r| r.tracker().ticket(engine, event.ticket.handle));
                        this.check(
                            current == Some(event.ticket),
                            "loaded result reached stale/recreated instance",
                        );
                    }
                    this.log(
                        "Resource",
                        format!(
                            "{} {:?} g={} attach={} handle={:?} dispatch={}",
                            event.id,
                            event.state,
                            event.ticket.generation,
                            event.ticket.attachment_epoch,
                            event.ticket.handle,
                            event.dispatched
                        ),
                    );
                }
                SceneViewEvent::ResourceView(event) => {
                    let first = match event.milestone {
                        ResourceViewMilestone::Ready => {
                            this.ready.insert((engine_id, event.document_epoch))
                        }
                        ResourceViewMilestone::ViewReady => {
                            this.view_ready.insert((engine_id, event.document_epoch))
                        }
                        ResourceViewMilestone::Complete => this.complete.insert((
                            engine_id,
                            event.document_epoch,
                            event.resource_epoch,
                        )),
                    };
                    this.check(first, "view milestone repeated in the same epoch");
                    if event.milestone != ResourceViewMilestone::Ready {
                        this.check(
                            this.paints.get() > 0,
                            "paint-derived milestone preceded host paint traversal",
                        );
                    }
                    if event.milestone == ResourceViewMilestone::Complete {
                        this.check(
                            event.counts.pending == 0,
                            "Complete still has pending resources",
                        );
                    }
                    this.log(
                        "ResourceView",
                        format!(
                            "{:?} doc={} resource={} p={} l={} f={} c={}",
                            event.milestone,
                            event.document_epoch,
                            event.resource_epoch,
                            event.counts.pending,
                            event.counts.loaded,
                            event.counts.failed,
                            event.counts.cancelled
                        ),
                    );
                }
                _ => return,
            }
            cx.notify();
        }));
        self.view = Some(view);
        self.log(
            "mount",
            format!("new SceneView; A backend job {id}; shared cache retained"),
        );
        cx.notify();
    }
    fn source(&mut self, action: u8, cx: &mut Context<Self>) {
        let Some(view) = self.view.clone() else {
            return;
        };
        let current = match &view
            .read(cx)
            .editor
            .scene()
            .get(&"hero".into())
            .unwrap()
            .object
            .shape
        {
            Shape::Image(image) => image.source.clone(),
            _ => unreachable!(),
        };
        let target = match action {
            0 => "A",
            1 => "B",
            2 => "A",
            5 => "denied",
            _ => &current,
        }
        .to_owned();
        if action <= 1 && current == target {
            self.log(
                "action",
                format!("already {target}; use A/B/A for a new same-source generation"),
            );
            cx.notify();
            return;
        }
        let job = (target != "denied").then(|| self.store.borrow_mut().start(&target));
        view.update(cx, |view, cx| {
            let h = view.editor.engine.handle(&"hero".into()).unwrap();
            match action {
                3 => {
                    let (subtree, _) = view.editor.engine.detach(h).unwrap();
                    view.editor.engine.attach(&subtree, None, None).unwrap();
                }
                4 => {
                    view.editor.engine.destroy(h).unwrap();
                    view.editor
                        .engine
                        .transaction(|s| s.insert(hero(&target), None, None))
                        .unwrap();
                }
                _ => {
                    let sources = if action == 2 {
                        vec!["B", "A"]
                    } else {
                        vec![target.as_str()]
                    };
                    for source in sources {
                        view.editor
                            .transaction(|s| {
                                s.update_object(&"hero".into(), |o| {
                                    if let Shape::Image(image) = &mut o.shape {
                                        image.source = source.into();
                                    }
                                })
                            })
                            .unwrap();
                    }
                }
            }
            view.editor.selection.clear();
            cx.notify();
        });
        self.log(
            "action",
            format!(
                "{} -> {target}; backend job {job:?}; older jobs deliberately remain alive",
                match action {
                    0 | 1 => "source",
                    2 => "A/B/A (no intermediate render)",
                    3 => "detach/remount",
                    4 => "destroy+same ID",
                    _ => "cancel authorization",
                }
            ),
        );
        cx.notify();
    }
    fn finish(&mut self, newest: bool, failed: bool, cx: &mut Context<Self>) {
        let result = self.store.borrow_mut().settle(newest, failed);
        if let Some((id, name, subscribers)) = result {
            self.log(
                "backend",
                format!(
                    "job {id} {name} {} -> notify {} subscribed views",
                    if failed {
                        "FAILED"
                    } else {
                        "decoded BGRA READY"
                    },
                    subscribers.len()
                ),
            );
            // This public cache completion notification, not an explicit view
            // update, is what causes SceneView to consume the producer result.
            for entity in subscribers {
                App::notify(cx, entity);
            }
        } else {
            self.log(
                "backend",
                "no pending jobs; no duplicate completion produced".into(),
            );
        }
        cx.notify();
    }
    fn drop_view(&mut self, cx: &mut Context<Self>) {
        self.subscription = None;
        self.view = None;
        self.log(
            "drop",
            "SceneView released; host retains external cache; Mount creates a new tracker owner"
                .into(),
        );
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self
            .view
            .as_ref()
            .map(|view| {
                let view = view.read(cx);
                let h = view.editor.engine.handle(&"hero".into()).unwrap();
                let ticket = view
                    .resources()
                    .and_then(|r| r.tracker().ticket(&view.editor.engine, h));
                let counts = view
                    .resources()
                    .map(|r| r.tracker().counts(&view.editor.engine));
                format!(
                    "hero: {ticket:?}\ncounts: {counts:?}; resource_error: {:?}",
                    view.resource_error
                )
            })
            .unwrap_or_else(|| "No SceneView mounted; cache stays alive".into());
        let jobs = self
            .store
            .borrow()
            .jobs
            .iter()
            .map(|(id, job)| {
                format!(
                    "{id}:{}:{}",
                    job.name,
                    match &job.result {
                        None => "pending",
                        Some(Ok(_)) => "ready",
                        Some(Err(_)) => "failed",
                    }
                )
            })
            .collect::<Vec<_>>()
            .join("  ");
        let mut stage = div()
            .relative()
            .h(px(320.))
            .flex_shrink_0()
            .overflow_hidden()
            .bg(rgb(0xe9eef2));
        if let Some(view) = &self.view {
            let paints = self.paints.clone();
            stage = stage.child(view.clone()).child(
                canvas(
                    |_, _, _| (),
                    move |_, _, _, _| {
                        paints.set(paints.get() + 1);
                    },
                )
                .absolute()
                .size_full(),
            );
        }
        div().size_full().flex().flex_col().bg(rgb(0x111b28)).text_color(rgb(0xe8eef4))
            .child(div().px_3().py_2().text_lg().child(format!("LIF-02 Resource lifecycle | violations={} | paint witnesses={}", self.violations, self.paints.get())))
            .child(div().h(px(44.)).px_3().flex().gap_2().items_center()
                .child(ControlButton::new("a", "Source A").on_click(cx.listener(|s,_,_,cx| s.source(0,cx))))
                .child(ControlButton::new("b", "Source B").on_click(cx.listener(|s,_,_,cx| s.source(1,cx))))
                .child(ControlButton::new("aba", "A/B/A").on_click(cx.listener(|s,_,_,cx| s.source(2,cx))))
                .child(ControlButton::new("remount", "Detach / remount").on_click(cx.listener(|s,_,_,cx| s.source(3,cx))))
                .child(ControlButton::new("destroy", "Destroy + same ID").on_click(cx.listener(|s,_,_,cx| s.source(4,cx))))
                .child(ControlButton::new("deny", "Cancel / deny").on_click(cx.listener(|s,_,_,cx| s.source(5,cx)))))
            .child(div().h(px(44.)).px_3().flex().gap_2().items_center()
                .child(ControlButton::new("old", "Complete oldest").on_click(cx.listener(|s,_,_,cx| s.finish(false,false,cx))))
                .child(ControlButton::new("new", "Complete newest").on_click(cx.listener(|s,_,_,cx| s.finish(true,false,cx))))
                .child(ControlButton::new("fail", "Fail oldest").on_click(cx.listener(|s,_,_,cx| s.finish(false,true,cx))))
                .child(ControlButton::new("redraw", "Redraw").on_click(cx.listener(|s,_,_,cx| { if let Some(v) = &s.view { v.update(cx, |_,cx| cx.notify()); } cx.notify(); })))
                .child(ControlButton::new("drop", "Drop view").on_click(cx.listener(|s,_,_,cx| s.drop_view(cx))))
                .child(ControlButton::new("mount", "Mount view").on_click(cx.listener(|s,_,_,cx| s.mount(cx)))))
            .child(stage)
            .child(div().px_3().py_2().text_xs().child(status).child(format!("\nBackend jobs: {jobs}")))
            .child(div().px_3().text_xs().child("Ready = layout; ViewReady = actual GPUI paint receipt; Complete includes failure + cancellation. No GPU fence or hard cancellation claim."))
            .child(div().px_3().py_2().text_xs().children(self.lines.iter().map(|line| div().child(line.clone()))))
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
                        size(px(1250.), px(910.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Resource lifecycle trace");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
