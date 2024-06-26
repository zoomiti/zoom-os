use core::task::Poll;

use futures::Future;

pub mod mutex;
pub mod sleep_future;
/// Implements a waker for waking multiple tasks
pub mod waker_list;

pub use sleep_future::sleep;

pub async fn yield_now() {
    struct YieldNow {
        yielded: bool,
    }

    impl Future for YieldNow {
        type Output = ();

        fn poll(
            mut self: core::pin::Pin<&mut Self>,
            cx: &mut core::task::Context<'_>,
        ) -> core::task::Poll<Self::Output> {
            if self.yielded {
                return Poll::Ready(());
            }

            self.yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    }
    YieldNow { yielded: false }.await;
}
