#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![test_runner(zoom_os::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

use core::panic::PanicInfo;

use bootloader::{entry_point, BootInfo};
use x86_64::VirtAddr;
use zoom_os::{
    allocator,
    keyboard::print_keypresses,
    memory::{self, BootInfoFrameAllocator},
    println,
    task::executor::Executor,
    vga_println,
};

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!("{}", info);
    loop {}
}

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    zoom_os::init();

    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe { BootInfoFrameAllocator::init(&boot_info.memory_map) };

    allocator::init_heap(&mut mapper, &mut frame_allocator).expect("heap initialization failed");

    let mut executor = Executor::new();
    executor.spawn(print_keypresses());
    executor.spawn(async {
        vga_println!("Asynchronously executed");
    });

    #[cfg(test)]
    test_main();

    vga_println!("Hello World{}", "!");

    executor.run()
}
