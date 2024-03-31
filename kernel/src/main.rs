#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(never_type)]
#![test_runner(kernel::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use core::panic::PanicInfo;

use bootloader_api::{entry_point, BootInfo};
use kernel::{
    display,
    keyboard::print_keypresses,
    println,
    qemu::exit_qemu,
    rtc,
    task::{run, spawn},
    BOOTLOADER_CONFIG,
};
use tracing::{error, info, span, Level};

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
    kernel::init(boot_info);
    let main_span = span!(Level::TRACE, "kernel_main");
    let _span = main_span.enter();

    let utc_date = rtc::read_date_time();
    info!(%utc_date);

    spawn(print_keypresses());

    spawn(display::clock::draw_clock());

    #[cfg(test)]
    test_main();

    println!("Hello World{}", "!");

    run()
}
