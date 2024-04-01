use core::ptr::addr_of;

use bootloader_api::info::{FrameBuffer, PixelFormat};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{Rgb888, RgbColor},
    Pixel,
};
use x86_64::{
    structures::paging::{Mapper, Page, PageTableFlags, Size4KiB},
    VirtAddr,
};

use crate::{
    memory::MAPPER,
    util::{once::OnceLock, r#async::mutex::Mutex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

pub static DISPLAY: OnceLock<Mutex<Display<'static>>> = OnceLock::new();

pub fn init(framebuffer: &'static mut FrameBuffer) {
    // Write combine
    let buffer = framebuffer.buffer();

    let page_range = {
        let region_start = VirtAddr::new(addr_of!(buffer[0]) as u64);
        let region_end = region_start + buffer.len() as u64 - 1;
        let region_start_page = Page::<Size4KiB>::containing_address(region_start);
        let region_end_page = Page::containing_address(region_end);
        region_start_page..=region_end_page
    };

    for page in page_range {
        unsafe {
            MAPPER
                .spin_lock()
                .update_flags(
                    page,
                    PageTableFlags::PRESENT
                        | PageTableFlags::WRITABLE
                        | PageTableFlags::NO_EXECUTE
                        | PageTableFlags::WRITE_THROUGH
                        | PageTableFlags::NO_CACHE,
                )
                .unwrap()
                .flush();
        }
    }

    DISPLAY.init_once(|| Mutex::new(Display::new(framebuffer)));
}

pub struct Display<'f> {
    framebuffer: &'f mut FrameBuffer,
}

impl<'f> Display<'f> {
    pub fn new(framebuffer: &'f mut FrameBuffer) -> Display {
        Display { framebuffer }
    }

    fn draw_pixel(&mut self, Pixel(coordinates, color): Pixel<Rgb888>) {
        // ignore any out of bounds pixels
        let (width, height) = {
            let info = self.framebuffer.info();

            (info.width, info.height)
        };

        let (x, y) = {
            let c: (i32, i32) = coordinates.into();
            (c.0 as usize, c.1 as usize)
        };

        if (0..width).contains(&x) && (0..height).contains(&y) {
            let color = Color {
                red: color.r(),
                green: color.g(),
                blue: color.b(),
            };

            set_pixel_in(self.framebuffer, Position { x, y }, color);
        }
    }
}

pub fn set_pixel_in(framebuffer: &mut FrameBuffer, position: Position, color: Color) {
    let info = framebuffer.info();

    // calculate offset to first byte of pixel
    let byte_offset = {
        // use stride to calculate pixel offset of target line
        let line_offset = position.y * info.stride;
        // add x position to get the absolute pixel offset in buffer
        let pixel_offset = line_offset + position.x;
        // convert to byte offset
        pixel_offset * info.bytes_per_pixel
    };

    // set pixel based on color format
    let pixel_buffer = &mut framebuffer.buffer_mut()[byte_offset..];
    match info.pixel_format {
        PixelFormat::Rgb => {
            pixel_buffer[0] = color.red;
            pixel_buffer[1] = color.green;
            pixel_buffer[2] = color.blue;
        }
        PixelFormat::Bgr => {
            pixel_buffer[0] = color.blue;
            pixel_buffer[1] = color.green;
            pixel_buffer[2] = color.red;
        }
        PixelFormat::U8 => {
            // use a simple average-based grayscale transform
            let gray = color.red / 3 + color.green / 3 + color.blue / 3;
            pixel_buffer[0] = gray;
        }
        other => panic!("unknown pixel format {other:?}"),
    }
}

impl<'f> DrawTarget for Display<'f> {
    type Color = Rgb888;

    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = embedded_graphics::prelude::Pixel<Self::Color>>,
    {
        for pixel in pixels {
            self.draw_pixel(pixel);
        }
        Ok(())
    }
}

impl<'f> OriginDimensions for Display<'f> {
    fn size(&self) -> Size {
        let info = self.framebuffer.info();

        Size::new(info.width as u32, info.height as u32)
    }
}
