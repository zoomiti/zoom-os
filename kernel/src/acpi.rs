use core::ptr::NonNull;

use acpi::{AcpiHandler, AcpiTables, InterruptModel, PhysicalMapping, PlatformInfo};
use alloc::{alloc::Global, rc::Rc};
use x86_64::{
    structures::paging::{Mapper, Page, PageSize, PageTableFlags, PhysFrame, Size4KiB},
    PhysAddr, VirtAddr,
};

use crate::{
    memory::{MAPPER, PAGE_ALLOCATOR},
    util::{once::OnceLock, r#async::mutex::Mutex},
};

pub static INTERRUPT_MODEL: OnceLock<InterruptModel<Global>> = OnceLock::new();

pub static KERNEL_ACPI_ADDR: OnceLock<VirtAddr> = OnceLock::new();
pub const KERNEL_ACPI_LEN: usize = 1024 * 1024;

pub fn init(boot_info: &'static bootloader_api::BootInfo) {
    let rsdp = boot_info.rsdp_addr.into_option().unwrap();

    let acpi_tables = match unsafe { AcpiTables::from_rsdp(KernelAcpi::new(), rsdp as usize) } {
        Ok(a) => a,
        Err(e) => panic!("acpi error: {:#?}\n is this a bios issue?", e),
    };

    if let Ok(platform_info) = PlatformInfo::new(&acpi_tables) {
        INTERRUPT_MODEL
            .try_init_once(|| platform_info.interrupt_model)
            .unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct KernelAcpi {
    start_addr: Rc<Mutex<u64>>,
    end_addr_exclusive: u64,
}

impl KernelAcpi {
    pub fn new() -> Self {
        let start_addr = KERNEL_ACPI_ADDR
            .get()
            .expect("kernel acpi address not init")
            .as_u64();
        let end_addr_exclusive = start_addr + KERNEL_ACPI_LEN as u64 - 1;
        Self {
            start_addr: Rc::new(Mutex::new(start_addr)),
            end_addr_exclusive,
        }
    }
}

impl Default for KernelAcpi {
    fn default() -> Self {
        Self::new()
    }
}

impl AcpiHandler for KernelAcpi {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> acpi::PhysicalMapping<Self, T> {
        let page = {
            let mut guard = self.start_addr.spin_lock();
            if *guard + Page::<Size4KiB>::SIZE >= self.end_addr_exclusive {
                panic!("acpi memory exhausted");
            }

            let page = Page::<Size4KiB>::containing_address(VirtAddr::new(*guard));
            assert!(size < Size4KiB::SIZE as usize);
            *guard += Size4KiB::SIZE;
            page
        };

        let mut mapper = MAPPER.spin_lock();
        let res = mapper
            .map_to(
                page,
                PhysFrame::containing_address(PhysAddr::new(physical_address as u64)),
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::NO_CACHE
                    | PageTableFlags::WRITE_THROUGH,
                &mut *PAGE_ALLOCATOR.get().unwrap().spin_lock(),
            )
            .unwrap();
        res.flush();
        PhysicalMapping::new(
            physical_address,
            NonNull::new(page.start_address().as_mut_ptr()).unwrap(),
            size,
            size,
            self.clone(),
        )
    }

    fn unmap_physical_region<T>(region: &acpi::PhysicalMapping<Self, T>) {
        let _ = region;
    }
}
