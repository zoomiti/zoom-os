use core::sync::atomic::AtomicU64;

use alloc::fmt;
use tracing::{field::Visit, info, span, subscriber::set_global_default, Subscriber};

use crate::{print, println};

pub fn init() {
    set_global_default(SimpleLogger).expect("Couldn't initialize logging");
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

pub struct SimpleLogger;

impl Subscriber for SimpleLogger {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        static ID: AtomicU64 = AtomicU64::new(1);
        let old = ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        span::Id::from_u64(old)
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        let metadata = event.metadata();

        let level = metadata.level();
        let target = metadata.target();

        print!("{level} {target}: ");
        event.record(&mut SerialVisitor);
        println!();
    }

    fn enter(&self, _span: &span::Id) {}

    fn exit(&self, _span: &span::Id) {}
}
