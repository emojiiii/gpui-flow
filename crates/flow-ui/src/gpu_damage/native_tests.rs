//! Opt-in Linux regression over actual SceneView/Window painting and submitted
//! wgpu work. Run alone: this opens a real native window and requires an adapter.
//! No screenshots, image encoding, synthetic paths, or analytic work counters.
use super::*;
use crate::SceneView;
use ::core::prelude::v1::test;
use gpui::prelude::*;
use gpui_flow_scene::{Affine, Color, DisplayObject, Scene as FlowScene, Style, display};
use gpui_wgpu::FlowHeadlessRenderer;
use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind, resume_unwind},
};

const STAGES: [&str; 12] = [
    "cold, two isolated instances",
    "warm unchanged",
    "local move",
    "local recolor",
    "local delete",
    "transparent z-order",
    "two commits before paint",
    "pan",
    "zoom",
    "resize",
    "unsupported state-style fallback",
    "recovery",
];
const MAX_STAGE_FRAMES: usize = 64;
type Outcome = Rc<RefCell<Option<Result<(), Box<dyn Any + Send>>>>>;

fn copy_scene(source: &g::Scene) -> g::Scene {
    let mut copy = g::Scene::default();
    copy.replay_balanced(0..source.len(), source).unwrap();
    copy.finish();
    copy
}

fn append_scene(destination: &mut g::Scene, source: &g::Scene) {
    destination
        .replay_balanced(0..source.len(), source)
        .unwrap();
}

fn assert_rgba(actual: &[u8], expected: &[u8], target: Size<DevicePixels>, label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: RGBA byte length");
    assert_eq!(
        actual.len(),
        target.width.0 as usize * target.height.0 as usize * 4
    );
    if let Some(offset) = actual.iter().zip(expected).position(|(a, b)| a != b) {
        panic!(
            "{label}: RGBA differs at ({}, {}), channel {}: {} != {}",
            (offset / 4) % target.width.0 as usize,
            (offset / 4) / target.width.0 as usize,
            offset % 4,
            actual[offset],
            expected[offset]
        );
    }
}

fn successful(receipt: FlowExecutionReceipt, namespace: FlowLayerNamespace) {
    assert_eq!(receipt.namespace, Some(namespace));
    assert!(
        receipt.encoded && receipt.submitted && receipt.readback_completed,
        "headless result must be checked, submitted GPU work: {receipt:?}"
    );
}

struct Capture {
    direct: g::Scene,
    cached: g::Scene,
    key: FrameKey,
    fallback: Option<&'static str>,
}

struct Sample {
    stage: usize,
    target: Size<DevicePixels>,
    keys: Vec<FrameKey>,
    fallback: Vec<Option<&'static str>>,
    layers: Vec<Option<LayerKey>>,
    paths: Vec<usize>,
    rgba: Vec<u8>,
    cached: FlowExecutionReceipt,
    full: FlowExecutionReceipt,
    direct: FlowExecutionReceipt,
}

struct Probe {
    // This renderer and namespace survive every stage and both Flow instances.
    persistent: FlowHeadlessRenderer,
    full: FlowHeadlessRenderer,
    direct: FlowHeadlessRenderer,
    namespace: FlowLayerNamespace,
    direct_namespace: FlowLayerNamespace,
    target: Size<DevicePixels>,
    stage: usize,
    captures: Vec<Capture>,
    sample: Option<Sample>,
    layer_keys: HashMap<u64, LayerKey>,
}

impl Probe {
    fn new() -> Self {
        let persistent = FlowHeadlessRenderer::new().expect("GPU oracle requires a usable adapter");
        eprintln!(
            "Flow GPU oracle adapter: {:?}. A Cpu device is software rendering, not physical-GPU evidence.",
            persistent.adapter_info()
        );
        let full = FlowHeadlessRenderer::new().expect("full-rebuild GPU reference adapter");
        let direct = FlowHeadlessRenderer::new().expect("direct GPU reference adapter");
        assert_eq!(
            persistent.adapter_info(),
            full.adapter_info(),
            "reference adapter changed"
        );
        assert_eq!(
            persistent.adapter_info(),
            direct.adapter_info(),
            "reference adapter changed"
        );
        Self {
            persistent,
            full,
            direct,
            namespace: FlowLayerNamespace::new_window_space(),
            direct_namespace: FlowLayerNamespace::new(),
            target: size(DevicePixels(1280), DevicePixels(640)),
            stage: 0,
            captures: Vec::new(),
            sample: None,
            layer_keys: HashMap::new(),
        }
    }

    fn capture(
        &mut self,
        direct: &g::Scene,
        cached: &g::Scene,
        key: &FrameKey,
        fallback: Option<&'static str>,
    ) {
        assert!(
            self.captures
                .iter()
                .all(|part| part.key.engine != key.engine),
            "a Flow surface must be captured once per frame"
        );
        assert!(
            direct.layers.frames.is_empty(),
            "reference must be the actual pre-cache Window paint"
        );
        assert!(
            direct.monochrome_sprites.is_empty()
                && direct.subpixel_sprites.is_empty()
                && direct.polychrome_sprites.is_empty()
                && direct.surfaces.is_empty(),
            "vector-only oracle cannot depend on the native window atlas"
        );
        assert!(
            direct.paths.len() >= 2 || matches!(self.stage, 10 | 11),
            "actual Flow Rect and cubic Path must reach Window::paint_path"
        );
        if let Some(layer) = cached.layers.frames.first() {
            assert_eq!(cached.layers.frames.len(), 1);
            if let Some(previous) = self.layer_keys.insert(key.engine, layer.key) {
                assert_eq!(
                    previous, layer.key,
                    "a bound Flow instance changed layer identity"
                );
            }
        }
        self.captures.push(Capture {
            direct: copy_scene(direct),
            cached: copy_scene(cached),
            key: key.clone(),
            fallback,
        });
        if self.captures.len() != 2 {
            return;
        }
        let captures = std::mem::take(&mut self.captures);
        let mut cached = g::Scene::default();
        let mut direct = g::Scene::default();
        let mut full = g::Scene::default();
        let mut layers = Vec::new();
        for capture in &captures {
            append_scene(&mut cached, &capture.cached);
            append_scene(&mut direct, &capture.direct);
            layers.push(capture.cached.layers.frames.first().map(|layer| layer.key));
            let layer = *self
                .layer_keys
                .get(&capture.key.engine)
                .expect("cold frame must be admitted");
            // Force the actual Cache admission and the actual GPU raster path.
            // A new namespace below prevents generation 1 from reusing old GPU tiles.
            let prepared = Cache::default().prepare(
                copy_scene(&capture.direct),
                capture.key.clone(),
                &Damage::new(capture.key.revision),
                layer,
                capture.fallback.is_none(),
                Admission::budget(MAX_TILES),
            );
            assert_eq!(prepared.fallback.is_some(), capture.fallback.is_some());
            append_scene(&mut full, &prepared.scene);
        }
        cached.finish();
        direct.finish();
        full.finish();
        let clear = rgba(0x182028ff);
        let cached = self
            .persistent
            .render(self.namespace, &cached, self.target, clear)
            .expect("persistent production GPU render/readback failed");
        let full_namespace = FlowLayerNamespace::new_window_space();
        let full = self
            .full
            .render(full_namespace, &full, self.target, clear)
            .expect("forced-full production GPU render/readback failed");
        let direct = self
            .direct
            .render(self.direct_namespace, &direct, self.target, clear)
            .expect("direct production GPU render/readback failed");
        successful(cached.receipt, self.namespace);
        successful(full.receipt, full_namespace);
        successful(direct.receipt, self.direct_namespace);
        assert_rgba(&cached.rgba, &direct.rgba, self.target, STAGES[self.stage]);
        assert_rgba(
            &cached.rgba,
            &full.rgba,
            self.target,
            "persistent vs forced-full",
        );
        self.sample = Some(Sample {
            stage: self.stage,
            target: self.target,
            keys: captures.iter().map(|capture| capture.key.clone()).collect(),
            fallback: captures.iter().map(|capture| capture.fallback).collect(),
            layers,
            paths: captures
                .iter()
                .map(|capture| capture.direct.paths.len())
                .collect(),
            rgba: cached.rgba,
            cached: cached.receipt,
            full: full.receipt,
            direct: direct.receipt,
        });
    }
}

struct TestRoot {
    views: [Entity<SceneView>; 2],
    probe: Rc<RefCell<Probe>>,
}
impl Render for TestRoot {
    fn render(&mut self, window: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let logical = window.viewport_size();
        let dpr = window.scale_factor();
        self.probe.borrow_mut().target = size(
            DevicePixels((f32::from(logical.width) * dpr).round() as i32),
            DevicePixels((f32::from(logical.height) * dpr).round() as i32),
        );
        div()
            .size_full()
            .flex()
            .child(
                div()
                    .relative()
                    .w(relative(0.5))
                    .h_full()
                    .overflow_hidden()
                    .child(self.views[0].clone()),
            )
            .child(
                div()
                    .relative()
                    .w(relative(0.5))
                    .h_full()
                    .overflow_hidden()
                    .child(self.views[1].clone()),
            )
    }
}

fn transform(dpr: f32, x: f32, y: f32) -> Affine {
    Affine::scale(1. / dpr, 1. / dpr) * Affine::translation(x, y)
}

fn fixture(dpr: f32, variant: usize) -> FlowScene {
    let mut scene = FlowScene::new();
    let primary = if variant == 0 {
        Color::hex(0xd94255)
    } else {
        Color::hex(0x347bea)
    };
    let secondary = if variant == 0 {
        Color::rgba(42, 204, 162, 137)
    } else {
        Color::rgba(248, 170, 38, 137)
    };
    scene
        .insert(
            DisplayObject::new("local", display::Rect::new(83., 71.).rounded(11.))
                .with_transform(transform(dpr, 79., 99.))
                .with_style(Style::fill(primary)),
            None,
            None,
        )
        .unwrap();
    // The same local geometry in two live instances, but deliberately different
    // paints. The curve and transparent overlap cross x/y = 512 device pixels.
    scene.insert(DisplayObject::new("curve", display::Path::from_svg(
        "M 350 485 C 385 410 530 620 589 467 C 614 518 555 618 461 584 C 406 565 372 548 350 485 Z"
    ).unwrap()).with_transform(transform(dpr, 0., 0.)).with_style(Style::fill(primary)), None, None).unwrap();
    scene
        .insert(
            DisplayObject::new("overlap", display::Rect::new(137., 119.).rounded(19.))
                .with_transform(transform(dpr, 463., 472.))
                .with_style(Style::fill(secondary)),
            None,
            None,
        )
        .unwrap();
    // Enough real vector geometry that warm instance uploads are measurably
    // smaller than direct painting, even with the backend's alignment padding.
    for index in 0..24 {
        scene
            .insert(
                DisplayObject::new(
                    format!("detail-{index}"),
                    display::Rect::new(17., 23.).rounded(5.),
                )
                .with_transform(transform(
                    dpr,
                    240. + (index % 8) as f32 * 31.,
                    130. + (index / 8) as f32 * 39.,
                ))
                .with_style(Style::fill(secondary)),
                None,
                None,
            )
            .unwrap();
    }
    scene
}

struct Driver {
    host: GpuDamageHost,
    root: Entity<TestRoot>,
    probe: Rc<RefCell<Probe>>,
    outcome: Outcome,
    completed: Rc<Cell<bool>>,
    stage: usize,
    frames: usize,
    dpr: f32,
    previous: Option<Sample>,
    cold_native: Option<FlowExecutionReceipt>,
    primed: bool,
    prime_keys: Option<Vec<FrameKey>>,
    stable_frames: usize,
}

impl Driver {
    fn prime(&mut self, sample: &Sample, window: &mut Window) -> bool {
        // X11 can deliver its mapped/configured size after the first paint.
        // Settle that real geometry before beginning the cold -> warm claim.
        if sample.target != size(DevicePixels(1280), DevicePixels(640)) {
            self.prime_keys = None;
            self.stable_frames = 0;
            window.resize(size(px(1280. / self.dpr), px(640. / self.dpr)));
            return false;
        }
        if self.prime_keys.as_ref() == Some(&sample.keys) {
            self.stable_frames += 1;
        } else {
            self.stable_frames = 0;
            self.prime_keys = Some(sample.keys.clone());
        }
        if self.stable_frames < 2 {
            return false;
        }
        // Begin the measured cold frame with explicitly empty validity in
        // both real backends; subsequent warm work must still be exactly zero.
        for input in self
            .host
            .record(window)
            .unwrap()
            .borrow()
            .bindings
            .values()
            .filter_map(Weak::upgrade)
        {
            input.borrow_mut().cache.invalidate();
        }
        self.probe.borrow_mut().namespace = FlowLayerNamespace::new_window_space();
        self.primed = true;
        self.frames = 0;
        true
    }

    fn ready(&self, sample: &Sample, cx: &App) -> bool {
        if sample.stage != self.stage {
            return false;
        }
        let root = self.root.read(cx);
        // Native-only coherent presentation can briefly withhold all content
        // while publishing its first snapshot. Never accept that as fallback proof.
        if sample.paths.iter().any(|count| *count < 2)
            || root.views.iter().any(|view| {
                let view = view.read(cx);
                view.compositing_pending || view.presented_geometry().is_none()
            })
            || (self.stage != 10 && sample.fallback.iter().any(Option::is_some))
        {
            return false;
        }
        if !sample.keys.iter().zip(&root.views).all(|(key, view)| {
            let engine = &view.read(cx).editor.engine;
            key.engine == engine.engine_id()
                && key.document == engine.document_epoch()
                && key.revision == engine.revision()
        }) {
            return false;
        }
        match self.stage {
            0 => sample.target == size(DevicePixels(1280), DevicePixels(640)),
            7 | 8 => {
                self.previous.as_ref().is_some_and(|old| {
                    sample.keys.iter().zip(&old.keys).all(|(key, old)| {
                        key.viewport.world_to_screen != old.viewport.world_to_screen
                    })
                })
            }
            9 => {
                self.previous.as_ref().is_some_and(|old| {
                    sample.target != old.target
                        && sample.keys.iter().zip(&old.keys).all(|(key, old)| {
                            key.viewport.canvas_bounds != old.viewport.canvas_bounds
                        })
                })
            }
            10 => sample.fallback.iter().all(Option::is_some),
            _ => sample.fallback.iter().all(Option::is_none),
        }
    }

    fn verify(&mut self, sample: &Sample, window: &Window) {
        assert_ne!(sample.keys[0].engine, sample.keys[1].engine);
        let native = self
            .host
            .status(window)
            .expect("bound native window status");
        if self.stage == 10 {
            assert!(sample.layers.iter().all(Option::is_none));
            assert!(
                sample
                    .fallback
                    .iter()
                    .all(|reason| *reason == Some("unsupported scene presentation"))
            );
            assert_eq!(sample.cached.tile_raster_passes, 0);
            assert_eq!(
                sample.cached.instance_upload_bytes,
                sample.direct.instance_upload_bytes
            );
            assert_eq!(native.admitted_tiles, 0);
            assert!(
                native.receipt.is_none(),
                "fallback cannot acknowledge cached tile work"
            );
            assert_eq!(native.fallback.len(), 2);
        } else {
            assert!(sample.fallback.iter().all(Option::is_none));
            assert!(
                native.fallback.is_empty(),
                "native fallback: {:?}",
                native.fallback
            );
            assert_ne!(
                sample.layers[0], sample.layers[1],
                "live Flow instances reused a layer key"
            );
            assert!(sample.layers.iter().all(Option::is_some));
            let receipt = native
                .receipt
                .expect("native submission was not acknowledged");
            assert!(receipt.encoded && receipt.submitted);
            assert!(
                !receipt.readback_completed,
                "native receipt must not claim a headless readback"
            );
            assert_eq!(
                receipt.namespace,
                Some(self.host.record(window).unwrap().borrow().namespace)
            );
            assert_eq!(
                receipt.tile_raster_passes, sample.cached.tile_raster_passes,
                "native and oracle recorded different tile workloads"
            );
            match self.stage {
                0 | 7 | 8 | 9 | 11 => {
                    assert!(sample.cached.tile_raster_passes > 0);
                    assert_eq!(
                        sample.cached.tile_raster_passes, sample.full.tile_raster_passes,
                        "geometry change/recovery did not rebuild all visible tiles"
                    );
                    if self.stage == 0 {
                        self.cold_native = Some(receipt);
                    }
                }
                1 => {
                    assert_eq!(
                        sample.cached.tile_raster_passes,
                        0,
                        "warm frame keys changed: previous={:?}, current={:?}",
                        self.previous.as_ref().map(|previous| &previous.keys),
                        sample.keys
                    );
                    assert_eq!(receipt.tile_raster_passes, 0);
                    assert!(
                        sample.cached.instance_upload_bytes < sample.full.instance_upload_bytes
                    );
                    assert!(
                        sample.cached.instance_upload_bytes < sample.direct.instance_upload_bytes
                    );
                    assert!(
                        receipt.instance_upload_bytes
                            < self.cold_native.unwrap().instance_upload_bytes
                    );
                }
                2..=6 => {
                    assert!(
                        sample.cached.tile_raster_passes > 0,
                        "local change produced no GPU tile work"
                    );
                    assert!(
                        sample.cached.tile_raster_passes < sample.full.tile_raster_passes,
                        "local change rebuilt as much GPU work as forced-full"
                    );
                    assert!(
                        sample.cached.instance_upload_bytes < sample.full.instance_upload_bytes
                    );
                    if matches!(self.stage, 2..=4) {
                        // The two equally sized panes have the same geometry.
                        // Updating the entire first pane while merely retaining
                        // the second must not pass as fine-grained local damage.
                        assert!(
                            sample.cached.tile_raster_passes * 2 < sample.full.tile_raster_passes,
                            "small local edit repainted at least a whole pane"
                        );
                    }
                }
                _ => unreachable!(),
            }
        }
        if let Some(previous) = &self.previous {
            if matches!(self.stage, 2..=6) {
                let row_bytes = sample.target.width.0 as usize * 4;
                let half_row = row_bytes / 2;
                for (row, previous_row) in sample
                    .rgba
                    .chunks_exact(row_bytes)
                    .zip(previous.rgba.chunks_exact(row_bytes))
                {
                    assert_eq!(
                        &row[half_row..],
                        &previous_row[half_row..],
                        "editing one Flow instance changed the other instance's pixels"
                    );
                }
            }
            if matches!(self.stage, 1 | 10 | 11) {
                assert_rgba(
                    &sample.rgba,
                    &previous.rgba,
                    sample.target,
                    "unchanged/fallback/recovery appearance",
                );
            } else {
                assert!(
                    sample.rgba != previous.rgba,
                    "{} did not change painted pixels",
                    STAGES[self.stage]
                );
            }
        }
        eprintln!(
            "Flow GPU stage {}: {}; cached/full/direct upload bytes = {}/{}/{}, actual cached/full tile passes = {}/{}",
            self.stage,
            STAGES[self.stage],
            sample.cached.instance_upload_bytes,
            sample.full.instance_upload_bytes,
            sample.direct.instance_upload_bytes,
            sample.cached.tile_raster_passes,
            sample.full.tile_raster_passes
        );
    }

    fn advance(&mut self, window: &mut Window, cx: &mut App) {
        let views = self.root.read(cx).views.clone();
        let stage = self.stage;
        let dpr = self.dpr;
        for (index, entity) in views.iter().enumerate() {
            entity.update(cx, |view, cx| {
                // Only the first instance changes during the local-edit stages;
                // the other must retain its independently colored cached tiles.
                if index == 0 {
                    match stage {
                        2 => {
                            view.editor
                                .engine
                                .transaction(|scene| {
                                    scene.update_object(&"local".into(), |object| {
                                        object.transform = transform(dpr, 104., 117.)
                                    })
                                })
                                .unwrap();
                        }
                        3 => {
                            view.editor
                                .engine
                                .transaction(|scene| {
                                    scene.update_object(&"local".into(), |object| {
                                        object.style.fill = Some(Color::hex(0xe6b53a).into())
                                    })
                                })
                                .unwrap();
                        }
                        4 => {
                            view.editor
                                .engine
                                .transaction(|scene| scene.remove(&"local".into()).map(|_| ()))
                                .unwrap();
                        }
                        5 => {
                            view.editor
                                .engine
                                .transaction(|scene| {
                                    scene.update_object(&"overlap".into(), |object| {
                                        object.z_index = -1
                                    })
                                })
                                .unwrap();
                        }
                        6 => {
                            let before = view.editor.engine.revision();
                            view.editor
                                .engine
                                .transaction(|scene| {
                                    scene.update_object(&"overlap".into(), |object| {
                                        object.transform = transform(dpr, 447., 460.)
                                    })
                                })
                                .unwrap();
                            view.editor
                                .engine
                                .transaction(|scene| {
                                    scene.update_object(&"curve".into(), |object| {
                                        object.style.fill = Some(Color::hex(0xb450d8).into())
                                    })
                                })
                                .unwrap();
                            assert_eq!(
                                view.editor.engine.revision(),
                                before + 2,
                                "two separate engine commits must precede one paint"
                            );
                        }
                        _ => {}
                    }
                }
                match stage {
                    7 => {
                        view.editor.viewport.pan = gpui_flow_core::Point::new(13. / dpr, -9. / dpr);
                    }
                    8 => {
                        view.editor.viewport.zoom = 0.91;
                    }
                    10 => {
                        let handle = view.editor.engine.handle(&"curve".into()).unwrap();
                        view.state_styles
                            .bind(&view.editor.engine, handle, Default::default())
                            .unwrap();
                    }
                    11 => view.state_styles.clear(),
                    _ => {}
                }
                cx.notify();
            });
        }
        if stage == 9 {
            window.resize(size(px(1312. / dpr), px(672. / dpr)));
        }
        self.probe.borrow_mut().stage = stage;
        self.root.update(cx, |_, cx| cx.notify());
        window.refresh();
    }
}

fn next_frame(driver: Rc<RefCell<Driver>>, window: &mut Window) {
    window.on_next_frame(move |window, cx| {
        let outcome = driver.borrow().outcome.clone();
        if outcome.borrow().is_some() {
            cx.quit();
            return;
        }
        let step = catch_unwind(AssertUnwindSafe(|| {
            let mut driver = driver.borrow_mut();
            driver.frames += 1;
            assert!(
                driver.frames <= MAX_STAGE_FRAMES,
                "{} did not produce the expected native frame in {MAX_STAGE_FRAMES} callbacks",
                STAGES[driver.stage]
            );
            let sample = driver.probe.borrow_mut().sample.take();
            if !driver.primed {
                if let Some(sample) = sample {
                    driver.prime(&sample, window);
                }
                driver.root.update(cx, |_, cx| cx.notify());
                window.refresh();
                return false;
            }
            if let Some(sample) = sample.filter(|sample| driver.ready(sample, cx)) {
                driver.verify(&sample, window);
                driver.previous = Some(sample);
                driver.stage += 1;
                driver.frames = 0;
                if driver.stage == STAGES.len() {
                    driver.completed.set(true);
                    return true;
                }
                driver.advance(window, cx);
            } else {
                window.refresh();
            }
            false
        }));
        match step {
            Err(error) => {
                *outcome.borrow_mut() = Some(Err(error));
                cx.quit();
            }
            Ok(true) => {
                *outcome.borrow_mut() = Some(Ok(()));
                cx.quit();
            }
            Ok(false) => next_frame(driver, window),
        }
    });
}

#[test]
#[ignore = "opens a Linux native GPU window; requires a real wgpu adapter; run alone with --test-threads=1"]
fn native_flow_paint_persistent_gpu_rgba_and_work() {
    let host = GpuDamageHost::default();
    let completed = Rc::new(Cell::new(false));
    let outcome: Outcome = Rc::new(RefCell::new(None));
    let completed_in_app = completed.clone();
    let outcome_in_app = outcome.clone();
    let probe = Rc::new(RefCell::new(Probe::new()));
    host.application()
        .with_assets(gpui_kit::assets::Assets)
        .run(move |cx| {
            let setup = catch_unwind(AssertUnwindSafe(|| {
                gpui_kit::init(cx);
                let (handle, root) = gpui_kit::open_window(
                    WindowOptions {
                        window_bounds: Some(WindowBounds::Windowed(Bounds::new(
                            point(px(0.), px(0.)),
                            size(px(1280.), px(640.)),
                        ))),
                        ..Default::default()
                    },
                    cx,
                    |window, cx| {
                        window.set_window_title("gpui-flow · real GPU damage regression");
                        let dpr = window.scale_factor();
                        assert!(dpr.is_finite() && dpr > 0.);
                        let views = std::array::from_fn(|index| {
                            cx.new(|cx| {
                                let mut view = SceneView::new(fixture(dpr, index), cx).unwrap();
                                view.show_grid = false;
                                view.theme.background = 0x182028;
                                view.set_gpu_damage(Some(&host), window, cx).unwrap();
                                view
                            })
                        });
                        cx.new(|_| TestRoot {
                            views,
                            probe: probe.clone(),
                        })
                    },
                )
                .unwrap();
                handle
                    .update(cx, |_, window, cx| {
                        let dpr = window.scale_factor();
                        let specs = window
                            .gpu_specs()
                            .expect("native window GPU adapter information");
                        eprintln!("Flow native window adapter: {specs:?}");
                        let callback_probe = probe.clone();
                        let callback_outcome = outcome_in_app.clone();
                        host.record(window).unwrap().borrow_mut().oracle =
                            Some(Box::new(move |direct, cached, key, fallback| {
                                if callback_outcome.borrow().is_some() {
                                    return;
                                }
                                if let Err(error) = catch_unwind(AssertUnwindSafe(|| {
                                    callback_probe
                                        .borrow_mut()
                                        .capture(direct, cached, key, fallback)
                                })) {
                                    *callback_outcome.borrow_mut() = Some(Err(error));
                                }
                            }));
                        let driver = Rc::new(RefCell::new(Driver {
                            host: host.clone(),
                            root,
                            probe: probe.clone(),
                            outcome: outcome_in_app.clone(),
                            completed: completed_in_app.clone(),
                            stage: 0,
                            frames: 0,
                            dpr,
                            previous: None,
                            cold_native: None,
                            primed: false,
                            prime_keys: None,
                            stable_frames: 0,
                        }));
                        window.resize(size(px(1280. / dpr), px(640. / dpr)));
                        window.refresh();
                        next_frame(driver, window);
                        cx.activate(true);
                    })
                    .unwrap();
            }));
            if let Err(error) = setup {
                *outcome_in_app.borrow_mut() = Some(Err(error));
                cx.quit();
            }
        });
    let result = outcome
        .borrow_mut()
        .take()
        .expect("native GPU test quit before its final assertions");
    if let Err(error) = result {
        resume_unwind(error);
    }
    assert!(
        completed.get(),
        "native GPU test did not complete all twelve stages"
    );
}
