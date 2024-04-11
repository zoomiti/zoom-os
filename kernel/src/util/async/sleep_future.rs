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
use tracing::instrument;

use crate::{ rtc::TIMER_FREQ};

use super::mutex::Mutex;

pub static MONOTONIC_TIME: AtomicUsize = AtomicUsize::new(0);

// TODO: Fix overflow issue
pub static WAKEUP_SERVICE: Mutex<BTreeMap<Reverse<usize>, SmallVec<[Waker; 5]>>> =
    Mutex::new(BTreeMap::new());

struct SleepFuture {
    end_tick: usize,
    registered: bool,
}

#[instrument]
pub async fn sleep(dur: Duration) {
    SleepFuture::new(dur).await
}

#[instrument]
fn register_sleep(tick: usize, waker: Waker) {
    x86_64::instructions::interrupts::without_interrupts(|| {
        let mut service = WAKEUP_SERVICE.spin_lock();
        let requested = service.entry(Reverse(tick)).or_default();
        requested.push(waker);
    })
}

#[instrument]
pub fn wake_sleep(tick: usize) {
    let mut service = WAKEUP_SERVICE
        .try_lock()
        .expect("Lock should not be held during interrupt");

    if let Some ((time,_ )) = service.first_key_value() && time.0 > tick {
        // Early return if we don't need to wakeup
        return;
    }

    let done = service.split_off(&Reverse(tick));

    for (_, wakers) in done {
        for waker in wakers {
            waker.wake_by_ref();
        }
    }
}

impl SleepFuture {
    pub fn new(dur: Duration) -> Self {
        let ticks = dur.as_secs_f64() * TIMER_FREQ as f64;
        // have to subtract one because monotonic is 1 num behind
        let ticks = ticks as usize -1;
        let start = MONOTONIC_TIME.load(Ordering::Acquire);
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
        let mn_time = MONOTONIC_TIME.load(Ordering::Acquire);
        if mn_time >= self.end_tick {
            Poll::Ready(())
        } else {
            if !self.registered {
                register_sleep(self.end_tick, cx.waker().clone());
                self.registered = true;
            }
            Poll::Pending
        }
    }
}
