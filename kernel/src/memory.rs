use core::{
    iter::StepBy,
    ops::{Coroutine, CoroutineState, Range},
    pin::Pin,
};

use bootloader_api::info::{MemoryRegion, MemoryRegionKind, MemoryRegions};
use x86_64::{
    registers::control::Cr3,
    structures::paging::{
        page_table::FrameError, FrameAllocator, OffsetPageTable, PageTable, PhysFrame, Size4KiB,
    },
    PhysAddr, VirtAddr,
};

use crate::{
    util::{
        once::{Lazy, OnceLock, TryInitError},
        r#async::mutex::Mutex,
    },
    PHYS_OFFSET,
};

pub static PAGE_ALLOCATOR: OnceLock<Mutex<BootInfoFrameAllocator>> = OnceLock::new();

pub static MAPPER: Lazy<Mutex<OffsetPageTable>> = Lazy::new(|| {
    let phys_mem_offset = VirtAddr::new(*PHYS_OFFSET.get());
    unsafe { Mutex::new(get_active_l4_table(phys_mem_offset)) }
});

pub fn init(memory_regions: &'static MemoryRegions) -> Result<(), TryInitError> {
    PAGE_ALLOCATOR
        .try_init_once(|| Mutex::new(unsafe { BootInfoFrameAllocator::init(memory_regions) }))?;
    Ok(())
}

/// Initialize a new OffsetPageTable.
///
/// # Safety
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
pub unsafe fn get_active_l4_table(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
    let level_4_table = active_level_4_table(physical_memory_offset);
    OffsetPageTable::new(level_4_table, physical_memory_offset)
}

/// Returns a mutable reference to the active level 4 table.
///
/// # Safety
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
pub unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
    let (level_4_table_frame, _) = Cr3::read();

    let phys = level_4_table_frame.start_address();
    let virt = physical_memory_offset + phys.as_u64();
    let page_table_ptr: *mut PageTable = virt.as_mut_ptr();

    &mut *page_table_ptr // unsafe
}

/// Translates the given virtual address to the mapped physical address, or
/// `None` if the address is not mapped.
///
/// # Safety
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`.
pub unsafe fn translate_addr(addr: VirtAddr, physical_memory_offset: VirtAddr) -> Option<PhysAddr> {
    translate_addr_inner(addr, physical_memory_offset)
}

/// Private function that is called by `translate_addr`.
///
/// This function is safe to limit the scope of `unsafe` because Rust treats
/// the whole body of unsafe functions as an unsafe block. This function must
/// only be reachable through `unsafe fn` from outside of this module.
fn translate_addr_inner(addr: VirtAddr, physical_memory_offset: VirtAddr) -> Option<PhysAddr> {
    // read the active level 4 frame from the CR3 register
    let (level_4_table_frame, _) = Cr3::read();

    let table_indexes = [
        addr.p4_index(),
        addr.p3_index(),
        addr.p2_index(),
        addr.p1_index(),
    ];
    let mut frame = level_4_table_frame;

    // traverse the multi-level page table
    for &index in &table_indexes {
        // convert the frame into a page table reference
        let virt = physical_memory_offset + frame.start_address().as_u64();
        let table_ptr: *const PageTable = virt.as_ptr();
        let table = unsafe { &*table_ptr };

        // read the page table entry and update `frame`
        let entry = &table[index];
        frame = match entry.frame() {
            Ok(frame) => frame,
            Err(FrameError::FrameNotPresent) => return None,
            Err(FrameError::HugeFrame) => panic!("huge pages not supported"),
        };
    }

    // calculate the physical address by adding the page offset
    Some(frame.start_address() + u64::from(addr.page_offset()))
}

// TODO: Allow for frame deallocation
pub struct BootInfoFrameAllocator {
    //memory_map: &'static MemoryRegions,
    memory_map_iter: core::slice::Iter<'static, MemoryRegion>,
    current_region: Option<StepBy<Range<u64>>>,
}

impl BootInfoFrameAllocator {
    /// Create a FrameAllocator from the passed memory map.
    ///
    /// # Safety
    /// This function is unsafe because the caller must guarantee that the passed
    /// memory map is valid. The main requirement is that all frames that are marked
    /// as `USABLE` in it are really unused.
    pub unsafe fn init(memory_map: &'static MemoryRegions) -> Self {
        Self {
            //memory_map,
            memory_map_iter: memory_map.iter(),
            current_region: None,
        }
    }
}

impl Coroutine for BootInfoFrameAllocator {
    type Yield = PhysFrame;

    type Return = ();

    fn resume(
        mut self: core::pin::Pin<&mut Self>,
        _arg: (),
    ) -> core::ops::CoroutineState<Self::Yield, Self::Return> {
        loop {
            // If we have a region iterate on it
            if let Some(range) = self.current_region.as_mut() {
                // Get the next available frame
                let Some(next) = range.next() else {
                    // If we ran out of frame, empty the current region to get a new one
                    self.current_region = None;
                    continue;
                };
                return CoroutineState::Yielded(PhysFrame::containing_address(PhysAddr::new(next)));
            } else {
                'get_region: loop {
                    let Some(possible_next_range) = self.memory_map_iter.next() else {
                        // No more memory regions
                        return CoroutineState::Complete(());
                    };
                    if possible_next_range.kind != MemoryRegionKind::Usable {
                        continue;
                    }
                    self.current_region =
                        // We have found a new region to try iterating from
                        Some((possible_next_range.start..possible_next_range.end).step_by(4096));
                    break 'get_region;
                }
            }
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        match Pin::new(self).resume(()) {
            CoroutineState::Yielded(frame) => Some(frame),
            CoroutineState::Complete(_) => None,
        }
    }
}
