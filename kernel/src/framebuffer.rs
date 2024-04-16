use core::{ptr::addr_of, u8, usize};

use alloc::{boxed::Box, vec};
use bootloader_api::info::{FrameBuffer, FrameBufferInfo, PixelFormat};
use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{Dimensions, OriginDimensions, Point, Size},
    pixelcolor::{Rgb888, RgbColor},
    primitives::Rectangle,
    Pixel,
};
use x86_64::{
    structures::paging::{Mapper, Page, PageTableFlags, Size4KiB},
    VirtAddr,
};

use crate::{
    memory::mapping::MAPPER,
    util::{once::OnceLock, r#async::mutex::Mutex},
    vga_buffer::{Writer, WRITER},
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

impl From<Color> for Rgb888 {
    fn from(value: Color) -> Self {
        Rgb888::new(value.red, value.green, value.blue)
    }
}

impl From<Rgb888> for Color {
    fn from(value: Rgb888) -> Self {
        Color {
            red: value.r(),
            green: value.g(),
            blue: value.b(),
        }
    }
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

    WRITER.init_once(|| Mutex::new(Writer::new(framebuffer.info())));

    DISPLAY.init_once(|| Mutex::new(Display::new(framebuffer)));
}

pub struct Display<'f> {
    framebuffer: &'f mut FrameBuffer,
    backbuffer: Box<[u8]>,
}

impl<'f> Display<'f> {
    pub fn new(framebuffer: &'f mut FrameBuffer) -> Display {
        Display {
            backbuffer: vec![
                0;
                framebuffer.info().width
                    * framebuffer.info().height
                    * framebuffer.info().bytes_per_pixel
            ]
            .into_boxed_slice(),
            framebuffer,
        }
    }

    #[inline(always)]
    pub fn get_info(&self) -> FrameBufferInfo {
        self.framebuffer.info()
    }

    #[inline(always)]
    fn draw_pixel(&mut self, Pixel(Point { x, y }, color): Pixel<Rgb888>) {
        // ignore any out of bounds pixels
        let info = self.framebuffer.info();
        let (width, height) = { (info.width, info.height) };

        let (x, y) = { (x as usize, y as usize) };

        if (0..width).contains(&x) && (0..height).contains(&y) {
            let color = Color {
                red: color.r(),
                green: color.g(),
                blue: color.b(),
            };

            // calculate offset to first byte of pixel
            let byte_offset = {
                // use stride to calculate pixel offset of target line
                let line_offset = y * info.width;
                // add x position to get the absolute pixel offset in buffer
                let pixel_offset = line_offset + x;
                // convert to byte offset
                pixel_offset * info.bytes_per_pixel
            };

            // set pixel based on color format
            let pixel_buffer = &mut self.backbuffer[byte_offset..];
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
    }

    pub fn draw_frame(&mut self) {
        let info = self.get_info();
        for y in 0..info.height {
            let wide_offset = (y * info.width) * info.bytes_per_pixel;
            let offset = (y * info.stride) * info.bytes_per_pixel;
            unsafe {
                let wide = self.backbuffer.as_mut_ptr().add(wide_offset);
                let addr = self.framebuffer.buffer_mut().as_mut_ptr().add(offset);
                core::ptr::copy_nonoverlapping(wide, addr, info.width * info.bytes_per_pixel);
            }
        }
    }
}

impl<'f> DrawTarget for Display<'f> {
    type Color = Rgb888;

    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for pixel in pixels {
            self.draw_pixel(pixel);
        }
        Ok(())
    }

    fn fill_solid(
        &mut self,
        area: &embedded_graphics::primitives::Rectangle,
        color: Self::Color,
    ) -> Result<(), Self::Error> {
        let intersection = self.bounding_box().intersection(area);
        if intersection == Rectangle::zero() {
            return Ok(());
        }

        let color: Color = color.into();
        let info = self.framebuffer.info();
        let range = intersection.columns();
        let width = (range.end - range.start) as usize;

        let vec: alloc::vec::Vec<u32>;
        let vec2: alloc::vec::Vec<u8>;

        let wide = match info.pixel_format {
            PixelFormat::Rgb => {
                let color =
                    color.red as u32 | (color.green as u32) << 8 | (color.blue as u32) << 16;
                debug_assert_eq!(info.bytes_per_pixel, 4);
                vec = vec![color; width];
                vec.as_ptr() as *const u8
            }
            PixelFormat::Bgr => {
                let color =
                    color.blue as u32 | (color.green as u32) << 8 | (color.red as u32) << 16;
                debug_assert_eq!(info.bytes_per_pixel, 4);
                vec = vec![color; width];
                vec.as_ptr() as *const u8
            }
            PixelFormat::U8 => {
                let gray = color.red / 3 + color.green / 3 + color.blue / 3;
                debug_assert_eq!(info.bytes_per_pixel, 1);
                vec2 = vec![gray; width];
                vec2.as_ptr()
            }
            _ => todo!(),
        };
        let x = range.start as usize;

        for y in intersection.rows() {
            let offset = (y as usize * info.width + x) * info.bytes_per_pixel;
            unsafe {
                let addr = self.backbuffer.as_mut_ptr().add(offset);
                core::ptr::copy_nonoverlapping(wide, addr, width * info.bytes_per_pixel);
            }
        }
        Ok(())
    }

    fn clear(&mut self, color: Self::Color) -> Result<(), Self::Error> {
        let color: Color = color.into();
        let info = self.get_info();

        let vec: alloc::vec::Vec<u32>;
        let vec2: alloc::vec::Vec<u8>;

        let wide = match info.pixel_format {
            PixelFormat::Rgb => {
                let color =
                    color.red as u32 | (color.green as u32) << 8 | (color.blue as u32) << 16;
                debug_assert_eq!(info.bytes_per_pixel, 4);
                vec = vec![color; info.width];
                vec.as_ptr() as *const u8
            }
            PixelFormat::Bgr => {
                let color =
                    color.blue as u32 | (color.green as u32) << 8 | (color.red as u32) << 16;
                debug_assert_eq!(info.bytes_per_pixel, 4);
                vec = vec![color; info.width];
                vec.as_ptr() as *const u8
            }
            PixelFormat::U8 => {
                let gray = color.red / 3 + color.green / 3 + color.blue / 3;
                debug_assert_eq!(info.bytes_per_pixel, 1);
                vec2 = vec![gray; info.width];
                vec2.as_ptr()
            }
            _ => todo!(),
        };
        for y in 0..info.height {
            let offset = (y * info.width) * info.bytes_per_pixel;
            unsafe {
                let addr = self.backbuffer.as_mut_ptr().add(offset);
                core::ptr::copy_nonoverlapping(wide, addr, info.width * info.bytes_per_pixel);
            }
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
