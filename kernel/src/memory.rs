use core::{
    mem::{self, size_of},
    ops::{Coroutine, CoroutineState, Range},
    pin::Pin,
};

use alloc::{boxed::Box, vec::Vec};
use bootloader_api::info::{MemoryRegion, MemoryRegionKind, MemoryRegions};
use itertools::Itertools;
use x86_64::{
    registers::control::Cr3,
    structures::paging::{
        page_table::FrameError, FrameAllocator, FrameDeallocator, Mapper, OffsetPageTable, Page,
        PageSize, PageTable, PageTableFlags, PhysFrame, Size4KiB,
    },
    PhysAddr, VirtAddr,
};

use crate::{
    println,
    util::{
        once::{Lazy, OnceLock, TryInitError},
        r#async::mutex::Mutex,
    },
    PHYS_OFFSET,
};

pub static PAGE_ALLOCATOR: OnceLock<Mutex<SmartFrameAllocator>> = OnceLock::new();

pub static MAPPER: Lazy<Mutex<OffsetPageTable>> = Lazy::new(|| {
    let phys_mem_offset = VirtAddr::new(*PHYS_OFFSET.get());
    unsafe { Mutex::new(get_active_l4_table(phys_mem_offset)) }
});

pub fn init(memory_regions: &'static MemoryRegions) -> Result<(), TryInitError> {
    PAGE_ALLOCATOR
        .try_init_once(|| Mutex::new(unsafe { SmartFrameAllocator::init(memory_regions) }))?;

    // Now that the page allocator is setup, do clean up
    {
        let mut alloc = PAGE_ALLOCATOR.get().spin_lock();
        println!("{:?}", alloc.memory_ranges);
    }

    // do this to initialize the allocator
    let _ = Box::new(0);

    let mut alloc = PAGE_ALLOCATOR.get().spin_lock();
    let mut new_vec = alloc.memory_ranges.clone();
    core::mem::swap(&mut alloc.memory_ranges, &mut new_vec);
    unsafe {
        alloc.deallocate_frame(PhysFrame::<Size4KiB>::containing_address(PhysAddr::new(
            new_vec.as_ptr() as u64,
        )));
    }
    mem::forget(new_vec);
    println!("{:?}", alloc);

    Ok(())
}

/// Initialize a new OffsetPageTable.
///
/// # Safety
/// This function is unsafe because the caller must guarantee that the
/// complete physical memory is mapped to virtual memory at the passed
/// `physical_memory_offset`. Also, this function must be only called once
/// to avoid aliasing `&mut` references (which is undefined behavior).
unsafe fn get_active_l4_table(physical_memory_offset: VirtAddr) -> OffsetPageTable<'static> {
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
unsafe fn active_level_4_table(physical_memory_offset: VirtAddr) -> &'static mut PageTable {
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
    current_region: Option<Range<u64>>,
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
        self: core::pin::Pin<&mut Self>,
        _arg: (),
    ) -> core::ops::CoroutineState<Self::Yield, Self::Return> {
        let Self {
            memory_map_iter,
            current_region,
        } = self.get_mut();

        loop {
            // If we have a region iterate on it
            if let Some(range) = current_region.as_mut() {
                let (start, end) = (range.start, range.end);
                // Get the next available frame
                if start + 4096 <= end {
                    *range = (start + 4096)..end;
                    return CoroutineState::Yielded(PhysFrame::containing_address(PhysAddr::new(
                        start,
                    )));
                } else {
                    // There wasn't enough space for a frame so move on
                    *current_region = None;
                    continue;
                }
            } else {
                'get_region: loop {
                    let Some(possible_next_range) = memory_map_iter.next() else {
                        // No more memory regions
                        return CoroutineState::Complete(());
                    };
                    if possible_next_range.kind != MemoryRegionKind::Usable {
                        continue;
                    }
                    *current_region =
                        // We have found a new region to try iterating from
                        Some(possible_next_range.start..possible_next_range.end);
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

#[derive(Debug)]
pub struct SmartFrameAllocator {
    memory_ranges: Vec<Range<u64>>,
}

impl SmartFrameAllocator {
    /// Create a FrameAllocator from the passed memory map.
    ///
    /// # Safety
    /// This function is unsafe because the caller must guarantee that the passed
    /// memory map is valid. The main requirement is that all frames that are marked
    /// as `USABLE` in it are really unused.
    pub unsafe fn init(memory_map: &'static MemoryRegions) -> Self {
        let mut allocator = BootInfoFrameAllocator::init(memory_map);

        let mut frame = allocator.allocate_frame().unwrap();
        if frame.start_address().as_u64() == 0 {
            // WE CANT USE NULL
            frame = allocator.allocate_frame().unwrap();
        }
        let virtaddr = VirtAddr::new(frame.start_address().as_u64());
        let page = Page::from_start_address(virtaddr).unwrap();
        let flags = PageTableFlags::WRITABLE | PageTableFlags::PRESENT;
        // identity map
        unsafe {
            MAPPER
                .spin_lock()
                .map_to(page, frame, flags, &mut allocator)
                .unwrap()
                .flush();
        }

        let mut self_ = Self {
            // this is safe because this vector will never be dropped
            memory_ranges: unsafe {
                Vec::from_raw_parts(virtaddr.as_mut_ptr(), 0, 4096 / size_of::<Range<u64>>())
            },
        };
        for region in allocator.memory_map_iter {
            let range = region.start..region.end;
            self_.memory_ranges.push(range);
        }
        if let Some(range) = allocator.current_region {
            self_.memory_ranges.push(range);
        }

        self_
    }

    fn coallesce(&mut self) {
        self.memory_ranges.sort_by_key(|r| r.start);
        let coallesced = mem::take(&mut self.memory_ranges)
            .into_iter()
            .coalesce(|x, y| {
                if x.end == y.start {
                    Ok(x.start..y.end)
                } else if y.end == x.start {
                    Ok(y.start..x.end)
                } else {
                    Err((x, y))
                }
            })
            .filter(|r| r.start != r.end)
            .collect();

        self.memory_ranges = coallesced;
    }
}

unsafe impl<S: PageSize> FrameAllocator<S> for SmartFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<S>> {
        for range in self.memory_ranges.iter_mut() {
            let (start, end) = (range.start, range.end);
            if start + S::SIZE <= end {
                *range = (start + S::SIZE)..end;
                return Some(PhysFrame::containing_address(PhysAddr::new(start)));
            }
        }
        None
    }
}

impl<S: PageSize> FrameDeallocator<S> for SmartFrameAllocator {
    unsafe fn deallocate_frame(&mut self, frame: PhysFrame<S>) {
        let address = frame.start_address().as_u64();
        self.memory_ranges.push(address..address + S::SIZE);
        self.coallesce();
    }
}
