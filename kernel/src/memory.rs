use core::{
    mem::{self},
    ops::Range,
};

use alloc::vec::Vec;
use bootloader_api::info::{MemoryRegion, MemoryRegionKind, MemoryRegions};
use itertools::Itertools;
use x86_64::{
    structures::paging::{FrameAllocator, FrameDeallocator, PageSize, PhysFrame, Size4KiB},
    PhysAddr,
};

use crate::{
    allocator,
    util::{
        once::{OnceLock, TryInitError},
        r#async::mutex::Mutex,
    },
};

pub mod mapping;

pub static PAGE_ALLOCATOR: OnceLock<Mutex<SmartFrameAllocator>> = OnceLock::new();

pub fn init(memory_regions: &'static MemoryRegions) -> Result<(), TryInitError> {
    PAGE_ALLOCATOR
        .try_init_once(|| Mutex::new(unsafe { SmartFrameAllocator::init(memory_regions) }))?;

    Ok(())
}

pub struct BootInfoFrameAllocator {
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
            memory_map_iter: memory_map.iter(),
            current_region: None,
        }
    }
}

unsafe impl FrameAllocator<Size4KiB> for BootInfoFrameAllocator {
    fn allocate_frame(&mut self) -> Option<PhysFrame<Size4KiB>> {
        let BootInfoFrameAllocator {
            memory_map_iter,
            current_region,
        } = self;

        loop {
            // If we have a region iterate on it
            if let Some(range) = current_region.as_mut() {
                let (start, end) = (range.start, range.end);
                let new_start = start + Size4KiB::SIZE;
                // Get the next available frame
                if new_start <= end {
                    range.start = new_start;
                    return Some(PhysFrame::containing_address(PhysAddr::new(start)));
                } else {
                    // There wasn't enough space for a frame so move on
                    *current_region = None;
                    continue;
                }
            } else {
                'get_region: loop {
                    let Some(possible_next_range) = memory_map_iter.next() else {
                        // No more memory regions
                        return None;
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

        allocator::init(&mut allocator);

        // Now that the allocator is setup we can use a vec
        let mut memory_ranges = Vec::new();

        for region in allocator
            .memory_map_iter
            .filter(|r| r.kind != MemoryRegionKind::Usable)
        {
            let range = region.start..region.end;
            memory_ranges.push(range);
        }
        if let Some(range) = allocator.current_region {
            memory_ranges.push(range);
        }

        Self { memory_ranges }
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
            let new_start = start + S::SIZE;
            if new_start <= end {
                range.start = new_start;
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
