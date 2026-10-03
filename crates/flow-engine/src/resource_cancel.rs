use std::{
    future::Future,
    pin::Pin,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context, Poll, Waker},
};
#[derive(Default)]
struct State {
    cancelled: AtomicBool,
    waiters: Mutex<Vec<Weak<Waiter>>>,
}
#[derive(Default)]
struct Waiter(Mutex<Option<Waker>>);
/// Runtime lifetime signal shared with a custom producer. Cancellation wakes
/// the producer even without another render; it grants no engine mutation.
#[derive(Clone, Default)]
pub struct ResourceCancellationToken(Arc<State>);
impl ResourceCancellationToken {
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Acquire)
    }
    pub fn cancelled(&self) -> impl Future<Output = ()> + Send + use<> {
        Cancelled {
            token: self.clone(),
            waiter: Arc::default(),
            registered: false,
        }
    }
    pub(crate) fn cancel(&self) {
        if !self.0.cancelled.swap(true, Ordering::AcqRel) {
            let waiters = std::mem::take(&mut *self.0.waiters.lock().expect("cancellation lock"));
            for waiter in waiters {
                let waker = waiter
                    .upgrade()
                    .and_then(|w| w.0.lock().expect("waiter lock").take());
                if let Some(waker) = waker {
                    waker.wake();
                }
            }
        }
    }
}
struct Cancelled {
    token: ResourceCancellationToken,
    waiter: Arc<Waiter>,
    registered: bool,
}
impl Future for Cancelled {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.token.is_cancelled() {
            return Poll::Ready(());
        }
        {
            let mut waiters = self.token.0.waiters.lock().expect("cancellation lock");
            if self.token.is_cancelled() {
                return Poll::Ready(());
            }
            *self.waiter.0.lock().expect("waiter lock") = Some(cx.waker().clone());
            if !self.registered {
                waiters.push(Arc::downgrade(&self.waiter));
            }
        }
        self.registered = true;
        Poll::Pending
    }
}
impl Drop for Cancelled {
    fn drop(&mut self) {
        if self.registered {
            self.token
                .0
                .waiters
                .lock()
                .expect("cancellation lock")
                .retain(|w| !w.ptr_eq(&Arc::downgrade(&self.waiter)));
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dropped_waiters_release_registration_and_wakers() {
        let token = ResourceCancellationToken::default();
        for _ in 0..100 {
            let mut future = Box::pin(token.cancelled());
            let mut cx = Context::from_waker(Waker::noop());
            assert!(future.as_mut().poll(&mut cx).is_pending());
            assert_eq!(token.0.waiters.lock().unwrap().len(), 1);
            drop(future);
            assert!(token.0.waiters.lock().unwrap().is_empty());
        }
    }
}
