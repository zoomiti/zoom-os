#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use lazy_static::lazy_static;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
use zoom_os::{exit_qemu, hlt_loop, print, println};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    print!("stack_overflow::stack_overflow...\t");

    zoom_os::gdt::init();
    init_test_init();

    // trigger stack overflow
    stack_overflow();

    panic!("Execution continued after stack overflow");
}

#[allow(unconditional_recursion)]
fn stack_overflow() {
    stack_overflow();
    volatile::Volatile::new(0).read(); // Prevent tail recursion
}

lazy_static! {
    static ref TEST_IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        unsafe {
            idt.double_fault
                .set_handler_fn(test_double_fault_handler)
                .set_stack_index(zoom_os::gdt::DOUBLE_FAULT_IST_INDEX);
        }

        idt
    };
}

fn init_test_init() {
    TEST_IDT.load();
}

extern "x86-interrupt" fn test_double_fault_handler(
    _stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    println!("[ok]");
    exit_qemu(zoom_os::QemuExitCode::Success);
    hlt_loop()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    zoom_os::test_panic_handler(info)
}