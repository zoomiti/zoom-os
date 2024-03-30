use core::ops::DerefMut;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use tracing::{instrument, trace};
use x86_64::instructions::{
    interrupts::{self, without_interrupts},
    port::Port,
};

use crate::util::{once::Lazy, r#async::mutex::Mutex};

const NMI_ENABLE: bool = true;

type Ports = (Port<u8>, Port<u8>);

pub static RTC: Lazy<Mutex<Ports>> = Lazy::new(|| Mutex::new((Port::new(0x70), Port::new(0x71))));

#[derive(Debug)]
pub struct DateTime {
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub weekday: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
    pub century: u8,
}

pub fn init() {
    let mut rtc = RTC.spin_lock();
    // Setup Intterupts
    // Read cmos
    let prev = read_cmos_reg(rtc.deref_mut(), 0x8b);

    // Write back
    write_cmos_reg(rtc.deref_mut(), 0x8b, prev | 0x40);
    drop(rtc);
    clear_interrup_mask();

    // Set Freq

    // set data format
    let mut rtc = RTC.spin_lock();
    set_data_format(rtc.deref_mut());
}

#[instrument]
pub fn read_date_time() -> NaiveDateTime {
    interrupts::without_interrupts(|| {
        let mut rtc = RTC.spin_lock();
        let rtc_ref = rtc.as_mut();
        update_guarded_op(rtc_ref, |rtc_ref| {
            let seconds = read_cmos_reg(rtc_ref, 0x00);
            let minutes = read_cmos_reg(rtc_ref, 0x02);
            let hours = read_cmos_reg(rtc_ref, 0x04);
            let weekday = read_cmos_reg(rtc_ref, 0x06);
            let day = read_cmos_reg(rtc_ref, 0x07);
            let month = read_cmos_reg(rtc_ref, 0x08);
            let year = read_cmos_reg(rtc_ref, 0x09);
            let century = read_cmos_reg(rtc_ref, 0x32);

            DateTime {
                seconds,
                minutes,
                hours,
                weekday,
                day,
                month,
                year,
                century,
            }
            .into()
        })
    })
}

impl From<DateTime> for NaiveDateTime {
    fn from(value: DateTime) -> Self {
        NaiveDateTime::new(
            NaiveDate::from_ymd_opt(
                value.century as i32 * 100 + value.year as i32,
                value.month as u32,
                value.day as u32,
            )
            .unwrap(),
            NaiveTime::from_hms_opt(
                value.hours as u32,
                value.minutes as u32,
                value.seconds as u32,
            )
            .unwrap(),
        )
    }
}

fn update_in_progress(port: &mut Ports) -> bool {
    const STATUS_REG_A_NUM: u8 = 0x0a;
    select_reg(port, STATUS_REG_A_NUM);
    in_progress_set(unsafe { port.1.read() })
}

fn in_progress_set(status_reg_a: u8) -> bool {
    const IN_PROGRESS_MASK: u8 = 1 << 7;
    status_reg_a & IN_PROGRESS_MASK == IN_PROGRESS_MASK
}

fn update_guarded_op<R, F: Fn(&mut Ports) -> R>(cmos_io: &mut Ports, f: F) -> R {
    let mut ret;
    loop {
        if update_in_progress(cmos_io) {
            trace!("in progress");
            continue;
        }

        ret = f(cmos_io);

        if update_in_progress(cmos_io) {
            continue;
        }

        break;
    }

    ret
}

fn set_data_format(port: &mut Ports) {
    const STATUS_REG_B_NUM: u8 = 0x0b;
    let mut status_reg = read_cmos_reg(port, STATUS_REG_B_NUM);
    status_reg |= 1 << 1; // Enables 24 hour mode
    status_reg |= 1 << 2; // Enables binary format of retrieved values

    write_cmos_reg(port, STATUS_REG_B_NUM, status_reg);
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
