use alloc::vec;
use tracing::instrument;
use x86_64::{
    instructions::tables::load_tss,
    registers::segmentation::{Segment, CS, DS, SS},
    structures::{
        gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector},
        tss::TaskStateSegment,
    },
    VirtAddr,
};

use crate::util::once::Lazy;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;

static TSS: Lazy<TaskStateSegment> = Lazy::new(|| {
    let mut tss = TaskStateSegment::new();
    tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = create_stack();
    tss
});

fn create_stack() -> VirtAddr {
    const STACK_SIZE: usize = 4096 * 5;
    let stack = vec![0; STACK_SIZE].leak();

    let stack_start = VirtAddr::from_ptr(stack.as_ptr());
    //stack end
    stack_start + STACK_SIZE as u64
}

static GDT: Lazy<(GlobalDescriptorTable, Selectors)> = Lazy::new(|| {
    let mut gdt = GlobalDescriptorTable::new();
    let kernel_code_selector = gdt.append(Descriptor::kernel_code_segment());
    let kernel_data_selector = gdt.append(Descriptor::kernel_data_segment());
    let tss_selector = gdt.append(Descriptor::tss_segment(&TSS));
    (
        gdt,
        Selectors {
            kernel_code_selector,
            kernel_data_selector,
            tss_selector,
        },
    )
});

struct Selectors {
    kernel_code_selector: SegmentSelector,
    kernel_data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
}

#[instrument(name = "gdt_init")]
pub fn init() {
    GDT.0.load();
    unsafe {
        CS::set_reg(GDT.1.kernel_code_selector);
        DS::set_reg(GDT.1.kernel_data_selector);
        SS::set_reg(SegmentSelector(0));
        load_tss(GDT.1.tss_selector);
    }
}
