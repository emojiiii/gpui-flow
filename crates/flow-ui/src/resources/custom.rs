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
