use super::*;
use gpui_flow_engine::ResourceCancellationToken;
use std::{
    cell::RefCell,
    future::{Future, poll_fn},
    pin::Pin,
    task::Poll,
};

pub const MAX_CUSTOM_RESOURCE_BYTES: usize = 1024 * 1024;
pub const MAX_CUSTOM_RESOURCE_JOBS: usize = 64;
/// One immutable, generation-bound request. A key is data, never authorization.
#[derive(Clone)]
pub struct CustomResourceRequest {
    pub ticket: ResourceTicket,
    pub kind: String,
    pub key: String,
    pub properties: serde_json::Value,
    pub cancellation: ResourceCancellationToken,
}
pub type CustomResourceFuture = Pin<Box<dyn Future<Output = Result<Vec<u8>, String>> + 'static>>;
/// The host supplies its authorized I/O. Dropping the future cancels this
/// adapter's work; external threads/processes must honor the token or own a
/// cancellation guard. This is not arbitrary-thread hard termination.
pub type SceneCustomResourceProvider = Rc<dyn Fn(CustomResourceRequest) -> CustomResourceFuture>;

type ResultCell = Rc<RefCell<Option<Result<Arc<[u8]>, String>>>>;
struct Job {
    result: ResultCell,
    _task: gpui::Task<()>,
}
#[derive(Default)]
pub(super) struct CustomResources {
    pub providers: BTreeMap<String, SceneCustomResourceProvider>,
    jobs: BTreeMap<ResourceTicket, Job>,
}
impl CustomResources {
    pub fn clear(&mut self) {
        self.jobs.clear();
    }
    pub fn retain(&mut self, live: impl Fn(ResourceTicket) -> bool) {
        self.jobs.retain(|ticket, _| live(*ticket));
    }
    pub fn bytes(&self, ticket: ResourceTicket) -> Option<Arc<[u8]>> {
        self.jobs
            .get(&ticket)?
            .result
            .borrow()
            .as_ref()?
            .as_ref()
            .ok()
            .cloned()
    }
    pub fn prepare(
        &mut self,
        request: CustomResourceRequest,
        window: &mut Window,
        cx: &mut App,
    ) -> Option<ResourceOutcome> {
        let ticket = request.ticket;
        if !self.jobs.contains_key(&ticket) {
            let Some(provider) = self.providers.get(&request.kind) else {
                return Some(ResourceOutcome::Failed(
                    "no custom resource producer registered by host".into(),
                ));
            };
            if self.jobs.len() >= MAX_CUSTOM_RESOURCE_JOBS {
                return Some(ResourceOutcome::Failed(
                    "custom resource job capacity exceeded".into(),
                ));
            }
            let cancel = request.cancellation.clone();
            let future = match catch_unwind(AssertUnwindSafe(|| provider(request))) {
                Ok(future) => future,
                Err(_) => {
                    return Some(ResourceOutcome::Failed(
                        "custom resource producer panicked".into(),
                    ));
                }
            };
            let result: ResultCell = Default::default();
            let output = result.clone();
            let task = window.spawn(cx, async move |cx| {
                if let Some(outcome) = run_producer(future, cancel).await {
                    *output.borrow_mut() = Some(outcome);
                    let _ = cx.update(|window, _| window.refresh());
                }
            });
            self.jobs.insert(
                ticket,
                Job {
                    result,
                    _task: task,
                },
            );
        }
        self.jobs.get(&ticket).and_then(|job| {
            job.result.borrow().as_ref().map(|result| match result {
                Ok(_) => ResourceOutcome::Loaded,
                Err(error) => ResourceOutcome::Failed(error.clone()),
            })
        })
    }
}

async fn run_producer(
    mut future: CustomResourceFuture,
    cancel: ResourceCancellationToken,
) -> Option<Result<Arc<[u8]>, String>> {
    let mut cancelled = Box::pin(cancel.cancelled());
    let outcome = poll_fn(|cx| {
        if cancelled.as_mut().poll(cx).is_ready() {
            return Poll::Ready(None);
        }
        match catch_unwind(AssertUnwindSafe(|| future.as_mut().poll(cx))) {
            Ok(Poll::Pending) => Poll::Pending,
            Ok(Poll::Ready(result)) => Poll::Ready(Some(result)),
            Err(_) => Poll::Ready(Some(Err("custom resource future panicked".into()))),
        }
    })
    .await;
    drop(future);
    if cancel.is_cancelled() {
        return None;
    }
    outcome.map(|result| {
        result
            .map_err(|error| error.chars().take(4096).collect())
            .and_then(|bytes| {
                if bytes.len() > MAX_CUSTOM_RESOURCE_BYTES
                    || bytes.capacity() > MAX_CUSTOM_RESOURCE_BYTES
                {
                    Err("custom resource output exceeds 1 MiB".into())
                } else {
                    Ok(Arc::from(bytes))
                }
            })
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::task::{Context, Waker};
    fn poll<T>(future: Pin<&mut impl Future<Output = T>>) -> Poll<T> {
        future.poll(&mut Context::from_waker(Waker::noop()))
    }
    #[test]
    fn producer_output_bound_and_panics_are_terminal_errors() {
        let cases: Vec<CustomResourceFuture> = vec![
            Box::pin(async { Ok(vec![7; MAX_CUSTOM_RESOURCE_BYTES + 1]) }),
            Box::pin(async { Ok(Vec::with_capacity(MAX_CUSTOM_RESOURCE_BYTES + 1)) }),
            Box::pin(async {
                panic!("intentional producer panic");
                #[allow(unreachable_code)]
                Ok(vec![])
            }),
        ];
        for future in cases {
            let mut run = Box::pin(run_producer(future, ResourceCancellationToken::default()));
            assert!(matches!(poll(run.as_mut()), Poll::Ready(Some(Err(_)))));
        }
        let mut run = Box::pin(run_producer(
            Box::pin(async { Ok(vec![1, 2, 3]) }),
            ResourceCancellationToken::default(),
        ));
        assert_eq!(
            poll(run.as_mut()),
            Poll::Ready(Some(Ok(Arc::from([1, 2, 3]))))
        );
    }
    #[test]
    fn pending_producer_is_dropped_after_invalidation_without_publishing_bytes() {
        struct Guard(Rc<RefCell<usize>>);
        impl Drop for Guard {
            fn drop(&mut self) {
                *self.0.borrow_mut() += 1;
            }
        }
        let mut scene = gpui_flow_scene::Scene::new();
        scene
            .insert(
                gpui_flow_scene::DisplayObject::new(
                    "image",
                    gpui_flow_scene::display::Image::new("a", gpui_flow_scene::Size::new(1., 1.)),
                ),
                None,
                None,
            )
            .unwrap();
        let mut engine = SceneEngine::new(scene).unwrap();
        let mut resources = SceneResources::new(&mut engine).unwrap();
        resources.synchronize(&mut engine).unwrap();
        let ticket = resources
            .ticket(&engine, engine.handle(&"image".into()).unwrap())
            .unwrap();
        let token = resources.cancellation_token(&engine, ticket).unwrap();
        let drops = Rc::new(RefCell::new(0));
        let guard = Guard(drops.clone());
        let future = Box::pin(async move {
            let _guard = guard;
            std::future::pending::<()>().await;
            Ok(vec![9])
        });
        let mut run = Box::pin(run_producer(future, token));
        assert!(poll(run.as_mut()).is_pending());
        assert_eq!(*drops.borrow(), 0);
        resources.invalidate(&engine, ResourceKind::Image).unwrap();
        assert_eq!(poll(run.as_mut()), Poll::Ready(None));
        assert_eq!(*drops.borrow(), 1);
    }
}

#[cfg(test)]
mod pixel_bridge_tests {
    use super::*;
    use crate::display::{CustomRasterOutput, CustomRasterRequest, CustomRasterizer};
    use crate::display::{
        composite::{CompositeController, RequestKey},
        composite_custom::CustomRasterizers,
    };
    use gpui_flow_composite::{CompositeError, CompositeLimits, RasterViewport, capture_subtree};
    use gpui_flow_engine::{
        DirtyFlags, ElementDefinition, ElementRegistry, PropertyDescriptor, PropertyKey,
        PropertySchema,
    };
    use gpui_flow_scene::{Affine, BlendMode, Bounds, Point, Scene, Shape, Size, display::Custom};
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Definition;
    impl ElementDefinition for Definition {
        fn kind(&self) -> &str {
            "host"
        }
        fn schema(&self) -> PropertySchema {
            PropertySchema {
                allow_unknown: false,
                properties: vec![
                    PropertyDescriptor::number("width", 8., 1., 20., DirtyFlags::GEOMETRY),
                    PropertyDescriptor::number("value", 1., 0., 20., DirtyFlags::DATA),
                ],
            }
        }
        fn shape(&self, p: &serde_json::Value) -> Result<Shape, String> {
            Ok(Custom::new(
                "host",
                Bounds::new(
                    Point::ZERO,
                    Size::new(p["width"].as_f64().unwrap() as f32, 8.),
                ),
                vec![],
            )
            .into())
        }
        fn resource_key(&self, _: &serde_json::Value) -> Result<Option<String>, String> {
            Ok(Some("asset".into()))
        }
    }
    struct Pixels {
        calls: Arc<AtomicUsize>,
    }
    impl CustomRasterizer for Pixels {
        fn rasterize(
            &self,
            request: CustomRasterRequest<'_>,
        ) -> Result<Option<CustomRasterOutput>, String> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let resource = request.resource.expect("required accepted payload");
            assert_eq!(resource.bytes, &[7, 9]);
            assert!(!request.cancellation.is_cancelled());
            Ok(Some(CustomRasterOutput {
                width: 1,
                height: 1,
                premul_rgba: vec![255; 4],
                local_bounds: Bounds::new(Point::ZERO, Size::new(1., 1.)),
                pixel_work: 1,
            }))
        }
    }
    fn setup() -> (
        SceneEngine,
        SceneResourceAdapter,
        CustomRasterizers,
        Arc<AtomicUsize>,
    ) {
        let mut registry = ElementRegistry::default();
        registry.register(Definition).unwrap();
        let mut scene = Scene::new();
        let mut object = registry
            .create("host", "custom", serde_json::json!({}))
            .unwrap();
        object.style.blend_mode = BlendMode::Normal;
        scene.insert(object, None, None).unwrap();
        let mut engine = SceneEngine::with_registry(scene, registry).unwrap();
        let mut resources = SceneResourceAdapter::new(&mut engine).unwrap();
        resources.prepare_canvases(&mut engine).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let mut rasters = CustomRasterizers::default();
        rasters.set(
            "host".into(),
            Some(Arc::new(Pixels {
                calls: calls.clone(),
            })),
        );
        (engine, resources, rasters, calls)
    }
    fn key(
        engine: &SceneEngine,
        resources: &SceneResourceAdapter,
        rasters: &CustomRasterizers,
    ) -> Arc<RequestKey> {
        let bounds = Bounds::new(Point::ZERO, Size::new(20., 20.));
        Arc::new(
            RequestKey::new(
                vec![
                    capture_subtree(engine.scene(), &"custom".into(), CompositeLimits::default())
                        .unwrap(),
                ],
                engine,
                RasterViewport {
                    world_to_screen: Affine::IDENTITY,
                    canvas_bounds: bounds,
                    content_mask: bounds,
                    device_scale: 1.,
                },
                1,
            )
            .with_resources(engine, Some(resources))
            .with_custom_rasterizers(rasters),
        )
    }
    fn load(engine: &mut SceneEngine, resources: &mut SceneResourceAdapter) -> ResourceTicket {
        let ticket = resources
            .tracker
            .ticket(engine, engine.handle(&"custom".into()).unwrap())
            .unwrap();
        resources.custom.jobs.insert(
            ticket,
            Job {
                result: Rc::new(RefCell::new(Some(Ok(Arc::from([7, 9]))))),
                _task: gpui::Task::ready(()),
            },
        );
        resources
            .settle(engine, ticket, ResourceOutcome::Loaded, None, &mut vec![])
            .unwrap();
        ticket
    }
    #[test]
    fn custom_pixel_resource_pending_failed_and_loaded_use_real_adapter_ticket_and_bytes() {
        let (mut e, mut r, rasters, calls) = setup();
        let pending = key(&e, &r, &rasters);
        let mut c = CompositeController::default();
        c.request(pending.clone());
        let w = c.take_work().unwrap();
        let result = w.run();
        assert_eq!(
            result[0].1.as_ref().unwrap_err(),
            &CompositeError::CustomPending("custom".into())
        );
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        c.finish(pending.clone(), result);
        assert!(c.pending());
        load(&mut e, &mut r);
        let ready = key(&e, &r, &rasters);
        assert_ne!(pending, ready);
        assert!(!pending.presentation_current(
            &e,
            Some(&r),
            None,
            &rasters,
            pending.viewport,
            pending.window
        ));
        c.request(ready.clone());
        let w = c.take_work().unwrap();
        let result = w.run();
        assert!(result[0].1.as_ref().unwrap().is_some());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
        c.finish(ready.clone(), result);
        assert!(c.frame(&"custom".into()).is_some());
        r.set_custom_resource_provider(&e, "host".into(), None)
            .unwrap();
        assert!(!ready.inputs_current());
        assert!(c.frame(&"custom".into()).is_none());
        r.prepare_canvases(&mut e).unwrap();
        let ticket = r
            .tracker
            .ticket(&e, e.handle(&"custom".into()).unwrap())
            .unwrap();
        r.settle(
            &mut e,
            ticket,
            ResourceOutcome::Failed("denied".into()),
            None,
            &mut vec![],
        )
        .unwrap();
        let failed = key(&e, &r, &rasters);
        c.request(failed.clone());
        let w = c.take_work().unwrap();
        let result = w.run();
        assert!(
            matches!(&result[0].1,Err(CompositeError::CustomRasterization{reason,..}) if reason=="denied")
        );
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
    #[test]
    fn resource_data_geometry_detach_and_dispose_cancel_pixels_before_another_render() {
        for change in 0..5 {
            let (mut e, mut r, rasters, _) = setup();
            let ticket = load(&mut e, &mut r);
            let first = key(&e, &r, &rasters);
            let token = r.tracker.cancellation_token(&e, ticket).unwrap();
            let mut c = CompositeController::default();
            c.request(first.clone());
            let w = c.take_work().unwrap();
            let late = w.run();
            match change {
                0 | 1 => {
                    let h = e.handle(&"custom".into()).unwrap();
                    e.set_property(
                        h,
                        PropertyKey::Custom(if change == 0 { "value" } else { "width" }.into()),
                        serde_json::json!(9.),
                    )
                    .unwrap();
                }
                2 => {
                    e.transaction(|s| s.remove(&"custom".into()).map(|_| ()))
                        .unwrap();
                }
                3 => {
                    r.dispose(&mut e).unwrap();
                }
                _ => {
                    r.set_custom_resource_provider(&e, "host".into(), None)
                        .unwrap();
                }
            }
            assert!(token.is_cancelled());
            assert!(!first.inputs_current());
            c.finish(first.clone(), late);
            assert!(c.frame(&"custom".into()).is_none());
            assert!(c.presentation_uploads(&first).is_empty());
        }
    }
    #[test]
    fn custom_resource_a_b_a_never_reuses_old_uploaded_pixels_or_ticket() {
        let (mut e, mut r, rasters, _) = setup();
        let old_ticket = load(&mut e, &mut r);
        let first = key(&e, &r, &rasters);
        let mut c = CompositeController::default();
        c.request_coherent(first.clone(), false);
        let work = c.take_work().unwrap();
        c.finish(first.clone(), work.run());
        let uploads = c.presentation_uploads(&first);
        for (id, frame) in &uploads {
            c.presentation_uploaded(&first, id, &frame.image);
        }
        assert!(c.publish_presentation(&first).is_some());
        let original = e.scene().clone();
        let h = e.handle(&"custom".into()).unwrap();
        e.set_property(
            h,
            PropertyKey::Custom("value".into()),
            serde_json::json!(2.),
        )
        .unwrap();
        r.prepare_canvases(&mut e).unwrap();
        let middle = load(&mut e, &mut r);
        assert_ne!(old_ticket, middle);
        e.replace_scene(original).unwrap();
        r.prepare_canvases(&mut e).unwrap();
        let latest = load(&mut e, &mut r);
        assert_ne!(old_ticket, latest);
        assert_ne!(middle, latest);
        assert!(!first.inputs_current());
        assert!(c.frame(&"custom".into()).is_none());
        assert!(c.old_frame(&"custom".into()).is_none());
        assert!(c.presentation_frame(&first, &"custom".into()).is_none());
        let fresh = key(&e, &r, &rasters);
        assert!(!first.presentation_current(
            &e,
            Some(&r),
            None,
            &rasters,
            fresh.viewport,
            fresh.window
        ));
        c.request_coherent(fresh.clone(), false);
        for (id, frame) in uploads {
            c.presentation_uploaded(&first, &id, &frame.image);
            assert!(c.accepted(&first, &id, &frame.image).is_none());
        }
        assert!(c.publish_presentation(&fresh).is_none());
        let work = c.take_work().unwrap();
        let result = work.run();
        assert!(result[0].1.as_ref().unwrap().is_some());
        c.finish(fresh.clone(), result);
        assert!(c.publish_presentation(&fresh).is_none());
    }
}
