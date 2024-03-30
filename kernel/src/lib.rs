#![no_std]
#![cfg_attr(test, no_main)]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![feature(allocator_api)]
#![feature(const_mut_refs)]
#![test_runner(crate::testing::test_runner)]
#![reexport_test_harness_main = "test_main"]

extern crate alloc;

pub mod acpi;
pub mod allocator;
pub mod apic;
pub mod display;
pub mod framebuffer;
pub mod gdt;
pub mod interrupts;
pub mod keyboard;
pub mod memory;
pub mod qemu;
pub mod rtc;
pub mod serial;
pub mod task;
pub mod testing;
pub mod tracer;
pub mod util;
// Deprecated since new version of bootloader
//pub mod vga_buffer;

use acpi::{KERNEL_ACPI_ADDR, KERNEL_ACPI_LEN};
use allocator::{KERNEL_HEAP_ADDR, KERNEL_HEAP_LEN};
use apic::{KERNEL_APIC_ADDR, KERNEL_APIC_LEN};
#[cfg(test)]
use bootloader_api::entry_point;
use bootloader_api::{config::Mapping, BootInfo, BootloaderConfig};
use tracing::{info, span, trace, Level};
use util::once::OnceLock;
use x86_64::{
    structures::paging::{Page, Size4KiB},
    VirtAddr,
};

pub static PHYS_OFFSET: OnceLock<u64> = OnceLock::new();

pub static KERNEL_CODE_ADDR: OnceLock<VirtAddr> = OnceLock::new();
pub static KERNEL_CODE_LEN: OnceLock<usize> = OnceLock::new();

#[test_case]
fn test_breakpoint_exception() {
    x86_64::instructions::interrupts::int3();
    // Execution should continue
}

pub fn init(boot_info: &'static mut BootInfo) {
    let kernel_code_addr = VirtAddr::new(boot_info.kernel_image_offset);
    let kernel_code_len = boot_info.kernel_len;
    let kernel_heap_addr = (kernel_code_addr + kernel_code_len).align_up(Page::<Size4KiB>::SIZE);
    let kernel_heap_len = KERNEL_HEAP_LEN;
    let kernel_acpi_addr =
        (kernel_heap_addr + kernel_heap_len as u64).align_up(Page::<Size4KiB>::SIZE);
    let kernel_acpi_len = KERNEL_ACPI_LEN;
    let kernel_apic_addr =
        (kernel_acpi_addr + kernel_acpi_len as u64).align_up(Page::<Size4KiB>::SIZE);
    let _kernel_apic_len = KERNEL_APIC_LEN;

    let phys_offset = boot_info.physical_memory_offset.into_option().unwrap();

    println!("kernel_code_addr: {:p}", kernel_code_addr);
    println!("kernel_code_len: {:#x}", kernel_code_len);
    println!("kernel_heap_addr: {:p}", kernel_heap_addr);
    println!("kernel_heap_len: {:#x}", kernel_heap_len);
    println!("kernel_acpi_addr: {:p}", kernel_acpi_addr);
    println!("kernel_acpi_len: {:#x}", kernel_acpi_len);
    println!("kernel_apic_addr: {:p}", kernel_apic_addr);
    println!("kernel_apic_len: {:#x}", _kernel_apic_len);

    KERNEL_CODE_ADDR.init_once(|| kernel_code_addr);
    KERNEL_CODE_LEN.init_once(|| kernel_code_len as usize);
    KERNEL_HEAP_ADDR.init_once(|| kernel_heap_addr);
    KERNEL_ACPI_ADDR.init_once(|| kernel_acpi_addr);
    KERNEL_APIC_ADDR.init_once(|| kernel_apic_addr);

    PHYS_OFFSET.init_once(|| phys_offset);

    memory::init(&boot_info.memory_regions).expect("page alloc failed to be created");
    tracer::init();
    let init_span = span!(Level::TRACE, "init");
    let _guard = init_span.enter();

    gdt::init();
    trace!("init gdt");
    interrupts::init_idt();
    trace!("init idt");
    // Unwrapping is okay because if we don't have rsdp we don't know how to boot
    acpi::init(*boot_info.rsdp_addr.as_ref().unwrap());
    trace!("init acpi");
    apic::init();
    trace!("init apic");
    rtc::init();
    trace!("init rtc");
    // I don't really want to support a target with no display
    framebuffer::init(boot_info.framebuffer.as_mut().unwrap());
    info!("init framebuffer");

    x86_64::instructions::interrupts::enable();
}

pub const BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::Dynamic);
    config.mappings.dynamic_range_start = Some(0xffff_8000_0000_0000);
    config.mappings.dynamic_range_end = Some(0xffff_ffff_ffff_ffff);
    config
};

#[cfg(test)]
entry_point!(test_kernel_main, config = &BOOTLOADER_CONFIG);

#[cfg(test)]
pub fn test_kernel_main(boot_info: &'static mut BootInfo) -> ! {
    use util::hlt_loop;

    init(boot_info); // new
    test_main();
    hlt_loop()
}
