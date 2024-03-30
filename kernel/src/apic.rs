use x2apic::{
    ioapic::{IoApic, IrqFlags, RedirectionTableEntry},
    lapic::{xapic_base, LocalApic, LocalApicBuilder, TimerDivide, TimerMode},
};
use x86_64::{
    instructions::port::Port,
    structures::paging::{Mapper, Page, PageTableFlags, PhysFrame, Size4KiB},
    PhysAddr, VirtAddr,
};

use crate::{
    acpi::INTERRUPT_MODEL,
    interrupts::InterruptIndex,
    memory::{MAPPER, PAGE_ALLOCATOR},
    util::{once::OnceLock, r#async::mutex::Mutex},
};

pub static LAPIC: OnceLock<Mutex<LocalApic>> = OnceLock::new();

pub static KERNEL_APIC_ADDR: OnceLock<VirtAddr> = OnceLock::new();
pub const KERNEL_APIC_LEN: usize = 4096;

pub fn init() {
    disable_8259();

    // SETUP LAPIC
    let apic_phys_addr =
        PhysAddr::try_new(unsafe { xapic_base() }).expect("physical address is not valid");
    let apic_phys_frame = PhysFrame::<Size4KiB>::containing_address(apic_phys_addr);

    let apic_virt_address = *KERNEL_APIC_ADDR.get().unwrap();

    let page = Page::containing_address(apic_virt_address);

    unsafe {
        MAPPER.spin_lock().map_to(
            page,
            apic_phys_frame,
            PageTableFlags::PRESENT
                | PageTableFlags::WRITABLE
                | PageTableFlags::NO_CACHE
                | PageTableFlags::NO_EXECUTE,
            &mut *PAGE_ALLOCATOR.get().expect("should exist").spin_lock(),
        )
    }
    .expect("should not fail to map apic")
    .flush();

    let mut lapic = LocalApicBuilder::new()
        .timer_vector(InterruptIndex::Timer as usize)
        .error_vector(InterruptIndex::LapicErr as usize)
        .spurious_vector(InterruptIndex::Spurious as usize)
        .set_xapic_base(apic_virt_address.as_u64())
        .timer_mode(TimerMode::Periodic)
        .timer_initial(65535)
        .timer_divide(TimerDivide::Div256)
        .build()
        .expect("");

    unsafe {
        lapic.enable();
    }

    // SETUP IOAPIC
    let interrupt_model = INTERRUPT_MODEL.get().unwrap();
    let apic = match interrupt_model {
        acpi::InterruptModel::Apic(apic) => apic,
        _ => panic!(),
    };

    let io_apics = &apic.io_apics;
    for io_apic in io_apics.iter() {
        let io_apic_phys_addr = PhysAddr::new(io_apic.address as u64);

        // Map io apic
        PhysAddr::try_new(unsafe { xapic_base() }).expect("physical address is not valid");
        let io_apic_phys_frame = PhysFrame::<Size4KiB>::containing_address(io_apic_phys_addr);

        let apic_virt_address = VirtAddr::new(io_apic_phys_addr.as_u64());

        let page = Page::containing_address(apic_virt_address);

        unsafe {
            MAPPER.spin_lock().map_to(
                page,
                io_apic_phys_frame,
                PageTableFlags::PRESENT
                    | PageTableFlags::WRITABLE
                    | PageTableFlags::NO_CACHE
                    | PageTableFlags::NO_EXECUTE,
                &mut *PAGE_ALLOCATOR.get().expect("should exist").spin_lock(),
            )
        }
        .expect("should not fail to map apic")
        .flush();

        unsafe {
            let mut io = IoApic::new(io_apic_phys_addr.as_u64());
            let offset = 32;
            io.init(offset); // 16

            // Setup Redirects
            let redirects = &apic.interrupt_source_overrides;

            for redirect in redirects.iter() {
                let mut entry = RedirectionTableEntry::default();
                entry.set_mode(x2apic::ioapic::IrqMode::Fixed);
                let polarity = match redirect.polarity {
                    acpi::platform::interrupt::Polarity::SameAsBus => {
                        // idk what to do here
                        continue;
                    }
                    acpi::platform::interrupt::Polarity::ActiveHigh => !IrqFlags::LOW_ACTIVE,
                    acpi::platform::interrupt::Polarity::ActiveLow => IrqFlags::LOW_ACTIVE,
                };
                let trigger = match redirect.trigger_mode {
                    acpi::platform::interrupt::TriggerMode::SameAsBus => {
                        // idk what to do here
                        continue;
                    }
                    acpi::platform::interrupt::TriggerMode::Edge => !IrqFlags::LEVEL_TRIGGERED,
                    acpi::platform::interrupt::TriggerMode::Level => IrqFlags::LEVEL_TRIGGERED,
                };
                entry.set_flags(trigger | polarity);
                entry.set_vector(redirect.isa_source);
                entry.set_dest(lapic.id() as u8);

                io.set_table_entry(redirect.global_system_interrupt as u8, entry);
                io.enable_irq(redirect.isa_source);
            }

            // Setup keyboard redirect
            let mut entry = RedirectionTableEntry::default();
            entry.set_dest(lapic.id() as u8);
            entry.set_vector(InterruptIndex::Keyboard as u8);
            entry.set_flags(IrqFlags::LEVEL_TRIGGERED);
            io.set_table_entry(InterruptIndex::Keyboard as u8 - offset, entry);
            io.enable_irq(InterruptIndex::Keyboard as u8 - offset);

            // Setup RTC redirect
            let mut entry = RedirectionTableEntry::default();
            entry.set_dest(lapic.id() as u8);
            entry.set_vector(InterruptIndex::Clock as u8);
            entry.set_flags(IrqFlags::LEVEL_TRIGGERED);
            io.set_table_entry(InterruptIndex::Clock as u8 - offset, entry);
            io.enable_irq(InterruptIndex::Clock as u8 - offset);
        }
    }
    LAPIC.try_init_once(|| Mutex::new(lapic)).unwrap();
}

fn disable_8259() {
    unsafe {
        // Disable 8259 immediately, thanks kennystrawnmusic

        let mut cmd_8259a = Port::<u8>::new(0x20);
        let mut data_8259a = Port::<u8>::new(0x21);
        let mut cmd_8259b = Port::<u8>::new(0xa0);
        let mut data_8259b = Port::<u8>::new(0xa1);

        let mut spin_port = Port::<u8>::new(0x80);
        let mut spin = || spin_port.write(0);

        cmd_8259a.write(0x11);
        cmd_8259b.write(0x11);
        spin();

        data_8259a.write(0xf8);
        data_8259b.write(0xff);
        spin();

        data_8259a.write(0b100);
        spin();

        data_8259b.write(0b10);
        spin();

        data_8259a.write(0x1);
        data_8259b.write(0x1);
        spin();

        data_8259a.write(u8::MAX);
        data_8259b.write(u8::MAX);
    };
}
