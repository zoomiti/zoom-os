use core::ops::DerefMut;

use x86_64::instructions::{interrupts::without_interrupts, port::Port};

use crate::util::{once::Lazy, r#async::mutex::Mutex};

const NMI_ENABLE: bool = true;

type Ports = (Port<u8>, Port<u8>);

pub static RTC: Lazy<Mutex<Ports>> = Lazy::new(|| Mutex::new((Port::new(0x70), Port::new(0x71))));

pub fn init() {
    let mut rtc = RTC.spin_lock();
    // Read cmos
    let prev = read_cmos_reg(rtc.deref_mut(), 0x8b);

    // Write back
    write_cmos_reg(rtc.deref_mut(), 0x8b, prev | 0x40);
    drop(rtc);
    clear_interrup_mask()
}

fn select_reg(port: &mut Ports, reg: u8) {
    unsafe { port.0.write(get_nmi_mask() | reg) }
}

fn read_cmos_reg(port: &mut Ports, reg: u8) -> u8 {
    select_reg(port, reg);
    unsafe { port.1.read() }
}
fn write_cmos_reg(port: &mut Ports, reg: u8, val: u8) {
    select_reg(port, reg);
    unsafe { port.1.write(val) }
}

pub fn clear_interrup_mask() {
    without_interrupts(|| {
        let mut rtc = RTC.spin_lock();
        read_cmos_reg(rtc.deref_mut(), 0x0C);
    })
}

fn get_nmi_mask() -> u8 {
    if NMI_ENABLE {
        0
    } else {
        1 << 7
    }
}
