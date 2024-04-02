use core::sync::atomic::{AtomicBool, AtomicU64};

use alloc::{
    borrow::Cow,
    collections::{BTreeMap, VecDeque},
    fmt, format,
};
use tracing::{field::Visit, info, span, subscriber::set_global_default, Subscriber};

use crate::{print, println, util::r#async::mutex::IntMutex, vga_print, vga_println};

pub fn init() {
    set_global_default(SimpleLogger::default()).expect("Couldn't initialize logging");
    info!("Initialized logging");
}

pub static SHOULD_USE_SCREEN: AtomicBool = AtomicBool::new(true);

pub struct SerialVisitor;

impl Visit for SerialVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn fmt::Debug) {
        let screen = SHOULD_USE_SCREEN.load(core::sync::atomic::Ordering::Relaxed);
        if field.name() == "message" {
            if screen {
                vga_print!("{value:?} ");
            }
            print!("{value:?} ");
        } else {
            if screen {
                vga_print!("{} = {:?}, ", field.name(), value);
            }
            print!("{} = {:?}, ", field.name(), value);
        }
    }
}

#[derive(Debug, Default)]
pub struct SimpleLogger {
    inner: IntMutex<SimpleLoggerInner>,
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

        //let mut inner = self.inner.spin_lock();
        //inner.spans.insert(old, _span.metadata().name());
        span::Id::from_u64(old)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();

        let level = metadata.level();
        let target = metadata.target();

        let stack = {
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
        };

        print!("[{level}] {target}{stack}");
        let screen = SHOULD_USE_SCREEN.load(core::sync::atomic::Ordering::Relaxed);
        if screen {
            vga_print!("[{level}] {target}{stack}");
        }
        event.record(&mut SerialVisitor);
        println!();
        if screen {
            vga_println!();
        }
    }

    fn enter(&self, span: &span::Id) {
        /*
                let mut inner = self.inner.spin_lock();
                inner.stack.push_back(span.into_non_zero_u64().into());
        */
    }

    fn exit(&self, _span: &span::Id) {
        /*
        let mut inner = self.inner.spin_lock();
        inner.stack.pop_back();
        */
    }
}
