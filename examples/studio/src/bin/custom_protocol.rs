//! Measured Custom resizing and cancellable asynchronous resource ownership.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, SceneCompositing, SceneToolWidget, SceneView, SceneViewEvent,
    display::{
        CustomRasterOutput, CustomRasterRequest, CustomRasterizer, DisplayElement, DisplayRenderer,
        RenderObject,
    },
    engine::*,
    interaction::scene::{SceneCommand, ToolSessionToken},
    resources::SceneCustomResourceProvider,
    scene::{self, Affine, Color, Contour, Scene, Shape, Style, display::Custom},
};
use serde_json::{Value, json};
use std::{cell::RefCell, rc::Rc, sync::Arc, time::Duration};

struct Card;
impl ElementDefinition for Card {
    fn kind(&self) -> &str {
        "async.card"
    }
    fn schema(&self) -> PropertySchema {
        PropertySchema {
            allow_unknown: false,
            properties: vec![
                PropertyDescriptor::number("width", 180., 40., 400., DirtyFlags::GEOMETRY),
                PropertyDescriptor::number("height", 100., 40., 220., DirtyFlags::GEOMETRY),
                PropertyDescriptor {
                    name: "source".into(),
                    value_type: PropertyType::String,
                    default: Some(json!("ready")),
                    required: false,
                    dirty: DirtyFlags::DATA,
                },
            ],
        }
    }
    fn shape(&self, p: &Value) -> Result<Shape, String> {
        let b = bounds(p);
        let (w, h) = (b.size.width, b.size.height);
        Ok(Custom::new(
            self.kind(),
            b,
            vec![Contour {
                closed: true,
                points: vec![
                    scene::Point::ZERO,
                    scene::Point::new(w, 0.),
                    scene::Point::new(w, h),
                    scene::Point::new(0., h),
                ],
            }],
        )
        .into())
    }
    fn measure(&self, p: &Value) -> Result<Option<CustomMeasurement>, String> {
        Ok(Some(CustomMeasurement {
            bounds: bounds(p),
            minimum: scene::Size::new(40., 40.),
            maximum: scene::Size::new(400., 220.),
        }))
    }
    fn resize_policy(&self) -> CustomResizePolicy {
        CustomResizePolicy::Dimensions
    }
    fn resize(&self, p: &Value, s: scene::Size) -> Result<Value, String> {
        let mut p = p.clone();
        p["width"] = json!(s.width);
        p["height"] = json!(s.height);
        Ok(p)
    }
    fn resource_key(&self, p: &Value) -> Result<Option<String>, String> {
        Ok(Some(p["source"].as_str().unwrap().into()))
    }
}
fn bounds(p: &Value) -> scene::Bounds {
    scene::Bounds::new(
        scene::Point::ZERO,
        scene::Size::new(
            p["width"].as_f64().unwrap() as f32,
            p["height"].as_f64().unwrap() as f32,
        ),
    )
}
struct Renderer;
impl DisplayRenderer for Renderer {
    fn render(&self, o: &RenderObject, _: &mut Window, _: &mut App) -> AnyElement {
        DisplayElement::new(o.clone()).into_any_element()
    }
}
// Independent CPU implementation. The native renderer above is never captured.
struct Rasterizer;
impl CustomRasterizer for Rasterizer {
    fn rasterize(
        &self,
        request: CustomRasterRequest<'_>,
    ) -> Result<Option<CustomRasterOutput>, String> {
        let Shape::Custom(custom) = &request.object.shape else {
            return Err("expected Custom".into());
        };
        let resource = request.resource.ok_or("accepted payload required")?;
        if !resource.bytes.starts_with(b"payload:") {
            return Err("unexpected payload".into());
        }
        let scale = request.world_transform.max_scale()
            * request.viewport.world_to_screen.max_scale()
            * request.viewport.device_scale;
        let width = (custom.bounds.size.width * scale).ceil().max(1.) as u32;
        let height = (custom.bounds.size.height * scale).ceil().max(1.) as u32;
        let work = u64::from(width) * u64::from(height);
        if width > request.limits.max_dimension
            || height > request.limits.max_dimension
            || work > request.max_pixel_work
            || work.saturating_mul(4) > request.max_output_bytes
        {
            return Err("host raster budget".into());
        }
        let mut pixels = Vec::new();
        pixels
            .try_reserve_exact(work as usize * 4)
            .map_err(|_| "allocation")?;
        let color = request
            .object
            .style
            .fills()
            .find_map(|paint| {
                if let scene::Paint::Solid { color } = paint {
                    Some(*color)
                } else {
                    None
                }
            })
            .unwrap_or(Color::TRANSPARENT);
        for y in 0..height {
            if request.cancellation.is_cancelled() {
                return Err("cancelled".into());
            }
            for x in 0..width {
                // A real transparent aperture demonstrates that CPU paint is not fallback contours.
                if x > width / 3 && x < width * 2 / 3 && y > height / 3 && y < height * 2 / 3 {
                    pixels.extend_from_slice(&[0; 4]);
                } else {
                    let a = u16::from(color.a);
                    pixels.extend_from_slice(&[
                        (u16::from(color.r) * a / 255) as u8,
                        (u16::from(color.g) * a / 255) as u8,
                        (u16::from(color.b) * a / 255) as u8,
                        color.a,
                    ]);
                }
            }
        }
        Ok(Some(CustomRasterOutput {
            width,
            height,
            premul_rgba: pixels,
            local_bounds: custom.bounds,
            pixel_work: work,
        }))
    }
}
struct Widget;
impl SceneToolWidget for Widget {
    fn render(&mut self, _: &mut Window, _: &mut Context<SceneView>) -> AnyElement {
        div()
            .p_3()
            .child("Custom sizing session: Resize, then Done or Cancel")
            .into_any_element()
    }
}
#[derive(Default, Debug)]
struct Counts {
    started: usize,
    live: usize,
    dropped: usize,
    cancelled: usize,
}
struct Guard {
    counts: Rc<RefCell<Counts>>,
    cancel: ResourceCancellationToken,
}
impl Drop for Guard {
    fn drop(&mut self) {
        let mut c = self.counts.borrow_mut();
        c.live -= 1;
        c.dropped += 1;
        c.cancelled += usize::from(self.cancel.is_cancelled());
    }
}
struct Lab {
    view: Option<Entity<SceneView>>,
    token: Option<ToolSessionToken>,
    counts: Rc<RefCell<Counts>>,
    status: String,
    _subscription: Subscription,
    readonly: bool,
}
impl Lab {
    fn new(cx: &mut Context<Self>) -> Self {
        let mut registry = ElementRegistry::default();
        registry.register(Card).unwrap();
        let mut scene = Scene::new();
        scene
            .insert(
                registry
                    .create("async.card", "card", json!({}))
                    .unwrap()
                    .with_transform(Affine::translation(80., 60.))
                    .with_style(Style::fill(Color::hex(0x42cbbb))),
                None,
                None,
            )
            .unwrap();
        let counts = Rc::new(RefCell::new(Counts::default()));
        let shared = counts.clone();
        let executor = cx.background_executor().clone();
        let provider: SceneCustomResourceProvider = Rc::new(move |request| {
            {
                let mut c = shared.borrow_mut();
                c.started += 1;
                c.live += 1;
            }
            let guard = Guard {
                counts: shared.clone(),
                cancel: request.cancellation.clone(),
            };
            let executor = executor.clone();
            Box::pin(async move {
                let _guard = guard;
                executor
                    .timer(Duration::from_millis(if request.key == "slow" {
                        20000
                    } else {
                        150
                    }))
                    .await;
                if request.key == "fail" {
                    Err("deliberate host failure".into())
                } else {
                    Ok(format!("payload:{}", request.key).into_bytes())
                }
            })
        });
        let view = cx.new(|cx| {
            let mut view =
                SceneView::from_engine(SceneEngine::with_registry(scene, registry).unwrap(), cx);
            view.show_grid = false;
            view.compositing_mode = SceneCompositing::OwnedCanvas;
            view.set_custom_rasterizer("async.card", Some(Arc::new(Rasterizer)), cx);
            view.renderers.register("async.card", Renderer);
            view.register_inner_tool("sizing", "custom", |_, _, _| Widget, cx)
                .unwrap();
            view.set_custom_resource_provider("async.card", Some(provider), cx)
                .unwrap();
            view
        });
        let subscription = cx.subscribe(&view, |_, _, event, cx| {
            if let SceneViewEvent::Resource(e) = event {
                println!("resource {:?} {:?}", e.ticket, e.state);
            }
            cx.notify();
        });
        Self {
            view: Some(view),
            token: None,
            counts,
            status: "Ready to observe".into(),
            _subscription: subscription,
            readonly: false,
        }
    }
    fn record(&mut self, cx: &App) {
        self.status = if let Some(entity) = &self.view {
            let v = entity.read(cx);
            let h = v.editor.engine.handle(&"card".into()).unwrap();
            let r = v.resources().unwrap();
            let ticket = r.tracker().ticket(&v.editor.engine, h);
            let state = ticket.and_then(|t| r.tracker().state(&v.editor.engine, t));
            let bytes = r.custom_resource_bytes(&v.editor.engine, h);
            format!(
                "state={state:?} payload={:?} size={:?} revision={} {:?}",
                bytes.map(|b| String::from_utf8_lossy(&b).to_string()),
                v.editor.engine.measure_custom(h).unwrap().bounds.size,
                v.editor.engine.revision(),
                self.counts.borrow()
            )
        } else {
            format!("released {:?}", self.counts.borrow())
        };
        println!("{}", self.status);
    }
    fn action(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        if name == "Release" {
            self.view = None;
            self.token = None;
            self.record(cx);
            cx.notify();
            return;
        }
        let Some(entity) = self.view.clone() else {
            self.record(cx);
            return;
        };
        entity.update(cx, |v, cx| {
            let h = v.editor.engine.handle(&"card".into()).unwrap();
            match name {
                "Open" => match v.open_inner_tool("sizing", h, window, cx) {
                    Ok(token) => self.token = Some(token),
                    Err(e) => println!("open rejected: {e}"),
                },
                "Resize" => {
                    if let Some(token) = self.token {
                        println!(
                            "resize {:?}",
                            v.resize_inner_tool(
                                token,
                                scene::Size::new(280., 140.),
                                scene::Point::new(0.5, 0.5),
                                cx
                            )
                        );
                    }
                }
                "Reject" => {
                    if let Some(token) = self.token {
                        let before = v.editor.scene().clone();
                        assert!(
                            v.resize_inner_tool(
                                token,
                                scene::Size::new(900., 140.),
                                scene::Point::ZERO,
                                cx
                            )
                            .is_err()
                        );
                        assert_eq!(v.editor.scene(), &before);
                    }
                }
                "Done" => {
                    v.finish_inner_tool(window, cx);
                    self.token = None;
                }
                "Cancel" => {
                    v.cancel_inner_tool(window, cx);
                    self.token = None;
                }
                "Undo" => {
                    println!("undo {:?}", v.execute_command(SceneCommand::Undo, cx));
                }
                "Redo" => {
                    println!("redo {:?}", v.execute_command(SceneCommand::Redo, cx));
                }
                "Slow" | "Ready" | "Fail" => {
                    println!(
                        "source {:?}",
                        v.execute_command(
                            SceneCommand::SetProperty {
                                id: "card".into(),
                                key: PropertyKey::Custom("source".into()),
                                value: json!(name.to_lowercase())
                            },
                            cx
                        )
                    );
                }
                "Readonly" => {
                    self.readonly = !self.readonly;
                    v.set_editable(!self.readonly, cx);
                }
                _ => {}
            }
        });
        self.record(cx);
        cx.notify();
    }
}
impl Render for Lab {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut buttons = div().flex().flex_wrap().gap_2().p_2();
        for name in [
            "Open", "Resize", "Reject", "Done", "Cancel", "Undo", "Redo", "Slow", "Ready", "Fail",
            "Readonly", "Release", "Record",
        ] {
            buttons = buttons.child(
                ControlButton::new(name, name)
                    .on_click(cx.listener(move |s, _, w, cx| s.action(name, w, cx))),
            );
        }
        let mut body = div()
            .size_full()
            .flex()
            .flex_col()
            .gap_2()
            .p_3()
            .bg(rgb(0x101824))
            .text_color(rgb(0xe3eef4))
            .child(buttons);
        if let Some(view) = &self.view {
            body = body.child(div().h(px(360.)).child(view.clone()));
        }
        body.child(div().p_2().child(self.status.clone())).child("After a frame, Record reads actual status. Slow→Ready cancels an outstanding future; Release then Record must show live=0.")
    }
}
fn main() {
    gpui_kit::application()
        .with_assets(gpui_kit::assets::Assets)
        .run(|cx| {
            gpui_kit::init(cx);
            gpui_kit::open_window(
                WindowOptions {
                    window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                        None,
                        size(px(1080.), px(660.)),
                        cx,
                    ))),
                    ..Default::default()
                },
                cx,
                |window, cx| {
                    window.set_window_title("Custom sizing and resources");
                    cx.new(Lab::new)
                },
            )
            .unwrap();
            cx.activate(true);
        });
}
