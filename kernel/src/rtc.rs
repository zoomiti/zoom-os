use core::time::Duration;

use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use thiserror::Error;
use tracing::{instrument, warn};
use x86_64::instructions::{interrupts, port::Port};

use crate::util::r#async::mutex::IntMutex;

const NMI_ENABLE: bool = true;

// rate 3 => 112 uS
pub const TIMER_PERIOD: Duration = Duration::from_micros(112);
pub const TIMER_FREQ: usize = 8192;
pub static RTC: IntMutex<Rtc> = IntMutex::new(Rtc::new());

#[derive(Debug)]
pub struct Rtc {
    command: Port<u8>,
    data: Port<u8>,
}

#[tracing::instrument(name = "rtc_init")]
pub fn init() {
    let mut rtc = RTC.spin_lock();
    rtc.set_data_format();
    // OS-DEV says 3 -> 8kHz but it seems like 4 is correct
    rtc.set_freq(4);
    rtc.enable_interrupts();
}

impl Rtc {
    pub const fn new() -> Self {
        Self {
            command: Port::new(0x70),
            data: Port::new(0x71),
        }
    }

    fn set_freq(&mut self, rate: u8) {
        debug_assert!(rate > 2 && rate < 16);
        let prev = self.read_cmos_reg(0x8A);
        self.write_cmos_reg(0x8A, (prev & 0xF0) | rate);
    }

    fn enable_interrupts(&mut self) {
        // Setup Interupts
        // Read cmos
        let prev = self.read_cmos_reg(0x8b);

        // Write back
        self.write_cmos_reg(0x8b, prev | 0x40);
        self.clear_interrup_mask();
    }

    pub fn clear_interrup_mask(&mut self) {
        self.read_cmos_reg(0x0C);
    }

    #[instrument]
    fn set_data_format(&mut self) {
        const STATUS_REG_B_NUM: u8 = 0x0b;
        let mut status_reg = self.read_cmos_reg(STATUS_REG_B_NUM);
        status_reg |= 1 << 1; // Enables 24 hour mode
        status_reg |= 1 << 2; // Enables binary format of retrieved values

        self.write_cmos_reg(STATUS_REG_B_NUM, status_reg);
    }
    #[instrument]
    pub fn read_date_time(&mut self) -> NaiveDateTime {
        loop {
            if let Ok(time) = self.try_read_date_time() {
                return time;
            }
            warn!("failed to get time");
            core::hint::spin_loop();
        }
    }

    pub fn try_read_date_time(&mut self) -> Result<NaiveDateTime, FromNaiveDateTimeError> {
        self.update_guarded_op(|rtc_ref| {
            let mut seconds = rtc_ref.read_cmos_reg(0x00);
            let mut minutes = rtc_ref.read_cmos_reg(0x02);
            let mut hours = rtc_ref.read_cmos_reg(0x04);
            let weekday = rtc_ref.read_cmos_reg(0x06);
            let mut day = rtc_ref.read_cmos_reg(0x07);
            let mut month = rtc_ref.read_cmos_reg(0x08);
            let mut year = rtc_ref.read_cmos_reg(0x09);
            let mut century = rtc_ref.read_cmos_reg(0x32);

            // Convert BCD to binary values if necessary
            // It shouldn't be, because by now we configured RTC but it seems necessary regardless
            let register_b = rtc_ref.read_cmos_reg(0x0B);
            if register_b & 0x04 == 0 {
                seconds = (seconds & 0x0F) + ((seconds / 16) * 10);
                minutes = (minutes & 0x0F) + ((minutes / 16) * 10);
                hours = ((hours & 0x0F) + (((hours & 0x70) / 16) * 10)) | (hours & 0x80);
                day = (day & 0x0F) + ((day / 16) * 10);
                month = (month & 0x0F) + ((month / 16) * 10);
                year = (year & 0x0F) + ((year / 16) * 10);
                century = (century & 0x0F) + ((century / 16) * 10);
            }

            RTCDateTime {
                seconds,
                minutes,
                hours,
                weekday,
                day,
                month,
                year,
                century,
            }
            .try_into()
        })
    }

    fn select_reg(&mut self, reg: u8) {
        // This is the first operation in any handling of rtc so this should always check if
        // interrupts are disable before doing rtc stuff
        debug_assert!(!interrupts::are_enabled());
        unsafe { self.command.write(get_nmi_mask() | reg) }
    }
    fn read_cmos_reg(&mut self, reg: u8) -> u8 {
        self.select_reg(reg);
        unsafe { self.data.read() }
    }
    fn write_cmos_reg(&mut self, reg: u8, val: u8) {
        self.select_reg(reg);
        unsafe { self.data.write(val) }
    }
    fn update_guarded_op<R, F: Fn(&mut Rtc) -> R>(&mut self, f: F) -> R {
        let mut ret;
        loop {
            if self.update_in_progress() {
                continue;
            }

            ret = f(self);

            if self.update_in_progress() {
                continue;
            }

            break;
        }

        ret
    }
    fn update_in_progress(&mut self) -> bool {
        const STATUS_REG_A_NUM: u8 = 0x0a;
        self.select_reg(STATUS_REG_A_NUM);
        in_progress_set(unsafe { self.data.read() })
    }
}

fn in_progress_set(status_reg_a: u8) -> bool {
    const IN_PROGRESS_MASK: u8 = 1 << 7;
    status_reg_a & IN_PROGRESS_MASK == IN_PROGRESS_MASK
}

impl Default for Rtc {
    fn default() -> Self {
        Self::new()
    }
}

fn get_nmi_mask() -> u8 {
    if NMI_ENABLE {
        0
    } else {
        1 << 7
    }
}

#[derive(Debug)]
pub struct RTCDateTime {
    pub seconds: u8,
    pub minutes: u8,
    pub hours: u8,
    pub weekday: u8,
    pub day: u8,
    pub month: u8,
    pub year: u8,
    pub century: u8,
}

#[derive(Error, Debug)]
#[error("Error converting RTC time to NaiveDateTime")]
pub enum FromNaiveDateTimeError {
    #[error("Invalid Date: {month}/{day}/{year}")]
    InvalidDate { year: i32, month: u32, day: u32 },
    #[error("Invalid Time: {hour}:{min}:{sec}")]
    InvalidTime { hour: u32, min: u32, sec: u32 },
}

impl TryFrom<RTCDateTime> for NaiveDateTime {
    type Error = FromNaiveDateTimeError;

    fn try_from(value: RTCDateTime) -> Result<Self, Self::Error> {
        let year = value.century as i32 * 100 + value.year as i32;
        let month = value.month as u32;
        let day = value.day as u32;
        let date = NaiveDate::from_ymd_opt(year, month, day)
            .ok_or(FromNaiveDateTimeError::InvalidDate { year, month, day })?;

        let hour = value.hours as u32;
        let min = value.minutes as u32;
        let sec = value.seconds as u32;
        let time = NaiveTime::from_hms_opt(hour, min, sec)
            .ok_or(FromNaiveDateTimeError::InvalidTime { hour, min, sec })?;

        Ok(NaiveDateTime::new(date, time))
    }
}
