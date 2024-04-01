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
use tracing::trace;
use x86_64::instructions::interrupts;

use super::waker_list::WakerList;

#[derive(Default)]
pub struct Mutex<T: ?Sized> {
    locked: AtomicBool,
    wakeup_list: WakerList,
    // HAS TO GO AT THE END
    inner: UnsafeCell<T>,
}

unsafe impl<T: ?Sized> Sync for Mutex<T> {}
unsafe impl<T: ?Sized> Send for Mutex<T> {}

/// A mutex implementation that supports sync and async
impl<T> Mutex<T> {
    pub const fn new(inner: T) -> Self {
        Self {
            inner: UnsafeCell::new(inner),
            locked: AtomicBool::new(false),
            wakeup_list: WakerList::new(),
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
                waker_list: &self.wakeup_list,
            }
            .await;
            if let Some(guard) = self.try_lock() {
                return guard;
            }
        }
    }

    pub fn spin_lock(&self) -> MutexGuard<'_, T> {
        let mut first = true;
        loop {
            if let Some(lock) = self.try_lock() {
                return lock;
            }
            if first {
                first = false;
                trace!("spinning");
            }
            core::hint::spin_loop();
        }
    }

    /// Force unlock of this [`Mutex<T>`].
    ///
    /// Will not notify an async waiters
    ///
    /// # Safety
    ///
    /// This is inherently unsafe for the reason any force unlocking of Mutex would be
    pub unsafe fn force_unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
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

impl<T: ?Sized> AsRef<T> for MutexGuard<'_, T> {
    fn as_ref(&self) -> &T {
        self.inner
    }
}

impl<T: ?Sized> AsMut<T> for MutexGuard<'_, T> {
    fn as_mut(&mut self) -> &mut T {
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
    waker_list: &'t WakerList,
}

impl Future for MutexLocker<'_> {
    type Output = ();
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.locked.load(Ordering::Acquire) {
            self.waker_list.register(cx.waker().clone());
            Poll::Pending
        } else {
            Poll::Ready(())
        }
    }
}

#[derive(Default)]
pub struct IntMutex<T: ?Sized>(Mutex<T>);

impl<T> IntMutex<T> {
    pub const fn new(inner: T) -> Self {
        Self(Mutex::new(inner))
    }
}
impl<T: ?Sized> IntMutex<T> {
    pub fn try_lock(&self) -> Option<IntMutexGuard<'_, T>> {
        let enabled = interrupts::are_enabled();
        if enabled {
            interrupts::disable()
        }
        let ret = self
            .0
            .try_lock()
            .map(|mg| IntMutexGuard(mg, interrupts::are_enabled()));
        if ret.is_none() && enabled {
            interrupts::enable()
        }

        ret
    }

    pub async fn lock(&self) -> IntMutexGuard<'_, T> {
        loop {
            MutexLocker {
                locked: &self.0.locked,
                waker_list: &self.0.wakeup_list,
            }
            .await;
            if let Some(guard) = self.try_lock() {
                return guard;
            }
        }
    }

    pub fn spin_lock(&self) -> IntMutexGuard<'_, T> {
        let mut first = true;
        loop {
            if let Some(lock) = self.try_lock() {
                return lock;
            }
            if first {
                first = false;
                trace!("spinning");
            }
            core::hint::spin_loop();
        }
    }

    /// Force unlock of this [`IntMutex<T>`].
    ///
    /// Will not notify an async waiters. Won't reenable int
    ///
    /// # Safety
    ///
    /// This is inherently unsafe for the reason any force unlocking of Mutex would be
    pub unsafe fn force_unlock(&self) {
        self.0.force_unlock()
    }

    pub fn get_mut(&mut self) -> &mut T {
        self.0.get_mut()
    }
}

/// Mutex wrapper that also disable interrupts
impl<T: ?Sized + Debug> Debug for IntMutex<T> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        let mut d = f.debug_struct("IntMutex");
        match self.try_lock() {
            Some(guard) => d.field("data", &&*guard),
            None => d.field("data", &format_args!("<locked>")),
        };
        d.finish_non_exhaustive()
    }
}

pub struct IntMutexGuard<'t, T: ?Sized>(MutexGuard<'t, T>, bool);
unsafe impl<T: ?Sized + Send> Send for IntMutexGuard<'_, T> {}
unsafe impl<T: Sync> Sync for IntMutexGuard<'_, T> {}

impl<T: ?Sized + Debug> Debug for IntMutexGuard<'_, T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("IntMutexGuard")
            .field("inner", &self.0.inner)
            .finish_non_exhaustive()
    }
}

impl<T: ?Sized> Deref for IntMutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        self.0.inner
    }
}

impl<T: ?Sized> DerefMut for IntMutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.inner
    }
}

impl<T: ?Sized> AsRef<T> for IntMutexGuard<'_, T> {
    fn as_ref(&self) -> &T {
        self.0.inner
    }
}

impl<T: ?Sized> AsMut<T> for IntMutexGuard<'_, T> {
    fn as_mut(&mut self) -> &mut T {
        self.0.inner
    }
}

impl<T: ?Sized> Drop for IntMutexGuard<'_, T> {
    fn drop(&mut self) {
        if self.1 {
            interrupts::enable();
        }
    }
}
