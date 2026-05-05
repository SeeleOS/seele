use acpi::sdt::madt::{Madt, MadtEntry};
use alloc::vec::Vec;
use core::pin::Pin;
use lazy_static::lazy_static;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Processor {
    pub index: usize,
    pub apic_id: u32,
    pub is_bsp: bool,
}

lazy_static! {
    static ref PROCESSORS: spin::Mutex<Vec<Processor>> = spin::Mutex::new(Vec::new());
}

pub fn register_bsp(apic_id: u32) {
    let mut processors = PROCESSORS.lock();
    if processors.is_empty() {
        processors.push(Processor {
            index: 0,
            apic_id,
            is_bsp: true,
        });
    }
}

pub fn processors() -> Vec<Processor> {
    PROCESSORS.lock().clone()
}

pub fn application_processors() -> Vec<Processor> {
    PROCESSORS
        .lock()
        .iter()
        .copied()
        .filter(|processor| !processor.is_bsp)
        .collect()
}

pub fn discover_from_acpi(bsp_apic_id: u32, madt: Pin<&Madt>) {
    let mut processors = PROCESSORS.lock();
    processors.clear();

    for entry in madt.entries() {
        let apic_id = match entry {
            MadtEntry::LocalApic(local_apic) if local_apic.flags & 1 != 0 => {
                local_apic.apic_id as u32
            }
            MadtEntry::LocalX2Apic(local_x2apic) if local_x2apic.flags & 1 != 0 => {
                local_x2apic.x2apic_id
            }
            _ => continue,
        };

        let index = processors.len();
        processors.push(Processor {
            index,
            apic_id,
            is_bsp: apic_id == bsp_apic_id,
        });
    }
}
