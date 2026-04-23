use alloc::vec::Vec;
use lazy_static::lazy_static;
use limine::response::MpResponse;

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

pub fn discover_from_limine(response: &MpResponse) {
    let bsp_apic_id = response.bsp_lapic_id();
    let mut processors = PROCESSORS.lock();
    processors.clear();

    for (index, cpu) in response.cpus().iter().enumerate() {
        processors.push(Processor {
            index,
            apic_id: cpu.lapic_id,
            is_bsp: cpu.lapic_id == bsp_apic_id,
        });
    }
}
