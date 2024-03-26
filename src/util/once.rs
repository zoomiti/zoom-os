use core::{
    borrow::Borrow,
    cell::UnsafeCell,
    fmt::{self, Display},
    mem::{ManuallyDrop, MaybeUninit},
    ops::Deref,
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};

use alloc::fmt;

use super::r#async::mutex::Mutex;

pub struct OnceLock<T> {
    state: AtomicBool,
    inner: UnsafeCell<MaybeUninit<T>>,
    mutex: Mutex<()>,
}

unsafe impl<T> Send for OnceLock<T> where T: Send {}
unsafe impl<T> Sync for OnceLock<T> where T: Send + Sync {}

#[derive(Debug)]
pub enum TryGetError {
    Uninitialized,
}

#[derive(Debug)]
pub enum TryInitError {
    AlreadyInitialized,
}

impl<T> OnceLock<T> {
    pub const fn uninit() -> Self {
        Self {
            state: AtomicBool::new(false),
            inner: UnsafeCell::new(MaybeUninit::uninit()),
            mutex: Mutex::new(()),
        }
    }

    pub fn is_init(&self) -> bool {
        self.state.load(Ordering::Acquire)
    }

    pub fn try_get(&self) -> Result<&T, TryGetError> {
        match self.state.load(Ordering::Acquire) {
            true => Ok(unsafe { self.get_unchecked() }),
            false => Err(TryGetError::Uninitialized),
        }
    }

    pub fn try_init_once(&self, func: impl FnOnce() -> T) -> Result<(), TryInitError> {
        match self.state.load(Ordering::Acquire) {
            true => Err(TryInitError::AlreadyInitialized),
            false => {
                let mut func = Some(func);
                self.state.store(true, Ordering::Release);
                self.try_init_inner(&mut || func.take().unwrap()());
                Ok(())
            }
        }
    }

    #[inline(never)]
    #[cold]
    fn try_init_inner(&self, func: &mut dyn FnMut() -> T) -> &T {
        let guard = self.mutex.spin_lock();
        unsafe {
            let inner = &mut *self.inner.get();
            inner.as_mut_ptr().write(func());
        }
        drop(guard);
        unsafe { self.get_unchecked() }
    }

    /// # Safety
    /// Only safe once initialized
    pub unsafe fn get_unchecked(&self) -> &T {
        let inner = &*self.inner.get();
        &*inner.as_ptr()
    }

    pub fn get_or_init(&self, func: impl FnOnce() -> T) -> &T {
        match self.try_get() {
            Ok(res) => res,
            Err(_) => {
                let mut func = Some(func);
                self.state.store(true, Ordering::Release);
                self.try_init_inner(&mut || func.take().unwrap()())
            }
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for OnceLock<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("OnceLock")
            .field("inner", &self.try_get().ok())
            .finish()
    }
}

pub struct Lazy<T, F = fn() -> T> {
    cell: OnceLock<T>,
    init: ManuallyDrop<F>,
}

impl<T, F> Lazy<T, F> {
    #[inline]
    pub const fn new(init: F) -> Self {
        Self {
            cell: OnceLock::uninit(),
            init: ManuallyDrop::new(init),
        }
    }

    pub fn is_init(&self) -> bool {
        self.cell.is_init()
    }
}

impl<T, F> Lazy<T, F>
where
    F: FnOnce() -> T,
{
    #[inline]
    pub fn get_or_init(&self) -> &T {
        self.cell.get_or_init(|| {
            // SAFETY: this (outer) closure is only called once and `init` is
            // never dropped, so it will never be touched again
            let func = unsafe { ptr::read(&*self.init) };
            func()
        })
    }
}

impl<T, F> AsRef<T> for Lazy<T, F>
where
    F: FnOnce() -> T,
{
    fn as_ref(&self) -> &T {
        Lazy::get_or_init(self)
    }
}

impl<T, F> Borrow<T> for Lazy<T, F>
where
    F: FnOnce() -> T,
{
    fn borrow(&self) -> &T {
        Lazy::get_or_init(self)
    }
}

impl<T, F> Deref for Lazy<T, F>
where
    F: FnOnce() -> T,
{
    type Target = T;

    fn deref(&self) -> &Self::Target {
        Lazy::get_or_init(self)
    }
}

impl<T: fmt::Debug, F> fmt::Debug for Lazy<T, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(&self.cell, f)
    }
}

impl<T: fmt::Display, F: FnOnce() -> T> fmt::Display for Lazy<T, F> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(Self::get_or_init(self), f)
    }
}
