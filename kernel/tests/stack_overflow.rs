#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

use core::panic::PanicInfo;

use kernel::{
    print, println,
    qemu::exit_qemu,
    util::{hlt_loop, once::Lazy},
};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    print!("stack_overflow::stack_overflow...\t");

    kernel::gdt::init();
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

static TEST_IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    unsafe {
        idt.double_fault
            .set_handler_fn(test_double_fault_handler)
            .set_stack_index(kernel::gdt::DOUBLE_FAULT_IST_INDEX);
    }

    idt
});

fn init_test_init() {
    TEST_IDT.load();
}

extern "x86-interrupt" fn test_double_fault_handler(
    _stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    println!("[ok]");
    exit_qemu(kernel::qemu::QemuExitCode::Success);
    hlt_loop()
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kernel::testing::test_panic_handler(info)
}
