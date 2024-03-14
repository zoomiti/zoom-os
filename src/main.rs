#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(zoom_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

use core::panic::PanicInfo;

use zoom_os::{hlt_loop, println, vga_println};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}

#[no_mangle]
pub extern "C" fn _start() -> ! {
    zoom_os::init();

    #[cfg(test)]
    test_main();

    vga_println!("Hello World{}", "!");

    hlt_loop()
}
