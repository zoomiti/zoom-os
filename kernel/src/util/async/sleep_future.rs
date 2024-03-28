use core::{
    cmp::Reverse,
    pin::Pin,
    sync::atomic::{AtomicUsize, Ordering},
    task::{Context, Poll, Waker},
    time::Duration,
    usize,
};

use alloc::collections::BTreeMap;
use futures::Future;
use smallvec::SmallVec;
use x86_64::instructions::interrupts;

use crate::task::spawn;

use super::mutex::Mutex;

// Actually closer to 54.9254 ms
pub const TIMER_FREQ: Duration = Duration::from_micros(976);
pub static MONOTONIC_TIME: AtomicUsize = AtomicUsize::new(0);

// TODO: Fix overflow issue
static WAKEUP_SERVICE: Mutex<BTreeMap<Reverse<usize>, SmallVec<[Waker; 1]>>> =
    Mutex::new(BTreeMap::new());

struct SleepFuture {
    end_tick: usize,
    registered: bool,
}

pub async fn sleep(dur: Duration) {
    SleepFuture::new(dur).await
}

async fn register_sleep(tick: usize, waker: Waker) {
    interrupts::disable();
    let mut service = WAKEUP_SERVICE.lock().await;
    let requested = service.entry(Reverse(tick)).or_default();
    requested.push(waker);
    drop(service);
    interrupts::enable();
}

pub(crate) fn wake_sleep(tick: usize) {
    let mut service = WAKEUP_SERVICE
        .try_lock()
        .expect("Lock should not be held during interrupt");
    let done = service.split_off(&Reverse(tick));

    for (_, wakers) in done {
        for waker in wakers {
            waker.wake();
        }
    }
}

impl SleepFuture {
    pub fn new(dur: Duration) -> Self {
        let ticks = dur.as_secs_f64() / TIMER_FREQ.as_secs_f64();
        let ticks = ticks as usize;
        let start = MONOTONIC_TIME.load(Ordering::Relaxed);
        let end_tick = start.wrapping_add(ticks);
        Self {
            end_tick,
            registered: false,
        }
    }
}

impl Future for SleepFuture {
    type Output = ();

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mn_time = MONOTONIC_TIME.load(Ordering::Relaxed);
        if mn_time >= self.end_tick {
            Poll::Ready(())
        } else {
            if !self.registered {
                let register_fut = register_sleep(self.end_tick, cx.waker().clone());
                spawn(register_fut);
                self.registered = true;
            }
            Poll::Pending
        }
    }
}
