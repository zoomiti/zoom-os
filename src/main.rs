#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(zoom_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use core::panic::PanicInfo;

use alloc::sync::Arc;
use bootloader::{entry_point, BootInfo};
use tracing::debug;
use zoom_os::{
    keyboard::print_keypresses,
    println,
    task::executor::Executor,
    util::r#async::{mutex::Mutex, yield_now},
    vga_print, vga_println,
};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    zoom_os::init(boot_info);

    let mut executor = Executor::new();
    executor.spawn(print_keypresses());
    executor.spawn(async {
        vga_println!("Asynchronously executed");
        debug!("The tracer is setup");
    });

    let locked = Mutex::new(());
    let locked1 = Arc::new(locked);
    let locked2 = Arc::clone(&locked1);
    debug!(?locked1, ?locked2, "Setting up the async mutex demo");

    executor.spawn(async move {
        loop {
            {
                let _guard = locked1.lock().await;
                vga_print!(".");
            }
            yield_now().await;
            for _ in 0..100000 {
                let _ = 10 + 10;
            }
        }
    });

    executor.spawn(async move {
        loop {
            {
                let _guard = locked2.lock().await;
                vga_print!("!");
            }
            yield_now().await;
            for _ in 0..100000 {
                let _ = 10 + 10;
            }
        }
    });

    #[cfg(test)]
    test_main();

    vga_println!("Hello World{}", "!");

    executor.run()
}
