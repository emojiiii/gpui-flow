//! Compact native owned-background lab. Pixel/budget oracles live in compositor
//! tests; stdout records actions and settled public state, not screenshot proof.
use gpui::{prelude::*, *};
use gpui_flow::{
    ControlButton, GpuiTextRasterizer, SceneCompositing, SceneInputOperation, SceneView,
    SceneViewEvent,
    core::Viewport,
    interaction::scene::SceneCommand,
    scene::{
        Affine, BlendMode, Bounds as B, Color, DisplayObject, Point as P, Scene, Size as S, Stroke,
        Style, display,
    },
};
use std::{collections::VecDeque, sync::Arc};

fn empty() -> Style {
    Style {
        fill: None,
        ..Default::default()
    }
}
fn document() -> Scene {
    let mut scene = Scene::new();
    let mut add = |object: DisplayObject, parent: Option<&str>| {
        scene.insert(object, parent.map(Into::into), None).unwrap();
    };
    add(DisplayObject::new("owner", display::Group::new()), None);
    for (i, color) in [0xee6677, 0x4477cc, 0x22aa88, 0xeeaa44, 0x9955bb]
        .into_iter()
        .enumerate()
    {
        add(
            DisplayObject::new(format!("stripe-{i}"), display::Rect::new(20., 164.))
                .with_transform(Affine::translation(24. + i as f32 * 46., 36.))
                .with_style(Style::fill(Color::hex(color))),
            Some("owner"),
        );
    }
    let mut text = display::Text::new("Real text • owned pixels", S::new(300., 34.));
    text.font_family = "DejaVu Sans".into();
    text.font_size = 22.;
    add(
        DisplayObject::new("text", text)
            .with_transform(Affine::translation(34., 73.))
            .with_style(Style::fill(Color::hex(0x102438))),
        Some("owner"),
    );
    add(
        DisplayObject::new(
            "image",
            display::Image::new(
                concat!(env!("CARGO_MANIFEST_DIR"), "/assets/checker.png"),
                S::new(50., 50.),
            ),
        )
        .with_transform(Affine::translation(114., 111.))
        .with_style(empty()),
        Some("owner"),
    );
    add(
        DisplayObject::new("canvas", display::Canvas::new(52., 46.))
            .with_transform(Affine::translation(179., 113.))
            .with_style(empty()),
        Some("owner"),
    );
    add(
        DisplayObject::new("clip", display::Frame::new(180., 108.).rounded(20.))
            .with_transform(Affine::translation(64., 64.))
            .with_style(empty()),
        Some("owner"),
    );
    add(
        DisplayObject::new(
            "glass",
            display::Rect::new(190., 116.).with_corners([24., 4., 24., 4.]),
        )
        .with_transform(Affine::translation(-6., -4.))
        .with_style(Style {
            background_blur: 1.,
            stroke: Some(Stroke::new(Color::rgba(255, 255, 255, 180), 1.)),
            ..empty()
        }),
        Some("clip"),
    );
    let mut second = DisplayObject::new("overlap", display::Ellipse::new(72., 72.))
        .with_transform(Affine::translation(216., 116.))
        .with_style(Style {
            background_blur: 0.75,
            stroke: Some(Stroke::new(Color::hex(0x173d54), 1.)),
            ..empty()
        });
    second.visible = false;
    add(second, Some("owner"));
    scene
}
struct Lab {
    view: Entity<SceneView>,
    bridge: Arc<GpuiTextRasterizer>,
    text_enabled: bool,
    canvas_variant: bool,
    edge: bool,
    log: VecDeque<String>,
    sequence: usize,
    _observe: Subscription,
    _input: Subscription,
}
impl Lab {
    fn new(bridge: Arc<GpuiTextRasterizer>, cx: &mut Context<Self>) -> Self {
        let view = cx.new(|cx| {
            let mut view = SceneView::new(document(), cx).unwrap();
            view.set_compositing_mode(SceneCompositing::OwnedCanvas, cx)
                .unwrap();
            view.theme.background = 0xe9f0f7;
            view.theme.grid = 0x526578;
            view.set_text_rasterizer(Some(bridge.clone()), cx).unwrap();
            view.set_editable(false, cx);
            view.set_input_tracing(true);
            view
        });
        let observe = cx.observe(&view, |_, _, cx| cx.notify());
        let input = cx.subscribe(&view, |this: &mut Self, _, event, cx| {
            if let SceneViewEvent::InputTrace(trace) = event
                && let SceneInputOperation::Canvas(
                    gpui_flow::interaction::CanvasEvent::PointerDown(pointer),
                ) = &trace.operation
            {
                this.record(
                    &format!(
                        "native pointer {:?}, capture={:?}",
                        pointer.position, trace.capture_owner
                    ),
                    cx,
                );
            }
        });
        let mut lab = Self {
            view,
            bridge,
            text_enabled: true,
            canvas_variant: false,
            edge: false,
            log: VecDeque::new(),
            sequence: 0,
            _observe: observe,
            _input: input,
        };
        lab.paint_canvas(cx);
        lab.record("ready; wait for pending=false, then Record", cx);
        lab
    }
    fn record(&mut self, action: &str, cx: &mut Context<Self>) {
        self.sequence += 1;
        let view = self.view.read(cx);
        let glass = &view.editor.scene().get(&"glass".into()).unwrap().object;
        let line = format!(
            "{} {action} | radius={} opacity={} mode={:?} zoom={:.2} pan={:?} pending={} presented={} errors={:?}",
            self.sequence,
            glass.style.background_blur,
            glass.style.opacity,
            view.compositing_mode,
            view.editor.viewport.zoom,
            view.editor.viewport.pan,
            view.compositing_pending,
            view.presented_geometry().is_some(),
            view.composite_diagnostics
        );
        println!("{line}");
        self.log.push_back(line);
        if self.log.len() > 4 {
            self.log.pop_front();
        }
        cx.notify();
    }
    fn edit(
        &mut self,
        id: &str,
        label: &str,
        edit: impl FnOnce(&mut DisplayObject),
        cx: &mut Context<Self>,
    ) {
        self.view.update(cx, |view, cx| {
            view.editor
                .transaction(|scene| scene.update_object(&id.into(), edit))
                .unwrap();
            cx.notify();
        });
        self.record(label, cx);
    }
    fn paint_canvas(&mut self, cx: &mut Context<Self>) {
        self.canvas_variant = !self.canvas_variant;
        let alternate = self.canvas_variant;
        self.view.update(cx, |view, cx| {
            let handle = view.editor.engine.handle(&"canvas".into()).unwrap();
            view.edit_canvas(
                handle,
                |canvas| {
                    canvas.clear()?;
                    for i in 0..5 {
                        canvas.fill_rect(
                            B::new(P::new(i as f32 * 11., 0.), S::new(6., 46.)),
                            Color::hex(if alternate { 0x082e50 } else { 0xf06a2d }),
                        )?;
                    }
                    Ok(())
                },
                cx,
            )
            .unwrap();
        });
        self.record("new immutable Canvas version", cx);
    }
    fn navigate(&mut self, zoom: f32, cx: &mut Context<Self>) {
        let pan = if self.edge {
            P::new(-64., -66.)
        } else {
            P::ZERO
        };
        self.view.update(cx, |view, cx| {
            view.execute_command(
                SceneCommand::SetViewport {
                    viewport: Viewport {
                        pan,
                        zoom,
                        ..Default::default()
                    },
                },
                cx,
            )
            .unwrap()
        });
        self.record("camera changed; source crosses output edges", cx);
    }
    fn reset(&mut self, cx: &mut Context<Self>) {
        self.text_enabled = true;
        self.edge = false;
        let bridge = self.bridge.clone();
        self.view.update(cx, |view, cx| {
            view.editor
                .transaction(|scene| {
                    scene.update_object(&"glass".into(), |o| {
                        o.style.background_blur = 1.;
                        o.style.opacity = 1.;
                    })?;
                    scene.update_object(&"owner".into(), |o| {
                        o.style.blend_mode = BlendMode::PassThrough
                    })?;
                    scene.update_object(&"overlap".into(), |o| o.visible = false)
                })
                .unwrap();
            view.set_text_rasterizer(Some(bridge), cx).unwrap();
            view.set_compositing_mode(SceneCompositing::OwnedCanvas, cx)
                .unwrap();
        });
        self.navigate(1., cx);
        self.record("restored owned complete source", cx);
    }
}
impl Render for Lab {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = self.view.read(cx);
        let errors = view
            .composite_diagnostics
            .iter()
            .map(|i| format!("{}: {:?}", i.object, i.error))
            .collect::<Vec<_>>()
            .join("; ");
        let status = format!(
            "{:?} | owner {:?} | radius {} | zoom {:.2} | DPR {:.2} | pending {} | presented {}",
            view.compositing_mode,
            view.editor
                .scene()
                .get(&"owner".into())
                .unwrap()
                .object
                .style
                .blend_mode,
            view.editor
                .scene()
                .get(&"glass".into())
                .unwrap()
                .object
                .style
                .background_blur,
            view.editor.viewport.zoom,
            window.scale_factor(),
            view.compositing_pending,
            view.presented_geometry().is_some()
        );
        div().size_full().flex().flex_col().bg(rgb(0x172331)).text_color(rgb(0xf1f5f9)).text_sm()
            .child(div().h(px(44.)).px_3().flex().items_center().gap_2()
                .child(ControlButton::new("radius","Radius 0/1/2").on_click(cx.listener(|s,_,_,cx|s.edit("glass","radius edit",|o|o.style.background_blur=if o.style.background_blur==1.{2.}else if o.style.background_blur==2.{0.}else{1.},cx))))
                .child(ControlButton::new("alpha","Opacity 1/half").on_click(cx.listener(|s,_,_,cx|s.edit("glass","opacity edit",|o|o.style.opacity=if o.style.opacity==1.{0.5}else{1.},cx))))
                .child(ControlButton::new("overlap","Overlap").on_click(cx.listener(|s,_,_,cx|s.edit("overlap","sequential earlier result",|o|o.visible= !o.visible,cx))))
                .child(ControlButton::new("owner","Normal owner").on_click(cx.listener(|s,_,_,cx|s.edit("owner","transparent owner toggle",|o|o.style.blend_mode=if o.style.blend_mode==BlendMode::Normal{BlendMode::PassThrough}else{BlendMode::Normal},cx))))
                .child(ControlButton::new("mode","Owned/Domains").on_click(cx.listener(|s,_,_,cx|{s.view.update(cx,|v,cx|v.set_compositing_mode(if v.compositing_mode==SceneCompositing::OwnedCanvas{SceneCompositing::Domains}else{SceneCompositing::OwnedCanvas},cx).unwrap());s.record("mode changed",cx);})))
                .child(ControlButton::new("undo","Undo").on_click(cx.listener(|s,_,_,cx|{let changed=s.view.update(cx,|v,cx|{let changed=v.editor.undo();cx.notify();changed});s.record(&format!("Undo changed={changed}"),cx);})))
                .child(ControlButton::new("reset","Reset").on_click(cx.listener(|s,_,_,cx|s.reset(cx)))))
            .child(div().h(px(44.)).px_3().flex().items_center().gap_2()
                .child(ControlButton::new("edge","Pan edge/home").on_click(cx.listener(|s,_,_,cx|{s.edge= !s.edge;s.navigate(1.,cx);})))
                .child(ControlButton::new("zoom","Zoom 1/1.2").on_click(cx.listener(|s,_,_,cx|{let zoom=if s.view.read(cx).editor.viewport.zoom==1.{1.2}else{1.};s.navigate(zoom,cx);})))
                .child(ControlButton::new("text","Text off/on").on_click(cx.listener(|s,_,_,cx|{s.text_enabled= !s.text_enabled;let bridge=s.text_enabled.then(||s.bridge.clone());s.view.update(cx,|v,cx|v.set_text_rasterizer(bridge,cx).unwrap());s.record("text producer withdrawal/recovery",cx);})))
                .child(ControlButton::new("canvas","Canvas paint").on_click(cx.listener(|s,_,_,cx|s.paint_canvas(cx))))
                .child(ControlButton::new("budget","Budget fail/restore").on_click(cx.listener(|s,_,_,cx|s.edit("glass","bounded work refusal/recovery",|o|o.style.background_blur=if o.style.background_blur==8.{1.}else{8.},cx))))
                .child(ControlButton::new("record","Record").on_click(cx.listener(|s,_,_,cx|s.record("operator settled-state checkpoint",cx))))
                .child(ControlButton::new("close","Close").on_click(cx.listener(|s,_,w,cx|{s.record("normal close requested",cx);w.remove_window();}))))
            .child(div().flex_1().min_h_0().overflow_hidden().child(self.view.clone()))
            .child(div().h(px(126.)).px_3().py_2().overflow_hidden()
                .child(status).child(if errors.is_empty(){"No compositor error".into()}else{errors})
                .child("Inspect glass over stripes, native glyphs, decoded checker and Canvas ink; rounded frame clips the write. Pan/zoom and Overlap exercise complete samples.")
                .child("Domains + PassThrough refuses; Normal creates a transparent owner. Reset before resource/budget recovery. Pixel oracles remain in unit tests.")
                .child(self.log.back().cloned().unwrap_or_default()))
    }
}
fn main() {
    let (app, bridge) =
        GpuiTextRasterizer::application(gpui_kit::platform::current_platform(false));
    app.with_assets(gpui_kit::assets::Assets).run(move |cx| {
        gpui_kit::init(cx);
        cx.on_window_closed(|cx, _| {
            println!("background_blur: window closed");
            cx.quit();
        })
        .detach();
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                    None,
                    size(px(920.), px(580.)),
                    cx,
                ))),
                ..Default::default()
            },
            move |window, cx| {
                window.set_window_title("gpui-flow · owned background blur");
                cx.new(|cx| Lab::new(bridge.clone(), cx))
            },
        )
        .unwrap();
        cx.activate(true);
    });
    println!("background_blur: event loop returned normally");
}
