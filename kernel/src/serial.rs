use core::u16;

use uart_16550::SerialPort;

use crate::util::{once::Lazy, r#async::mutex::IntMutex};

const SERIAL_ADDR: u16 = 0x3f8;

pub static SERIAL1: Lazy<IntMutex<SerialPort>> = Lazy::new(|| {
    let mut serial_port = unsafe { SerialPort::new(SERIAL_ADDR) };
    serial_port.init();
    IntMutex::new(serial_port)
});

#[doc(hidden)]
pub fn _print(args: ::core::fmt::Arguments) {
    use core::fmt::Write;

    SERIAL1
        .spin_lock()
        .write_fmt(args)
        .expect("Printing to serial failed");
}

/// Prints to the host through the serial interface.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::serial::_print(format_args!($($arg)*));
    };
}

/// Prints to the host through the serial interface, appending a newline.
#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($fmt:expr) => ($crate::print!(concat!($fmt, "\n")));
    ($fmt:expr, $($arg:tt)*) => ($crate::print!(
        concat!($fmt, "\n"), $($arg)*));
}
