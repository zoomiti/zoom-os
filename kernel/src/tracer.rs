use core::sync::atomic::{AtomicBool, AtomicU64};

use alloc::{collections::BTreeMap, fmt, vec::Vec};
use tracing::{field::Visit, info, span, subscriber::set_global_default, Metadata, Subscriber};
use tracing_core::span::Current;

use crate::{print, println, util::r#async::mutex::Mutex, vga_print, vga_println};

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
    inner: Mutex<SimpleLoggerInner>,
}

#[derive(Debug, Default)]
pub struct SimpleLoggerInner {
    spans: BTreeMap<u64, (usize, &'static Metadata<'static>)>,
    stack: Vec<u64>,
}

impl Subscriber for SimpleLogger {
    fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
        true
    }

    fn new_span(&self, _span: &span::Attributes<'_>) -> span::Id {
        x86_64::instructions::interrupts::without_interrupts(|| {
            static ID: AtomicU64 = AtomicU64::new(1);
            let mut inner = self.inner.spin_lock();
            for (id, (count, span)) in inner.spans.iter_mut() {
                if _span.metadata() == *span {
                    *count += 1;
                    return span::Id::from_u64(*id);
                }
            }
            let old = ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            inner.spans.insert(old, (1, _span.metadata()));
            span::Id::from_u64(old)
        })
    }

    fn record(&self, _span: &span::Id, _values: &span::Record<'_>) {}

    fn record_follows_from(&self, _span: &span::Id, _follows: &span::Id) {}

    fn event(&self, event: &tracing::Event<'_>) {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let metadata = event.metadata();

            let level = metadata.level();
            let target = metadata.target();
            let screen = SHOULD_USE_SCREEN.load(core::sync::atomic::Ordering::Relaxed);

            print!("[{level}] ");
            if screen {
                vga_print!("[{level}] ");
            }
            if let Some(inner) = self.inner.try_lock() {
                let mut stack_iter = inner.stack.iter();
                let start = stack_iter.next();

                if let Some(start) = start {
                    print!("{}", inner.spans[start].1.name());
                    if screen {
                        vga_print!("{}", inner.spans[start].1.name());
                    }
                    for n in stack_iter {
                        print!("::{}", inner.spans[n].1.name());
                        if screen {
                            vga_print!("::{}", inner.spans[n].1.name());
                        }
                    }
                    print!(": ");
                    if screen {
                        vga_print!(": ");
                    }
                }
            };

            print!("{target}: ");
            if screen {
                vga_print!("{target}: ");
            }
            event.record(&mut SerialVisitor);
            println!();
            if screen {
                vga_println!();
            }
        })
    }

    fn enter(&self, span: &span::Id) {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut inner = self.inner.spin_lock();
            inner.stack.push(span.into_non_zero_u64().into());
        })
    }

    fn exit(&self, _span: &span::Id) {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut inner = self.inner.spin_lock();
            // FIXME: this technically assumes that all spans are entered and exited in heirarchical
            // order
            inner.stack.pop();
        })
    }

    fn current_span(&self) -> Current {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let inner = self.inner.spin_lock();
            match inner.stack.last() {
                Some(id) => Current::new(span::Id::from_u64(*id), inner.spans[id].1),
                None => Current::none(),
            }
        })
    }
}
