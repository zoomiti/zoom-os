use core::{
    borrow::Borrow,
    cell::UnsafeCell,
    fmt,
    mem::{ManuallyDrop, MaybeUninit},
    ops::Deref,
    ptr,
    sync::atomic::{AtomicU8, Ordering},
};

pub struct OnceLock<T> {
    inner: UnsafeCell<MaybeUninit<T>>,
    status: AtomicU8,
}

const UNINIT: u8 = 0;
const RUNNING: u8 = 1;
const INIT: u8 = 2;

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
    pub const fn new() -> Self {
        Self {
            inner: UnsafeCell::new(MaybeUninit::uninit()),
            status: AtomicU8::new(UNINIT),
        }
    }

    pub const fn with_value(val: T) -> Self {
        Self {
            inner: UnsafeCell::new(MaybeUninit::new(val)),
            status: AtomicU8::new(INIT),
        }
    }

    #[inline(always)]
    pub fn is_init(&self) -> bool {
        self.status.load(Ordering::Acquire) == INIT
    }

    pub fn get(&self) -> Option<&T> {
        if self.is_init() {
            Some(unsafe { self.get_unchecked() })
        } else {
            None
        }
    }

    pub fn init_once(&self, func: impl FnOnce() -> T) {
        if !self.is_init() {
            let mut func = Some(func);
            // # Safety
            // the inner function is only called once
            self.try_init_inner(&mut || unsafe { func.take().unwrap_unchecked() }());
        }
    }

    pub fn try_get(&self) -> Result<&T, TryGetError> {
        match self.is_init() {
            true => Ok(unsafe { self.get_unchecked() }),
            false => Err(TryGetError::Uninitialized),
        }
    }

    pub fn try_init_once(&self, func: impl FnOnce() -> T) -> Result<(), TryInitError> {
        match self.is_init() {
            true => Err(TryInitError::AlreadyInitialized),
            false => {
                let mut func = Some(func);
                // # Safety
                // the inner function is only called once
                self.try_init_inner(&mut || unsafe { func.take().unwrap_unchecked() }());
                Ok(())
            }
        }
    }

    #[inline(never)]
    #[cold]
    fn try_init_inner(&self, func: &mut dyn FnMut() -> T) {
        loop {
            let exchange = self.status.compare_exchange_weak(
                UNINIT,
                RUNNING,
                Ordering::Acquire,
                Ordering::Acquire,
            );
            match exchange {
                Ok(_) => {
                    unsafe {
                        let inner = &mut *self.inner.get();
                        inner.as_mut_ptr().write(func());
                    }
                    self.status.store(INIT, Ordering::Release);
                    return;
                }
                Err(INIT) => return,
                Err(RUNNING) => core::hint::spin_loop(),
                Err(UNINIT) => (),
                Err(_) => debug_assert!(false),
            }
        }
    }

    /// # Safety
    /// Only safe once initialized
    pub unsafe fn get_unchecked(&self) -> &T {
        debug_assert!(self.is_init());
        let inner = &*self.inner.get();
        &*inner.as_ptr()
    }

    pub fn get_or_init(&self, func: impl FnOnce() -> T) -> &T {
        match self.try_get() {
            Ok(res) => res,
            Err(_) => {
                let mut func = Some(func);
                // # Safety
                // the inner function is only called once
                self.try_init_inner(&mut || unsafe { func.take().unwrap_unchecked() }());
                // # Safety
                // we just init
                unsafe { self.get_unchecked() }
            }
        }
    }
}

impl<T> Default for OnceLock<T> {
    fn default() -> Self {
        Self::new()
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
            cell: OnceLock::new(),
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

#[cfg(test)]
mod test {

    use super::{Lazy, OnceLock};

    #[test_case]
    fn get_init_once() {
        let once = OnceLock::new();
        assert_eq!(once.get(), None);
        let res = once.try_init_once(|| 4);
        assert!(res.is_ok());
        assert_eq!(once.get(), Some(&4));
        let res = once.try_init_once(|| 5);
        assert!(res.is_err());
        assert_eq!(once.get(), Some(&4));
    }

    #[test_case]
    fn with_value() {
        let once = OnceLock::with_value(5);
        assert_eq!(once.get(), Some(&5));
        let res = once.try_init_once(|| 4);
        assert!(res.is_err());
        assert_eq!(once.get(), Some(&5));
    }

    #[test_case]
    fn test_lazy() {
        let lazy = Lazy::new(|| 6);
        assert!(!lazy.is_init());
        assert_eq!(*lazy, 6);
        assert!(lazy.is_init());
    }
}
