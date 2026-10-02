//! Source-alpha picking with predecoded in-memory fixtures. Never fetches a URL.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, ImageAlphaHit, ImageHitFallback, SceneView, SceneViewEvent,
    scene::{
        Affine, Color, DisplayObject, EventKind, Point as ScenePoint, Scene, Shape,
        Size as SceneSize, Style, display,
    },
};
use std::{sync::Arc, time::Duration};

struct FixtureCache {
    image: Arc<RenderImage>,
}
impl FixtureCache {
    fn new(cx: &mut App) -> Entity<Self> {
        let frames = [false, true].map(|reverse| {
            let pixels = image::RgbaImage::from_fn(240, 120, |x, _| {
                let x = if reverse { 239 - x } else { x };
                let alpha = if x < 80 {
                    0
                } else if x < 160 {
                    128
                } else {
                    255
                };
                // RenderImage's public contract is straight BGRA.
                image::Rgba([75, 165, 245, alpha])
            });
            image::Frame::from_parts(pixels, 0, 0, image::Delay::from_numer_denom_ms(1000, 1))
        });
        let entity = cx.new(|_| Self {
            image: Arc::new(RenderImage::new(frames.to_vec())),
        });
        cx.observe_release(&entity, |cache, cx| {
            cx.drop_image(cache.image.clone(), None)
        })
        .detach();
        entity
    }
}
impl ImageCache for FixtureCache {
    fn load(
        &mut self,
        resource: &Resource,
        _: &mut Window,
        _: &mut App,
    ) -> Option<Result<Arc<RenderImage>, ImageCacheError>> {
        match resource {
            Resource::Embedded(name) if name.as_ref() == "pending" => None,
            Resource::Embedded(name) if name.as_ref() == "failed" => Some(Err(
                ImageCacheError::Asset("Deliberate fixture failure".into()),
            )),
            Resource::Embedded(name) if name.as_ref() == "ready" => Some(Ok(self.image.clone())),
            _ => Some(Err(ImageCacheError::Asset("Unknown fixture".into()))),
        }
    }
}
fn document() -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new("underlay", display::Rect::new(450., 320.))
                .with_transform(Affine::translation(110., 55.))
                .with_style(Style::fill(Color::hex(0x387fac))),
            None,
            None,
        )
        .unwrap();
    let mut clip = display::Frame::new(300., 220.).rounded(55.);
    clip.clip_children = false;
    let mut parent =
        DisplayObject::new("clip-parent", clip).with_transform(Affine::translation(155., 90.));
    parent.hit_policy = gpui_flow::scene::HitPolicy::None;
    parent.style.fill = None;
    scene.insert(parent, None, None).unwrap();
    let mut image = display::Image::new("ready", SceneSize::new(360., 240.));
    image.fit = display::ImageFit::Contain;
    scene
        .insert(
            DisplayObject::new("image", image),
            Some("clip-parent".into()),
            None,
        )
        .unwrap();
    scene
}
struct Lab {
    view: Entity<SceneView>,
    alpha: bool,
    threshold: u8,
    fallback: bool,
    frame: usize,
    paused: bool,
    last_pointer: String,
    _subscription: Subscription,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let cache = FixtureCache::new(cx);
        let view = cx.new(|cx| {
            let mut view = SceneView::new(document(), cx).unwrap();
            view.show_grid = false;
            view.set_editable(false, cx);
            view.set_image_resolver(
                std::rc::Rc::new(|source| {
                    matches!(source, "ready" | "pending" | "failed")
                        .then(|| Resource::Embedded(source.to_owned().into()))
                }),
                cx,
            )
            .unwrap();
            view.set_image_cache(Some(cache.into()), cx).unwrap();
            view
        });
        let subscription = cx.subscribe(&view, |this: &mut Self, _, event, cx| {
            if let SceneViewEvent::Pointer(event) = event
                && event.kind == EventKind::PointerDown
            {
                this.last_pointer = event.target.to_string();
                eprintln!("alpha-hit pointer-down: {}", event.target);
            }
            cx.notify();
        });
        let mut lab = Self {
            view,
            alpha: true,
            threshold: 0,
            fallback: false,
            frame: 0,
            paused: false,
            last_pointer: "none".into(),
            _subscription: subscription,
        };
        lab.policy(cx);
        lab
    }
    fn policy(&mut self, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            let h = view.editor.engine.handle(&"image".into()).unwrap();
            let fallback = if self.fallback {
                ImageHitFallback::Geometry
            } else {
                ImageHitFallback::PassThrough
            };
            view.set_image_alpha_hit(
                h,
                self.alpha.then_some(ImageAlphaHit {
                    threshold: self.threshold,
                    pending: fallback,
                    failed: fallback,
                }),
                cx,
            );
        });
        cx.notify();
    }
    fn change(&mut self, action: u8, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            view.editor
                .transaction(|scene| {
                    if action == 3 {
                        scene.update_object(&"clip-parent".into(), |o| {
                            if let Shape::Frame(frame) = &mut o.shape {
                                frame.clip_children = !frame.clip_children;
                            }
                        })?;
                    } else {
                        scene.update_object(&"image".into(), |o| {
                            let Shape::Image(image) = &mut o.shape else {
                                return;
                            };
                            match action {
                                0 => {
                                    image.fit = match image.fit {
                                        display::ImageFit::Contain => display::ImageFit::Cover,
                                        display::ImageFit::Cover => display::ImageFit::Fill,
                                        display::ImageFit::Fill => display::ImageFit::Contain,
                                    }
                                }
                                1 => {
                                    image.corner_radius =
                                        if image.corner_radius > 0. { 0. } else { 70. }
                                }
                                2 => {
                                    o.transform = if o.transform == Affine::IDENTITY {
                                        (Affine::rotation(18.)
                                            * Affine::skew(12., 0.)
                                            * Affine::scale(0.9, 0.8))
                                        .around(ScenePoint::new(180., 120.))
                                    } else {
                                        Affine::IDENTITY
                                    }
                                }
                                _ => {
                                    image.source = match image.source.as_str() {
                                        "ready" => "pending",
                                        "pending" => "failed",
                                        _ => "ready",
                                    }
                                    .into()
                                }
                            }
                        })?;
                    }
                    Ok(())
                })
                .unwrap();
            cx.notify();
        });
        if action == 4 {
            self.paused = false;
            self.frame = 0;
        }
        cx.notify();
    }
    fn remount(&mut self, cx: &mut Context<Self>) {
        self.view.update(cx, |view, cx| {
            let h = view.editor.engine.handle(&"image".into()).unwrap();
            let parent = view.editor.engine.handle(&"clip-parent".into()).unwrap();
            let (subtree, _) = view.editor.engine.detach(h).unwrap();
            view.editor
                .engine
                .attach(&subtree, Some(parent), None)
                .unwrap();
            view.editor.selection.clear();
            cx.notify();
        });
        self.alpha = false;
        self.paused = false;
        self.frame = 0;
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.paused {
            self.paused = self.view.update(cx, |view, cx| {
                let h = view.editor.engine.handle(&"image".into()).unwrap();
                view.set_image_playing(h, false, cx)
            });
        }
        let view = self.view.read(cx);
        let node = view.editor.scene().get(&"image".into()).unwrap();
        let Shape::Image(image) = &node.object.shape else {
            unreachable!()
        };
        let selection = view
            .editor
            .selection
            .ids
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(", ");
        let status = format!(
            "Mode: {} | threshold > {} | fit: {:?} | source: {} | frame: {} | fallback: {}\nSelected: {} | PointerDown target: {} | zoom: {:.1}",
            if self.alpha { "ALPHA" } else { "GEOMETRY" },
            self.threshold,
            image.fit,
            image.source,
            self.frame,
            if self.fallback {
                "Geometry"
            } else {
                "PassThrough"
            },
            if selection.is_empty() {
                "none"
            } else {
                &selection
            },
            self.last_pointer,
            view.editor.viewport.zoom
        );
        div().size_full().flex().flex_col().bg(rgb(0x101927)).text_color(rgb(0xe6edf5))
            .child(div().h(px(54.)).px_3().flex().items_center().gap_2()
                .child(ControlButton::new("alpha", "Geometry / Alpha").on_click(cx.listener(|s,_,_,cx| {s.alpha = !s.alpha; s.policy(cx);})))
                .child(ControlButton::new("threshold", "Threshold 0 / 127 / 128 / 255").on_click(cx.listener(|s,_,_,cx| {s.threshold = match s.threshold {0=>127,127=>128,128=>255,_=>0}; s.policy(cx);})))
                .child(ControlButton::new("fit", "Cycle fit").on_click(cx.listener(|s,_,_,cx|s.change(0,cx))))
                .child(ControlButton::new("corners", "Round corners").on_click(cx.listener(|s,_,_,cx|s.change(1,cx))))
                .child(ControlButton::new("affine", "Affine").on_click(cx.listener(|s,_,_,cx|s.change(2,cx))))
                .child(ControlButton::new("clip", "Ancestor clip").on_click(cx.listener(|s,_,_,cx|s.change(3,cx)))))
            .child(div().h(px(50.)).px_3().flex().items_center().gap_2()
                .child(ControlButton::new("source", "Ready / pending / failed").on_click(cx.listener(|s,_,_,cx|s.change(4,cx))))
                .child(ControlButton::new("fallback", "Fallback geometry / through").on_click(cx.listener(|s,_,_,cx| {s.fallback = !s.fallback; s.policy(cx);})))
                .child(ControlButton::new("frame", "Seek other frame").on_click(cx.listener(|s,_,_,cx| {
                    s.frame = 1 - s.frame;
                    s.view.update(cx, |view,cx| { let h = view.editor.engine.handle(&"image".into()).unwrap();
                        view.seek_image(h, Duration::from_millis(s.frame as u64 * 1000), cx); }); cx.notify();
                })))
                .child(ControlButton::new("remount", "Remount").on_click(cx.listener(|s,_,_,cx|s.remount(cx))))
                .child(ControlButton::new("undo", "Undo config").on_click(cx.listener(|s,_,_,cx| {
                    s.view.update(cx, |v,cx| {v.editor.undo(); cx.notify();}); s.paused = false; cx.notify();
                })))
                .child(ControlButton::new("zoom", "Zoom").on_click(cx.listener(|s,_,_,cx| {
                    s.view.update(cx, |v,cx| {v.editor.viewport.zoom = if v.editor.viewport.zoom < 1.2 {1.3} else {1.}; cx.notify();}); cx.notify();
                }))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(116.)).px_4().text_sm()
                .child(status)
                .child("\nBlue is selectable underlay. Image thirds: transparent / alpha 128 / opaque. Contain has top/bottom letterbox.\nClick each third: alpha holes and rejected thresholds reach underlay; geometry reaches image. Remount drops opt-in; toggle Alpha to reinstall."))
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
                        size(px(1080.), px(660.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("gpui-flow · Image alpha hit");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
