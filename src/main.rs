#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(zoom_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use core::{panic::PanicInfo, time::Duration};

use alloc::sync::Arc;
use bootloader::{entry_point, BootInfo};
use tracing::{debug, error};
use zoom_os::{
    keyboard::print_keypresses,
    task::{run, spawn},
    util::r#async::{mutex::Mutex, sleep},
    vga_print, vga_println,
};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    error!("{}", info);
    loop {}
}

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    zoom_os::init(boot_info);

    spawn(print_keypresses());
    spawn(async {
        vga_println!("Asynchronously executed");
        debug!("The tracer should be setup by now");
    });

    let locked = Mutex::new(());
    let locked1 = Arc::new(locked);
    let locked2 = Arc::clone(&locked1);
    debug!(?locked1, ?locked2, "Setting up the async mutex demo");

    spawn(async move {
        loop {
            {
                let _guard = locked1.lock().await;
                vga_print!(".");
            }
            sleep(Duration::from_millis(150)).await;
        }
    });

    spawn(async move {
        loop {
            {
                let _guard = locked2.lock().await;
                vga_print!("!");
            }
            sleep(Duration::from_millis(450)).await;
        }
    });

    #[cfg(test)]
    test_main();

    vga_println!("Hello World{}", "!");

    run()
}
