use alloc::vec::Vec;
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
