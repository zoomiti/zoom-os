#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(kernel::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use core::{panic::PanicInfo};

use alloc::sync::Arc;
use bootloader_api::{entry_point, BootInfo};
use kernel::{
    keyboard::print_keypresses, println,
    qemu::exit_qemu,
    task::{run, spawn},
    util::r#async::{mutex::Mutex},
    BOOTLOADER_CONFIG,
};
use tracing::{debug, error, Level};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if tracing::event_enabled!(Level::ERROR) {
        error!("{}", info);
    } else {
        println!("{}", info);
    }
    exit_qemu(kernel::qemu::QemuExitCode::Failed);
    loop {}
}

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    if let Some(framebuffer) = boot_info.framebuffer.as_mut() {
        for byte in framebuffer.buffer_mut() {
            *byte = 0x90;
        }
    }
    kernel::init(boot_info);

    spawn(print_keypresses());
    spawn(async {
        println!("Asynchronously executed");
        debug!("The tracer should be setup by now");
    });

    let locked = Mutex::new(());
    let locked1 = Arc::new(locked);
    let locked2 = Arc::clone(&locked1);
    debug!(?locked1, ?locked2, "Setting up the async mutex demo");

    /*
    spawn(async move {
        loop {
            {
                let _guard = locked1.lock().await;
                print!(".");
            }
            //sleep(Duration::from_millis(150)).await;
            yield_now().await;
        }
    });

    spawn(async move {
        loop {
            {
                let _guard = locked2.lock().await;
                print!("!");
            }
            //sleep(Duration::from_millis(450)).await;
            yield_now().await;
        }
    });
    */

    #[cfg(test)]
    test_main();

    println!("Hello World{}", "!");

    run()
}
