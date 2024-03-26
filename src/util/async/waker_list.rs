use core::task::Waker;

use alloc::collections::BTreeMap;
use spin::Mutex;

#[derive(Debug, Default)]
pub struct WakerList {
    inner: Mutex<WakerListInner>,
}

#[derive(Debug, Default)]
pub struct WakerListInner {
    id: u64,
    wakers: BTreeMap<u64, Waker>,
}

impl WakerList {
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(WakerListInner {
                id: 0,
                wakers: BTreeMap::new(),
            }),
        }
    }

    pub fn notify_one(&self) {
        let inner = self.inner.lock();
        if let Some((_, waker)) = inner.wakers.iter().next() {
            waker.wake_by_ref();
        }
    }

    pub fn handle(&self) -> WakerListHandle<'_> {
        let mut inner = self.inner.lock();
        let id = inner.id;
        (inner.id, _) = inner.id.overflowing_add(1);
        WakerListHandle {
            id,
            inner: &self.inner,
        }
    }
}

pub struct WakerListHandle<'a> {
    id: u64,
    inner: &'a Mutex<WakerListInner>,
}

impl WakerListHandle<'_> {
    pub fn register(&mut self, waker: Waker) {
        let mut inner = self.inner.lock();
        inner.wakers.insert(self.id, waker);
    }
}

impl Drop for WakerListHandle<'_> {
    fn drop(&mut self) {
        let mut inner = self.inner.lock();
        inner.wakers.remove(&self.id);
    }
}
