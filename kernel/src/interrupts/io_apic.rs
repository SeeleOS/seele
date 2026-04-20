use acpi::sdt::madt::{Madt, MadtEntry};
use x2apic::ioapic::{IoApic, IrqFlags, IrqMode, RedirectionTableEntry};

use crate::{
    acpi::ACPI_TABLE, interrupts::hardware_interrupt::HardwareInterrupt,
    memory::utils::apply_offset,
};

pub fn init() {
    let madt = ACPI_TABLE.get().unwrap().find_table::<Madt>().unwrap();

    let mut io_apic_entry = None;
    let mut keyboard_gsi = 1u8;
    let mut mouse_gsi = 12u8;

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

                if bus == 0 {
                    match irq {
                        1 => keyboard_gsi = gsi as u8,
                        12 => mouse_gsi = gsi as u8,
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    unsafe {
        let mut io_apic = IoApic::new(apply_offset(io_apic_entry.unwrap().io_apic_address as u64));

        let keyboard_entry = new_io_apic_entry(HardwareInterrupt::Keyboard.as_u8());
        let mouse_entry = new_io_apic_entry(HardwareInterrupt::Mouse.as_u8());

        io_apic.set_table_entry(keyboard_gsi, keyboard_entry);
        io_apic.set_table_entry(mouse_gsi, mouse_entry);

        io_apic.enable_irq(keyboard_gsi);
        io_apic.enable_irq(mouse_gsi);
    }
}

fn new_io_apic_entry(vector: u8) -> RedirectionTableEntry {
    let mut entry = RedirectionTableEntry::default();
    entry.set_mode(IrqMode::Fixed);
    entry.set_flags(IrqFlags::MASKED);
    entry.set_dest(0); // CPU(s)
    entry.set_vector(vector);
    entry
}
