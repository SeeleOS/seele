use acpi::platform::InterruptModel;
use alloc::vec::Vec;
use lazy_static::lazy_static;

use crate::{acpi::ACPI_TABLE, smp::current_apic_id_raw};

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

pub fn discover_from_acpi() {
    let Ok((_, processor_info)) = InterruptModel::new(ACPI_TABLE.get().unwrap()) else {
        return;
    };
    let Some(processor_info) = processor_info else {
        return;
    };

    let bsp_apic_id = current_apic_id_raw();
    let mut processors = PROCESSORS.lock();
    processors.clear();
    processors.push(Processor {
        index: 0,
        apic_id: bsp_apic_id,
        is_bsp: true,
    });

    let mut next_index = 1usize;
    for processor in core::iter::once(processor_info.boot_processor)
        .chain(processor_info.application_processors.iter().copied())
    {
        if processor.local_apic_id == bsp_apic_id
            || processor.state == acpi::platform::ProcessorState::Disabled
        {
            continue;
        }

        processors.push(Processor {
            index: next_index,
            apic_id: processor.local_apic_id,
            is_bsp: false,
        });
        next_index += 1;
    }
}
