use core::{
    cell::UnsafeCell,
    mem::MaybeUninit,
    sync::atomic::{AtomicBool, Ordering},
};

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
                self.try_init_inner(&mut || func.take().unwrap()());
                self.state.store(true, Ordering::Release);
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
}
