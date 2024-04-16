use x86_64::{
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags, Size4KiB},
    VirtAddr,
};

use crate::{
    memory::mapping::MAPPER,
    util::{once::OnceLock, r#async::mutex::Mutex},
};

use self::block::FixedSizeBlockAllocator;

mod block;
mod linked_list;

#[global_allocator]
static ALLOCATOR: Mutex<FixedSizeBlockAllocator> = Mutex::new(FixedSizeBlockAllocator::new());

pub fn init(page_allocator: &mut impl FrameAllocator<Size4KiB>) {
    let page_range = {
        let heap_start = *KERNEL_HEAP_ADDR.get();
        let heap_end = heap_start + KERNEL_HEAP_LEN as u64 - 1u64;
        let heap_start_page = Page::<Size4KiB>::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        heap_start_page..=heap_end_page
    };
    {
        let mut mapper = MAPPER.spin_lock();
        for page in page_range {
            let frame = page_allocator.allocate_frame().unwrap();
            let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
            unsafe {
                mapper
                    .map_to(page, frame, flags, &mut *page_allocator)
                    .expect("should not fail")
                    .flush();
            }
        }
    }
    unsafe {
        ALLOCATOR
            .spin_lock()
            .init(KERNEL_HEAP_ADDR.get().as_mut_ptr(), KERNEL_HEAP_LEN);
    }
}

pub static KERNEL_HEAP_ADDR: OnceLock<VirtAddr> = OnceLock::new();
pub const KERNEL_HEAP_LEN: usize = 32 * 1024 * 1024;

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
