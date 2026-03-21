use acpi::sdt::madt::{Madt, MadtEntry};
use x2apic::ioapic::{IoApic, IrqFlags, IrqMode, RedirectionTableEntry};

use crate::{acpi::ACPI_TABLE, memory::utils::apply_offset};

pub fn init() {
    let madt = ACPI_TABLE.get().unwrap().find_table::<Madt>().unwrap();

    let mut io_apic_entry = None;
    let mut keyboard_gsi = 1u8;

    for entry in madt.get().entries() {
        match entry {
            MadtEntry::IoApic(io) => {
                io_apic_entry = Some(io);
            }
            // ISO basically means a IRQ is getting overwritten by a GSI
            MadtEntry::InterruptSourceOverride(iso) => {
                let bus = iso.bus;
                let irq = iso.irq;
                let gsi = iso.global_system_interrupt;

                if irq == 1 && bus == 0 {
                    keyboard_gsi = gsi as u8;
                }
            }
            _ => {}
        }
    }

    unsafe {
        let mut io_apic = IoApic::new(apply_offset(io_apic_entry.unwrap().io_apic_address as u64));

        let mut entry = RedirectionTableEntry::default();
        entry.set_mode(IrqMode::Fixed);
        entry.set_flags(IrqFlags::MASKED);
        entry.set_dest(0); // CPU(s)
        entry.set_vector(33);

        io_apic.set_table_entry(keyboard_gsi, entry);
        io_apic.enable_irq(keyboard_gsi);
    }
}
