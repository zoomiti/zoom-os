use core::sync::atomic::Ordering;

use pic8259::ChainedPics;
use x86_64::{
    instructions::port::Port,
    structures::idt::{InterruptDescriptorTable, InterruptStackFrame},
};

use crate::{
    gdt,
    keyboard::add_scancode,
    util::{
        once::Lazy,
        r#async::{
            mutex::Mutex,
            sleep_future::{wake_sleep, MONOTONIC_TIME},
        },
    },
    vga_println,
};

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub static PICS: Mutex<ChainedPics> =
    Mutex::new(unsafe { ChainedPics::new(PIC_1_OFFSET, PIC_2_OFFSET) });

fn notify_end_of_interrupt(index: InterruptIndex) {
    unsafe { PICS.spin_lock().notify_end_of_interrupt(index.as_u8()) }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Keyboard,
}

impl InterruptIndex {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

static IDT: Lazy<InterruptDescriptorTable> = Lazy::new(|| {
    let mut idt = InterruptDescriptorTable::new();
    idt.breakpoint.set_handler_fn(breakpoint_handler);
    unsafe {
        idt.double_fault
            .set_handler_fn(double_fault_hander)
            .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
    }
    idt[InterruptIndex::Timer.as_u8()].set_handler_fn(timer_interrupt_handler);
    idt[InterruptIndex::Keyboard.as_u8()].set_handler_fn(keyboard_interrupt_handler);
    idt
});

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn breakpoint_handler(stack_frame: InterruptStackFrame) {
    vga_println!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn double_fault_hander(
    stack_frame: InterruptStackFrame,
    _error_code: u64,
) -> ! {
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn timer_interrupt_handler(_stack_frame: InterruptStackFrame) {
    let curr_time = MONOTONIC_TIME.fetch_add(1, Ordering::Acquire);
    wake_sleep(curr_time);
    notify_end_of_interrupt(InterruptIndex::Timer);
}

extern "x86-interrupt" fn keyboard_interrupt_handler(_stack_frame: InterruptStackFrame) {
    //static KEYBOARD: Lazy<Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>>> = Lazy::new(|| {
    //    Mutex::new(Keyboard::new(
    //        ScancodeSet1::new(),
    //        layouts::Us104Key,
    //        HandleControl::Ignore,
    //    ))
    //});

    //let mut keyboard = KEYBOARD.lock();
    let mut port = Port::new(0x60);

    let scancode: u8 = unsafe { port.read() };
    add_scancode(scancode);

    //if let Ok(Some(key_event)) = keyboard.add_byte(scancode) {
    //    if let Some(key) = keyboard.process_keyevent(key_event) {
    //        match key {
    //            DecodedKey::Unicode(character) => vga_print!("{}", character),
    //            DecodedKey::RawKey(key) => vga_print!("{:?}", key),
    //        }
    //    }
    //}

    notify_end_of_interrupt(InterruptIndex::Keyboard);
}
