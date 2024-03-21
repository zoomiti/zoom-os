use core::{
    cell::UnsafeCell,
    fmt::Debug,
    ops::{Deref, DerefMut},
    pin::Pin,
    sync::atomic::{AtomicBool, Ordering},
    task::{Context, Poll},
};

use alloc::fmt;
use futures::Future;

use super::waker_list::{WakerList, WakerListHandle};

#[derive(Default)]
pub struct Mutex<T: ?Sized> {
    locked: AtomicBool,
    wakeup_list: WakerList,
    // HAS TO GO AT THE END
    inner: UnsafeCell<T>,
}

unsafe impl<T> Sync for Mutex<T> {}
unsafe impl<T> Send for Mutex<T> {}

impl<T> Mutex<T> {
    pub fn new(inner: T) -> Self {
        Self {
            inner: UnsafeCell::new(inner),
            locked: AtomicBool::new(false),
            wakeup_list: Default::default(),
        }
    }
}
impl<T: ?Sized> Mutex<T> {
    pub fn try_lock(&self) -> Option<MutexGuard<'_, T>> {
        let locked = self.locked.load(Ordering::Acquire);
        if locked {
            return None;
        }

        self.locked
            .compare_exchange_weak(false, true, Ordering::AcqRel, Ordering::Acquire)
            .ok()?;

        Some(MutexGuard {
            inner: unsafe { &mut *self.inner.get() },
            locked: &self.locked,
            waker_list: &self.wakeup_list,
        })
    }

    pub async fn lock(&self) -> MutexGuard<'_, T> {
        loop {
            MutexLocker {
                locked: &self.locked,
                wake_handle: self.wakeup_list.handle(),
            }
            .await;
            if let Some(guard) = self.try_lock() {
                return guard;
            }
        }
    }
}

impl<T: ?Sized + Debug> Debug for Mutex<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_struct("Mutex");
        match self.try_lock() {
            Some(guard) => d.field("data", &&*guard),
            None => d.field("data", &format_args!("<locked>")),
        };
        d.finish_non_exhaustive()
    }
}

pub struct MutexGuard<'t, T: ?Sized> {
    inner: &'t mut T,
    locked: &'t AtomicBool,
    waker_list: &'t WakerList,
}

unsafe impl<T: ?Sized + Send> Send for MutexGuard<'_, T> {}
unsafe impl<T: Sync> Sync for MutexGuard<'_, T> {}

impl<T: ?Sized + Debug> Debug for MutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("MutexGuard")
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

impl<T: ?Sized> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.inner
    }
}

impl<T: ?Sized> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner
    }
}

impl<T: ?Sized> Drop for MutexGuard<'_, T> {
    fn drop(&mut self) {
        self.locked.store(false, Ordering::Release);
        self.waker_list.notify_one();
    }
}

struct MutexLocker<'t> {
    locked: &'t AtomicBool,
    wake_handle: WakerListHandle<'t>,
}

impl Future for MutexLocker<'_> {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.locked.load(Ordering::Acquire) {
            self.wake_handle.register(cx.waker().clone());
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}
