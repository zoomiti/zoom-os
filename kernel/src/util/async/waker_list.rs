use core::task::Waker;

use crossbeam_queue::SegQueue;

#[derive(Debug, Default)]
pub struct WakerList {
    inner: SegQueue<Waker>,
}

impl WakerList {
    pub const fn new() -> Self {
        Self {
            inner: SegQueue::new(),
        }
    }

    pub fn notify_one(&self) {
        if let Some(waker) = self.inner.pop() {
            waker.wake_by_ref();
        }
    }

    pub fn register(&self, waker: Waker) {
        self.inner.push(waker);
    }
}
