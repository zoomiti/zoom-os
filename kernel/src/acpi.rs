use core::{cell::OnceCell, ptr::NonNull};

use acpi::{AcpiError, AcpiHandler, AcpiTables, PhysicalMapping, PlatformInfo};
use alloc::{alloc::Global, rc::Rc};
use thiserror::Error;
use tracing::{error, instrument, warn};
use x86_64::{
    structures::paging::{Mapper, Page, PageSize, PageTableFlags, PhysFrame, Size4KiB},
    PhysAddr, VirtAddr,
};

use crate::{
    memory::{MAPPER, PAGE_ALLOCATOR},
    util::{
        once::{OnceLock, TryInitError},
        r#async::mutex::Mutex,
    },
};

pub static KERNEL_ACPI_ADDR: OnceLock<VirtAddr> = OnceLock::new();
pub const KERNEL_ACPI_LEN: usize = 1024 * 1024;

#[derive(Error, Debug)]
pub enum AcpiInitError {
    #[error("Rsdp ({1:x}) that bootloader found is bad: {0:?}")]
    BadRsdp(AcpiError, u64),
    #[error("Interrupt Model has already been init somehow")]
    InterruptModelAlreadyInit(#[from] TryInitError),
    #[error("PlatformInfo creation erorr: {0:?}")]
    PlatformInfoError(AcpiError),
}

#[instrument(err)]
pub fn init(rsdp: u64) -> Result<PlatformInfo<'static, Global>, AcpiInitError> {
    let acpi_tables = match unsafe { AcpiTables::from_rsdp(KernelAcpi::new(), rsdp as usize) } {
        Ok(tables) => tables,
        Err(err) => {
            warn!("Bad rsdp: trying to find using bios method");
            let try_bios = unsafe { AcpiTables::search_for_rsdp_bios(KernelAcpi::new()) };

            match try_bios {
                Ok(tables) => tables,
                Err(err2) => {
                    error!("Looking for bios rsdp failed: {:?}", err2);
                    return Err(AcpiInitError::BadRsdp(err, rsdp));
                }
            }
        }
    };

    PlatformInfo::new(&acpi_tables).map_err(AcpiInitError::PlatformInfoError)
}

#[derive(Debug, Clone)]
pub struct KernelAcpi {
    start_addr: Rc<Mutex<u64>>,
    end_addr_exclusive: u64,
}

impl KernelAcpi {
    pub fn new() -> Self {
        let start_addr = KERNEL_ACPI_ADDR.get().as_u64();
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
        let page_range = {
            let guard = self.start_addr.spin_lock();
            if *guard + size as u64 >= self.end_addr_exclusive {
                panic!("acpi memory exhausted");
            }

            let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(*guard));
            let end = *guard + size as u64;
            let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(end));
            start_page..=end_page
        };

        let virtual_start = OnceCell::new();
        let mut mapper = MAPPER.spin_lock();
        for page in page_range {
            virtual_start.get_or_init(|| NonNull::new(page.start_address().as_mut_ptr()));
            let res = mapper
                .map_to(
                    page,
                    PhysFrame::containing_address(PhysAddr::new(physical_address as u64)),
                    PageTableFlags::PRESENT
                        | PageTableFlags::WRITABLE
                        | PageTableFlags::NO_CACHE
                        | PageTableFlags::WRITE_THROUGH,
                    &mut *PAGE_ALLOCATOR.get().spin_lock(),
                )
                .unwrap();
            res.flush();
            let mut guard = self.start_addr.spin_lock();
            *guard += Size4KiB::SIZE;
        }
        PhysicalMapping::new(
            physical_address,
            virtual_start.into_inner().unwrap().unwrap(),
            size,
            size,
            self.clone(),
        )
    }

    fn unmap_physical_region<T>(region: &acpi::PhysicalMapping<Self, T>) {
        let page_range = {
            let start_page = Page::<Size4KiB>::containing_address(VirtAddr::new(
                region.virtual_start().as_ptr() as u64,
            ));
            let end = region.virtual_start().as_ptr() as u64 + region.region_length() as u64;
            let end_page = Page::<Size4KiB>::containing_address(VirtAddr::new(end));
            start_page..=end_page
        };
        for page in page_range {
            MAPPER.spin_lock().unmap(page).unwrap().1.flush();
            *region.handler().start_addr.spin_lock() -= Size4KiB::SIZE;
        }
    }
}
