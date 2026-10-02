//! Native source-frame preview gestures and precise CPU crop pixels. No network.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, ImageAlphaHit, ImageHitFallback, SceneView, SceneViewEvent,
    interaction::scene::SceneCommand,
    scene::{
        Affine, BlendMode, Bounds as Rect, Color, DisplayObject, EventKind, Point as P, Scene,
        Shape, Size as S, Style, display,
    },
};
use std::sync::Arc;

struct Cache {
    image: Arc<RenderImage>,
}
impl Cache {
    fn new(cx: &mut App) -> Entity<Self> {
        let pixels = image::RgbaImage::from_fn(160, 120, |x, y| {
            if x < 40 {
                image::Rgba([70, 70, 240, 255])
            } else if x < 80 {
                image::Rgba([65, 210, 75, 255])
            } else if y < 60 {
                image::Rgba([235, 120, 50, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(pixels)]));
        let cache = cx.new(|_| Self { image });
        cx.observe_release(&cache, |cache, cx| cx.drop_image(cache.image.clone(), None))
            .detach();
        cache
    }
}
impl ImageCache for Cache {
    fn load(
        &mut self,
        resource: &Resource,
        _: &mut Window,
        _: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        match resource {
            Resource::Embedded(name) if name.as_ref() == "ready" => Some(Ok(self.image.clone())),
            Resource::Embedded(name) if name.as_ref() == "pending" => None,
            _ => Some(Err(ImageCacheError::Asset(
                "Deliberate crop fixture failure".into(),
            ))),
        }
    }
}
fn scene() -> Scene {
    let mut scene = Scene::new();
    for (id, x, cpu) in [("native", 100., false), ("cpu", 570., true)] {
        scene
            .insert(
                DisplayObject::new(format!("under-{id}"), display::Rect::new(320., 240.))
                    .with_transform(Affine::translation(x, 60.))
                    .with_style(Style::fill(Color::hex(0x76537a))),
                None,
                None,
            )
            .unwrap();
        let mut frame = DisplayObject::new(format!("parent-{id}"), display::Frame::new(320., 240.));
        frame.style.fill = None;
        frame.hit_policy = gpui_flow::scene::HitPolicy::None;
        frame.transform = Affine::translation(x, 60.);
        scene.insert(frame, None, None).unwrap();
        let mut image = display::Image::new("ready", S::new(320., 240.)).rounded(24.);
        image.fit = display::ImageFit::Fill;
        image.natural_size = Some(S::new(1., 999.));
        let mut object = DisplayObject::new(id, image);
        if cpu {
            object.style.blend_mode = BlendMode::Normal;
        }
        scene
            .insert(object, Some(format!("parent-{id}").into()), None)
            .unwrap();
    }
    scene
}
struct Lab {
    view: Entity<SceneView>,
    cache: Entity<Cache>,
    sequence: u64,
    last: String,
    _subscription: Subscription,
}
impl Lab {
    fn view(cache: Entity<Cache>, cx: &mut App) -> Entity<SceneView> {
        cx.new(|cx| {
            let mut v = SceneView::new(scene(), cx).unwrap();
            v.show_grid = false;
            v.set_input_tracing(true);
            v.set_image_resolver(
                std::rc::Rc::new(|source| Some(Resource::Embedded(source.to_owned().into()))),
                cx,
            )
            .unwrap();
            v.set_image_cache(Some(cache.into()), cx).unwrap();
            v
        })
    }
    fn subscribe(view: &Entity<SceneView>, cx: &mut Context<Self>) -> Subscription {
        cx.subscribe(view, |this: &mut Self, _, event, cx| {
            match event {
                SceneViewEvent::Changed => this.record("changed", cx),
                SceneViewEvent::InputTrace(trace) => {
                    if !matches!(
                        trace.operation,
                        gpui_flow::SceneInputOperation::Canvas(
                            gpui_flow::interaction::CanvasEvent::PointerMove(_)
                        )
                    ) || trace.capture_owner.is_some()
                    {
                        this.record(
                            &format!(
                                "input {:?} capture={:?} editing={} errors={:?}",
                                trace.operation, trace.capture_owner, trace.editing, trace.errors
                            ),
                            cx,
                        );
                    }
                }
                SceneViewEvent::Pointer(e) if e.kind == EventKind::PointerDown => {
                    this.last = e.target.to_string();
                }
                _ => {}
            }
            cx.notify();
        })
    }
    fn new(cx: &mut Context<Self>) -> Self {
        let cache = Cache::new(cx);
        let view = Self::view(cache.clone(), cx);
        let subscription = Self::subscribe(&view, cx);
        Self {
            view,
            cache,
            sequence: 0,
            last: "none".into(),
            _subscription: subscription,
        }
    }
    fn record(&mut self, action: &str, cx: &App) {
        self.sequence += 1;
        let v = self.view.read(cx);
        let images: Vec<_> = ["native", "cpu"]
            .iter()
            .map(|id| {
                let o = &v.editor.scene().get(&(*id).into()).unwrap().object;
                serde_json::json!({"id":id,"shape":o.shape,"transform":o.transform})
            })
            .collect();
        eprintln!(
            "{}",
            serde_json::json!({"fixture":"image-crop","seq":self.sequence,"action":action,"crop_active":v.image_crop_editor_active(),"editable":v.is_editable(),"images":images,"viewport_zoom":v.editor.viewport.zoom,"error":v.interaction_error})
        );
    }
    fn open(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            let h = v.editor.engine.handle(&id.into()).unwrap();
            if let Err(e) = v.open_image_crop_editor(h, window, cx) {
                v.interaction_error = Some(e.to_string());
            }
        });
        self.record(&format!("open {id}"), cx);
        cx.notify();
    }
    fn change(&mut self, action: u8, cx: &mut Context<Self>) {
        self.view.update(cx, |v, cx| {
            match action {
                0 => v.close_image_crop_editor(cx),
                1 => {
                    v.execute_command(SceneCommand::Undo, cx).unwrap();
                }
                2 => {
                    v.execute_command(SceneCommand::Redo, cx).unwrap();
                }
                3 => v.set_editable(!v.is_editable(), cx),
                4 => {
                    v.editor.viewport.zoom = if v.editor.viewport.zoom == 1. {
                        1.2
                    } else {
                        1.
                    };
                }
                5 => {
                    v.editor
                        .transaction(|s| {
                            for id in ["native", "cpu"] {
                                s.update_object(&id.into(), |o| {
                                    if let Shape::Image(i) = &mut o.shape {
                                        i.fit = match i.fit {
                                            display::ImageFit::Fill => display::ImageFit::Contain,
                                            display::ImageFit::Contain => display::ImageFit::Cover,
                                            _ => display::ImageFit::Fill,
                                        };
                                    }
                                })?;
                            }
                            Ok(())
                        })
                        .unwrap();
                }
                6 => {
                    v.editor
                        .transaction(|s| {
                            s.update_object(&"parent-cpu".into(), |o| {
                                o.transform = if o.transform.b == 0. {
                                    Affine::translation(610., 60.)
                                        * Affine::rotation(12.)
                                        * Affine::scale(0.9, 0.8)
                                } else {
                                    Affine::translation(570., 60.)
                                };
                                if let Shape::Frame(f) = &mut o.shape {
                                    f.corner_radius = if f.corner_radius == 0. { 50. } else { 0. };
                                }
                            })
                        })
                        .unwrap();
                }
                7 => {
                    for id in ["native", "cpu"] {
                        let h = v.editor.engine.handle(&id.into()).unwrap();
                        let policy = if v.image_alpha_hit(h).is_some() {
                            None
                        } else {
                            Some(ImageAlphaHit {
                                threshold: 0,
                                pending: ImageHitFallback::PassThrough,
                                failed: ImageHitFallback::PassThrough,
                            })
                        };
                        v.set_image_alpha_hit(h, policy, cx);
                    }
                }
                8 => {
                    v.editor
                        .transaction(|s| {
                            s.update_object(&"native".into(), |o| {
                                if let Shape::Image(i) = &mut o.shape {
                                    i.source = match i.source.as_str() {
                                        "ready" => "pending",
                                        "pending" => "failed",
                                        _ => "ready",
                                    }
                                    .into();
                                }
                            })
                        })
                        .unwrap();
                }
                9 | 10 => {
                    v.editor
                        .transaction(|s| {
                            for id in ["native", "cpu"] {
                                s.update_object(&id.into(), |o| {
                                    if let Shape::Image(i) = &mut o.shape {
                                        i.source_crop = Some(if action == 9 {
                                            Rect::new(P::new(0.5, 0.), S::new(0.5, 1.))
                                        } else {
                                            Rect::new(P::new(0.203, 0.167), S::new(0.503, 0.571))
                                        });
                                    }
                                })?;
                            }
                            Ok(())
                        })
                        .unwrap();
                }
                _ => {}
            }
            cx.notify();
        });
        self.record(&format!("button {action}"), cx);
        cx.notify();
    }
    fn remount(&mut self, cx: &mut Context<Self>) {
        self.view = Self::view(self.cache.clone(), cx);
        self._subscription = Self::subscribe(&self.view, cx);
        self.record("remount", cx);
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let v = self.view.read(cx);
        let state = format!(
            "Crop active={} • Editable={} • last hit={} • zoom={} • {:?}",
            v.image_crop_editor_active(),
            v.is_editable(),
            self.last,
            v.editor.viewport.zoom,
            v.interaction_error
        );
        div().size_full().flex().flex_col().bg(rgb(0x18222e)).text_color(rgb(0xe3ebf4))
        .child(div().h(px(48.)).px_3().flex().items_center().gap_2()
            .child(ControlButton::new("crop-native","Crop left").on_click(cx.listener(|s,_,w,cx|s.open("native",w,cx))))
            .child(ControlButton::new("crop-cpu","Crop CPU").on_click(cx.listener(|s,_,w,cx|s.open("cpu",w,cx))))
            .child(ControlButton::new("done","Done").on_click(cx.listener(|s,_,_,cx|s.change(0,cx))))
            .child(ControlButton::new("undo","Undo").on_click(cx.listener(|s,_,_,cx|s.change(1,cx))))
            .child(ControlButton::new("redo","Redo").on_click(cx.listener(|s,_,_,cx|s.change(2,cx))))
            .child(ControlButton::new("readonly","Read-only").on_click(cx.listener(|s,_,_,cx|s.change(3,cx))))
            .child(ControlButton::new("zoom","Zoom").on_click(cx.listener(|s,_,_,cx|s.change(4,cx)))))
        .child(div().h(px(48.)).px_3().flex().items_center().gap_2()
            .child(ControlButton::new("fit","Cycle fit").on_click(cx.listener(|s,_,_,cx|s.change(5,cx))))
            .child(ControlButton::new("affine","Affine + clip CPU").on_click(cx.listener(|s,_,_,cx|s.change(6,cx))))
            .child(ControlButton::new("alpha","Alpha hit").on_click(cx.listener(|s,_,_,cx|s.change(7,cx))))
            .child(ControlButton::new("source","Source ready/pending/failed").on_click(cx.listener(|s,_,_,cx|s.change(8,cx))))
            .child(ControlButton::new("preset","Right-half both").on_click(cx.listener(|s,_,_,cx|s.change(9,cx))))
            .child(ControlButton::new("fractional","Fractional both").on_click(cx.listener(|s,_,_,cx|s.change(10,cx))))
            .child(ControlButton::new("remount","Remount / reset").on_click(cx.listener(|s,_,_,cx|s.remount(cx)))))
        .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
        .child(div().h(px(92.)).px_4().text_sm().child(state).child("\nLeft full-source preview = native, right = CPU; all committed source crops = CPU. Crop shows full-source Fill preview; yellow handles resize, inside pans. Done/Enter shows cropped fit. Escape cancels active drag; a second Escape closes. Undo/Redo is one per completed drag. Purple is transparent underlay."))
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
                        size(px(1130.), px(710.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |w, cx| {
                    w.set_window_title("gpui-flow · Image crop trace");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
