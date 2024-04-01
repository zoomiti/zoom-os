//use linked_list_allocator::LockedHeap;
use x86_64::{
    structures::paging::{FrameAllocator, Mapper, Page, PageTableFlags},
    VirtAddr,
};

use crate::{
    memory::{MAPPER, PAGE_ALLOCATOR},
    util::{
        once::{Lazy, OnceLock},
        r#async::mutex::IntMutex,
    },
};

use self::block::FixedSizeBlockAllocator;

mod block;
mod linked_list;

#[global_allocator]
//static ALLOCATOR: LockedHeap = LockedHeap::empty();
static ALLOCATOR: Lazy<IntMutex<FixedSizeBlockAllocator>> = Lazy::new(|| {
    let mut alloc = IntMutex::new(FixedSizeBlockAllocator::new());
    let page_range = {
        let heap_start = *KERNEL_HEAP_ADDR.get();
        let heap_end = heap_start + KERNEL_HEAP_LEN as u64 - 1u64;
        let heap_start_page = Page::containing_address(heap_start);
        let heap_end_page = Page::containing_address(heap_end);
        heap_start_page..=heap_end_page
    };

    let mut page_alloc = PAGE_ALLOCATOR.get().spin_lock();
    for page in page_range {
        let frame = page_alloc.allocate_frame().unwrap();
        let flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE;
        unsafe {
            MAPPER
                .spin_lock()
                .map_to(page, frame, flags, &mut *page_alloc)
                .expect("should not fail")
                .flush();
        }
    }
    unsafe {
        alloc
            .get_mut()
            .init(KERNEL_HEAP_ADDR.get().as_u64() as usize, KERNEL_HEAP_LEN);
    }
    alloc
});

pub static KERNEL_HEAP_ADDR: OnceLock<VirtAddr> = OnceLock::new();
pub const KERNEL_HEAP_LEN: usize = 8 * 1024 * 1024;

fn align_up(addr: usize, align: usize) -> usize {
    (addr + align - 1) & !(align - 1)
}
