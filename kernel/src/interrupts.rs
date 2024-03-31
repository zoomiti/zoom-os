use core::sync::atomic::Ordering;

use num_enum::IntoPrimitive;
use raw_cpuid::{CpuId, Hypervisor};
use tracing::error;
use x86_64::{
    instructions::port::Port,
    structures::idt::{
        InterruptDescriptorTable, InterruptStackFrame, PageFaultErrorCode, SelectorErrorCode,
    },
};

use crate::{
    apic::LAPIC,
    gdt,
    keyboard::add_scancode,
    pic::PICS,
    println, rtc,
    util::{
        once::Lazy,
        r#async::sleep_future::{wake_sleep, MONOTONIC_TIME},
    },
};

pub const INTERRUPT_START: u8 = 32;

//pub static PICS: Mutex<ChainedPics> =
//   Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

fn notify_end_of_interrupt(index: InterruptIndex) {
    if let Ok(lapic) = LAPIC.try_get() {
        unsafe { lapic.spin_lock().end_of_interrupt() }
    } else {
        // If LAPIC is not init that means we are in legacy mode
        unsafe { PICS.spin_lock().notify_end_of_interrupt(index.into()) }
    }
}

#[derive(Debug, Clone, Copy, IntoPrimitive)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = INTERRUPT_START,
    Keyboard,
    Clock = INTERRUPT_START + 8,
    LapicErr = INTERRUPT_START + 17, //49
    Spurious = 0xff,
}

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    idt.general_protection_fault
        .set_handler_fn(general_protection_fault_handler);
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    idt.page_fault.set_handler_fn(page_fault_handler);
    idt.invalid_tss.set_handler_fn(invalid_tss_handler);
    idt.segment_not_present
        .set_handler_fn(segment_not_present_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_hander)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt[InterruptIndex::Timer as u8].set_handler_fn(timer_interrupt_handler);
    idt[InterruptIndex::Keyboard as u8].set_handler_fn(keyboard_interrupt_handler);
    idt[InterruptIndex::LapicErr as u8].set_handler_fn(lapic_err_interrupt_handler);
    idt[InterruptIndex::Spurious as u8].set_handler_fn(spurious_interrupt_handler);
    idt[InterruptIndex::Clock as u8]
        .set_handler_fn(clock_interrupt_handler)
        .disable_interrupts(true);

    idt
});

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    println!(
        "encountered a general protection fault, error code {} =",
        error_code
    );
    println!("index: {}", (error_code >> 3) & ((1 << 14) - 1));
    println!("tbl: {}", (error_code >> 1) & 0b11);
    println!("e: {}", error_code & 1);

    panic!("EXCEPTION: GENERAL PROTECTION FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_hander(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!(
        "EXCEPTION: DOUBLE FAULT\n{:#?}\nerror: {_error_code}",
        stack_frame
    );
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    panic!(
        "EXCEPTION: PAGE FAULT\nAccessed Address: {:?}\nError Code: {:?}\n{:#?}",
        Cr2::read(),
        error_code,
        stack_frame
    );
}

extern "x86-interrupt" fn invalid_tss_handler(stack_frame: InterruptStackFrame, error_code: u64) {
    error!("Invalid TSS at segment selector: {error_code:#?}\n{stack_frame:#?}");
}

extern "x86-interrupt" fn segment_not_present_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
) {
    let error_code = SelectorErrorCode::new_truncate(error_code);
    let cpu = CpuId::new();
    let index = match cpu.get_hypervisor_info() {
        Some(hypervisor) if hypervisor.identify() == Hypervisor::QEMU => error_code.index() / 2,
        _ => error_code.index(),
    };
    error!(
        "Segmet not present {:#?}\n\
        Descriptor Table involved: {:#?}\n\
        {stack_frame:#?}",
        index,
        error_code.descriptor_table(),
    );
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    notify_end_of_interrupt(InterruptIndex::Timer);
}

extern "x86-interrupt" fn clock_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let curr_time = MONOTONIC_TIME.fetch_add(1, Ordering::Acquire);
    wake_sleep(curr_time);
    notify_end_of_interrupt(InterruptIndex::Clock);
    rtc::clear_interrup_mask();
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let mut port = Port::new(0x60);

    let scancode: u8 = unsafe { port.read() };
    add_scancode(scancode);

    notify_end_of_interrupt(InterruptIndex::Keyboard);
}

extern "x86-interrupt" fn lapic_err_interrupt_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: LAPIC ERROR\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn spurious_interrupt_handler(stack_frame: InterruptStackFrame) {
    panic!("EXCEPTION: SPURIOUS INTERRUPT\n{:#?}", stack_frame);
}
