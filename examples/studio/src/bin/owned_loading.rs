//! Shared owned image/media loading. Pass worker, PNG and local media paths.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, MediaView, SceneView, SceneViewEvent,
    media_backend::{
        AudioOutput, FfmpegBackend, ImageLoadLimits, LoadLimits, MediaBackend, PlaybackOptions,
        ResourceLoadPool,
    },
    resources::SceneOwnedImageLoader,
    scene::{Affine, DisplayObject, Scene, Size as SceneSize, display},
};
use std::path::PathBuf;

fn document(path: &std::path::Path) -> Scene {
    let mut scene = Scene::new();
    scene
        .insert(
            DisplayObject::new(
                "image",
                display::Image::new(
                    path.to_string_lossy().into_owned(),
                    SceneSize::new(260., 180.),
                ),
            )
            .with_transform(Affine::translation(20., 20.)),
            None,
            None,
        )
        .unwrap();
    scene
}
struct Lab {
    views: [Option<Entity<SceneView>>; 2],
    subscriptions: [Option<Subscription>; 2],
    pool: ResourceLoadPool,
    loader: SceneOwnedImageLoader,
    image: PathBuf,
    media_source: PathBuf,
    media: Option<Entity<MediaView>>,
    status: String,
}
impl Lab {
    fn new(worker: PathBuf, image: PathBuf, media_source: PathBuf, cx: &mut Context<Self>) -> Self {
        let pool = ResourceLoadPool::new(LoadLimits {
            max_bytes: 128 * 1024 * 1024,
            max_jobs: 4,
        })
        .unwrap();
        let loader =
            SceneOwnedImageLoader::new(pool.clone(), worker, ImageLoadLimits::default()).unwrap();
        let mut lab = Self {
            views: [None, None],
            subscriptions: [None, None],
            pool,
            loader,
            image,
            media_source,
            media: None,
            status: "Two views share one image job".into(),
        };
        lab.create(cx);
        lab
    }
    fn create(&mut self, cx: &mut Context<Self>) {
        for i in 0..2 {
            self.subscriptions[i] = None;
            self.views[i] = None;
            let view = cx.new(|cx| {
                let mut v = SceneView::new(document(&self.image), cx).unwrap();
                v.show_grid = false;
                v.set_owned_image_loader(Some(self.loader.clone()), cx)
                    .unwrap();
                v
            });
            self.subscriptions[i] = Some(cx.subscribe(&view, move |_, _, event, cx| {
                if let SceneViewEvent::Resource(event) = event {
                    println!("resource view={i} {event:?}");
                    cx.notify();
                }
            }));
            self.views[i] = Some(view);
        }
    }
    fn record(&self, cx: &App) {
        println!(
            "record status={:?} pool={:?}",
            self.status,
            self.pool.usage()
        );
        for (i, view) in self.views.iter().enumerate() {
            if let Some(view) = view {
                let v = view.read(cx);
                if let Some(r) = v.resources() {
                    let handle = v.editor.engine.handle(&"image".into()).unwrap();
                    println!(
                        "view={i} scene_ticket={:?} load={:?} counts={:?}",
                        r.tracker().ticket(&v.editor.engine, handle),
                        r.image_load_event(&v.editor.engine, handle),
                        r.tracker().counts(&v.editor.engine)
                    );
                }
            }
        }
        if let Some(media) = &self.media {
            println!("media={:?}", media.read(cx).resource_event());
        }
    }
    fn action(&mut self, name: &str, cx: &mut Context<Self>) {
        self.status = name.into();
        match name {
            "Drop left" | "Drop right" => {
                let i = usize::from(name == "Drop right");
                self.subscriptions[i] = None;
                self.views[i] = None;
            }
            "Recreate" => self.create(cx),
            "Fail left" | "Restore left" => {
                if let Some(view) = &self.views[0] {
                    let path = if name == "Fail left" {
                        self.image.with_extension("missing-owned-loading")
                    } else {
                        self.image.clone()
                    };
                    if let Err(e) = view.update(cx, |v, cx| v.set_scene(document(&path), cx)) {
                        self.status = e.to_string();
                    }
                }
            }
            "Reload left" => {
                if let Some(view) = &self.views[0]
                    && let Err(e) = view.update(cx, |v, cx| v.reload_owned_images(cx))
                {
                    self.status = e.to_string();
                }
            }
            "Media" => {
                if self.media.is_none() {
                    let backend = FfmpegBackend::default().with_load_pool(self.pool.clone());
                    match backend.open(
                        &self.media_source,
                        PlaybackOptions {
                            audio: AudioOutput::Disabled,
                            max_width: 160,
                            max_height: 90,
                            ..Default::default()
                        },
                    ) {
                        Ok(controller) => {
                            self.media = Some(
                                cx.new(|cx| MediaView::new(controller, cx).preview_height(100.)),
                            )
                        }
                        Err(e) => self.status = e.to_string(),
                    }
                }
            }
            "Drop media" => {
                if let Some(view) = self.media.take() {
                    view.read(cx).controller.shutdown();
                    println!("media_closed={:?}", view.read(cx).resource_event());
                }
            }
            "Release all" => {
                self.subscriptions = [None, None];
                self.views = [None, None];
                if let Some(view) = self.media.take() {
                    view.read(cx).controller.shutdown();
                    println!("media_closed={:?}", view.read(cx).resource_event());
                }
            }
            "Close" => {
                cx.quit();
                return;
            }
            _ => {}
        }
        self.record(cx);
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut buttons = div().flex().flex_wrap().gap_2().p_3();
        for name in [
            "Drop left",
            "Drop right",
            "Recreate",
            "Fail left",
            "Restore left",
            "Reload left",
            "Media",
            "Drop media",
            "Release all",
            "Record",
            "Close",
        ] {
            buttons = buttons.child(
                ControlButton::new(name, name)
                    .on_click(cx.listener(move |s, _, _, cx| s.action(name, cx))),
            );
        }
        let mut panes = div().flex().gap_2().h(px(250.));
        for (i, view) in self.views.iter().enumerate() {
            let pane = div().flex_1().border_1().border_color(rgb(0x445566));
            panes = panes.child(match view {
                Some(v) => pane.child(v.clone()).into_any_element(),
                None => pane
                    .p_4()
                    .child(format!("View {i} released"))
                    .into_any_element(),
            });
        }
        let mut body = div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .bg(rgb(0x0b0f18))
            .text_color(rgb(0xeeeeee))
            .child(buttons)
            .child(panes);
        if let Some(media) = &self.media {
            body = body.child(div().h(px(210.)).child(media.clone()));
        }
        body.child(
            div()
                .p_2()
                .child(format!("{} | {:?}", self.status, self.pool.usage())),
        )
        .child("Record after a frame to read actual released counts. Media audio is disabled.")
    }
}
fn main() {
    let args = std::env::args_os()
        .skip(1)
        .map(PathBuf::from)
        .collect::<Vec<_>>();
    if args.len() != 3 {
        eprintln!("usage: owned_loading /absolute/flow-image-worker image.png local-media.wav");
        std::process::exit(2);
    }
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            gpui_kit::init(cx);
            let args = args.clone();
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1120.), px(700.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                move |window, cx| {
                    window.set_window_title("gpui-flow owned loading");
                    cx.new(|cx| Lab::new(args[0].clone(), args[1].clone(), args[2].clone(), cx))
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
