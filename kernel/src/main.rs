#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(never_type)]
#![test_runner(kernel::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use core::{panic::PanicInfo, time::Duration};

use alloc::string::ToString;
use bootloader_api::{entry_point, BootInfo};
use embedded_graphics::{
    mono_font::{ascii::FONT_9X15, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    text::{Baseline, Text},
};
use kernel::{
    framebuffer::DISPLAY,
    keyboard::print_keypresses,
    println,
    qemu::exit_qemu,
    rtc::RTC,
    task::{run, spawn},
    util::r#async::sleep,
    vga_println, BOOTLOADER_CONFIG,
};
use tracing::{error, info, span, Level};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    if tracing::event_enabled!(Level::ERROR) {
        error!("{}", info);
    } else {
        println!("{}", info);
    }
    if let Ok(disp) = DISPLAY.try_get() {
        // This is safe because we are literally shutting down
        // No one else should be writing to it.
        unsafe { disp.force_unlock() };
        let mut disp = disp.spin_lock();
        let _ = disp.clear(Rgb888::BLACK);
        let info = info.to_string();
        let text = Text::with_baseline(
            &info,
            Point::zero(),
            MonoTextStyle::new(&FONT_9X15, Rgb888::RED),
            Baseline::Top,
        );
        let _ = text.draw(disp.as_mut());
    }
    //exit_qemu(kernel::qemu::QemuExitCode::Failed);
    loop {}
}

entry_point!(kernel_main, config = &BOOTLOADER_CONFIG);

fn kernel_main(boot_info: &'static mut BootInfo) -> ! {
    kernel::init(boot_info);
    let main_span = span!(Level::TRACE, "kernel_main");
    let _span = main_span.enter();

    let utc_date = RTC.spin_lock().read_date_time();
    info!(%utc_date);

    spawn(print_keypresses());

    spawn(async {
        sleep(Duration::from_secs(10)).await;
        kernel::display::clock::draw_clock().await;
    });

    #[cfg(test)]
    test_main();

    println!("Hello World{}", "!");
    vga_println!("Hello World!");

    run()
}
