#![allow(dead_code)]
use bootloader_api::info::FrameBufferInfo;
use core::str;
use core::{fmt, slice};
use embedded_graphics::primitives::{PrimitiveStyle, Rectangle, StyledDrawable};
use embedded_graphics::{mono_font::MonoTextStyle, pixelcolor::Rgb888, prelude::*, text::Text};

use crate::{
    framebuffer::DISPLAY,
    util::{once::OnceLock, r#async::mutex::IntMutex},
};

pub static WRITER: OnceLock<IntMutex<Writer>> = OnceLock::new();

pub struct Writer {
    info: FrameBufferInfo,
    x_pos: usize,
    y_pos: usize,
}

impl Writer {
    pub fn new(info: FrameBufferInfo) -> Self {
        Self {
            info,
            x_pos: 0,
            y_pos: 0,
        }
    }

    pub fn write_byte(&mut self, byte: u8) {
        match byte {
            b'\n' => self.new_line(),
            byte => {
                let new_xpos = self.x_pos + 9;
                if new_xpos >= self.info.width {
                    self.new_line();
                }
                let new_ypos = self.y_pos + 15;
                if new_ypos >= self.info.height {
                    self.x_pos = 0;
                    self.y_pos = 0;
                    let _ = DISPLAY.get().spin_lock().clear(Rgb888::BLACK);
                }

                // Safe because we should only be getting ascii
                let slice = unsafe { slice::from_raw_parts(&byte as *const u8, 1) };
                let text = unsafe { str::from_utf8_unchecked(slice) };
                let text = Text::with_baseline(
                    text,
                    embedded_graphics::geometry::Point {
                        x: self.x_pos as i32,
                        y: self.y_pos as i32,
                    },
                    MonoTextStyle::new(
                        &embedded_graphics::mono_font::ascii::FONT_9X15,
                        Rgb888::WHITE,
                    ),
                    embedded_graphics::text::Baseline::Top,
                );
                let _ = text.draw(DISPLAY.get().spin_lock().as_mut());
                self.x_pos += 9;
            }
        }
    }

    fn backspace(&mut self) {
        if self.x_pos == 0 {
            self.y_pos -= 15;
            self.x_pos = (self.info.stride / 9) * 9;
        }
        self.x_pos -= 9;
        let rect = Rectangle::new(
            Point {
                x: self.x_pos as i32,
                y: self.y_pos as i32,
            },
            Size {
                width: 9,
                height: 15,
            },
        );
        let _ = rect.draw_styled(
            &PrimitiveStyle::with_fill(Rgb888::BLACK),
            DISPLAY.get().spin_lock().as_mut(),
        );
    }

    fn new_line(&mut self) {
        self.y_pos += 15;
        self.x_pos = 0;
    }

    pub fn write_string(&mut self, s: &str) {
        for byte in s.bytes() {
            match byte {
                // printable ASCII byte or newline
                0x20..=0x7e | b'\n' => self.write_byte(byte),
                // backspace
                0x08 => self.backspace(),
                // not part of printable ASCII range
                _ => self.write_byte(0xfe),
            }
        }
    }
}

impl fmt::Write for Writer {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_string(s);
        Ok(())
    }
}

#[macro_export]
macro_rules! vga_print {
    ($($arg:tt)*) => ($crate::vga_buffer::_print(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! vga_println {
    () => ($crate::vga_print!("\n"));
    ($($arg:tt)*) => ($crate::vga_print!("{}\n", format_args!($($arg)*)));
}

#[doc(hidden)]
pub fn _print(args: fmt::Arguments) {
    use core::fmt::Write;

    if let Ok(writer) = WRITER.try_get() {
        writer.spin_lock().write_fmt(args).unwrap();
    }
}
