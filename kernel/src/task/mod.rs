use core::{
    future::Future,
    pin::Pin,
    sync::atomic::AtomicU64,
    task::{Context, Poll},
};

use alloc::boxed::Box;

mod executor;
pub use executor::run;
pub use executor::spawn;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(transparent)]
struct TaskId(u64);

impl TaskId {
    fn new() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);
        TaskId(NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
}

pub struct Task {
    id: TaskId,
    future: Pin<Box<dyn Future<Output = ()> + Send>>,
}

impl Task {
    pub fn new(future: impl Future<Output = ()> + Send + 'static) -> Self {
        Self {
            id: TaskId::new(),
            future: Box::pin(future),
        }
    }

    fn poll(&mut self, context: &mut Context<'_>) -> Poll<()> {
        self.future.as_mut().poll(context)
    }
}

impl<F: Future<Output = ()> + Send + 'static> From<F> for Task {
    fn from(value: F) -> Self {
        Self::new(value)
    }
}

#[macro_export]
macro_rules! loop_yield {
    ($($body:tt)*) => {
        loop {
            $($body)*
            $crate::util::r#async::yield_now().await;
        }
    };
}
