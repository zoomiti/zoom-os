use core::{
    alloc::{GlobalAlloc, Layout},
    mem,
    ptr::NonNull,
};

use crate::util::r#async::mutex::Mutex;

const BLOCK_SIZES: &[usize] = &[8, 16, 32, 64, 128, 512, 1024, 2048];

struct ListNode {
    next: Option<&'static mut ListNode>,
}

impl ListNode {
    fn length(&self) -> usize {
        let mut node = self;
        let mut count = 1;
        while let Some(n) = &node.next {
            count += 1;
            node = n;
        }
        count
    }
}

pub struct FixedSizeBlockAllocator {
    list_heads: [Option<&'static mut ListNode>; BLOCK_SIZES.len()],
    fallback_allocator: linked_list_allocator::Heap,
}

impl FixedSizeBlockAllocator {
    pub const fn new() -> Self {
        const EMPTY: Option<&'static mut ListNode> = None;
        Self {
            list_heads: [EMPTY; BLOCK_SIZES.len()],
            fallback_allocator: linked_list_allocator::Heap::empty(),
        }
    }

    pub unsafe fn init(&mut self, heap_start: *mut u8, heap_size: usize) {
        self.fallback_allocator.init(heap_start, heap_size)
    }

    /// Allocates using the fallback allocator.
    fn fallback_alloc(&mut self, layout: Layout) -> *mut u8 {
        match self.fallback_allocator.allocate_first_fit(layout) {
            Ok(ptr) => ptr.as_ptr(),
            Err(_) => core::ptr::null_mut(),
        }
    }
}

impl Default for FixedSizeBlockAllocator {
    fn default() -> Self {
        Self::new()
    }
}

/// Choose an appropriate block size for the given layout.
///
/// Returns an index into the `BLOCK_SIZES` array.
fn list_index(layout: &Layout) -> Option<usize> {
    let required_block_size = layout.size().max(layout.align());
    BLOCK_SIZES.iter().position(|&s| s >= required_block_size)
}

unsafe impl GlobalAlloc for Mutex<FixedSizeBlockAllocator> {
    unsafe fn alloc(&self, layout: core::alloc::Layout) -> *mut u8 {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut alloc = self.spin_lock();
            match list_index(&layout) {
                Some(index) => match alloc.list_heads[index].take() {
                    Some(node) => {
                        alloc.list_heads[index] = node.next.take();
                        node as *mut ListNode as *mut u8
                    }
                    None => {
                        let block_size = BLOCK_SIZES[index];

                        let block_align = block_size;
                        let layout = Layout::from_size_align(block_size, block_align).unwrap();
                        alloc.fallback_alloc(layout)
                    }
                },
                None => alloc.fallback_alloc(layout),
            }
        })
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: core::alloc::Layout) {
        x86_64::instructions::interrupts::without_interrupts(|| {
            let mut alloc = self.spin_lock();
            match list_index(&layout) {
                Some(index) => {
                    if let Some(n) = &alloc.list_heads[index]
                        && n.length() > 16
                    {
                        let ptr = NonNull::new(ptr).unwrap();
                        alloc.fallback_allocator.deallocate(ptr, layout);
                    } else {
                        let new_node = ListNode {
                            next: alloc.list_heads[index].take(),
                        };
                        assert!(mem::size_of::<ListNode>() <= BLOCK_SIZES[index]);
                        assert!(mem::align_of::<ListNode>() <= BLOCK_SIZES[index]);
                        let new_node_ptr = ptr as *mut ListNode;
                        new_node_ptr.write(new_node);
                        alloc.list_heads[index] = Some(&mut *new_node_ptr);
                    }
                }
                None => {
                    let ptr = NonNull::new(ptr).unwrap();
                    alloc.fallback_allocator.deallocate(ptr, layout);
                }
            }
        })
    }
}
