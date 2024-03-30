use core::sync::atomic::AtomicU64;

use alloc::{
    borrow::Cow,
    collections::{BTreeMap, VecDeque},
    fmt, format,
};
use tracing::{field::Visit, info, span, subscriber::set_global_default, Subscriber};
use x86_64::instructions::interrupts;

use crate::{print, println, util::r#async::mutex::Mutex};

pub fn init() {
    set_global_default(SimpleLogger::default()).expect("Couldn't initialize logging");
    info!("Initialized logging");
}

pub struct SerialVisitor;

impl Visit for SerialVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        if field.name() == "message" {
            print!("{value:?} ");
        } else {
            print!("{} = {:?}, ", field.name(), value);
        }
    }
}

#[derive(Debug, Default)]
pub struct SimpleLogger {
    inner: Mutex<SimpleLoggerInner>,
}

#[derive(Debug, Default)]
pub struct SimpleLoggerInner {
    spans: BTreeMap<u64, &'static str>,
    stack: VecDeque<u64>,
}

impl Subscriber for SimpleLogger {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        static ID: AtomicU64 = AtomicU64::new(1);
        let old = ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);

        interrupts::without_interrupts(|| {
            let mut inner = self.inner.spin_lock();
            inner.spans.insert(old, _span.metadata().name());
            span::Id::from_u64(old)
        })
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();

        let level = metadata.level();
        let target = metadata.target();

        let stack = interrupts::without_interrupts(|| {
            let inner = self.inner.spin_lock();
            let mut stack_iter = inner.stack.iter();
            let start = stack_iter.next();

            if let Some(start) = start {
                let start_str = format!(" {}", inner.spans[start]);
                let mut ret = stack_iter.fold(start_str, |mut s, n| {
                    s.push_str("::");
                    s.push_str(inner.spans[n]);
                    s
                });
                ret.push_str(": ");
                ret.into()
            } else {
                Cow::from(": ")
            }
        });

        print!("[{level}] {target}{stack}");
        event.record(&mut SerialVisitor);
        println!();
    }

    fn enter(&self, span: &span::Id) {
        interrupts::without_interrupts(|| {
            let mut inner = self.inner.spin_lock();
            inner.stack.push_back(span.into_non_zero_u64().into());
        });
    }

    fn exit(&self, _span: &span::Id) {
        interrupts::without_interrupts(|| {
            let mut inner = self.inner.spin_lock();
            inner.stack.pop_back();
        });
    }
}
