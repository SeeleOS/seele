pub use crate::smp::cpu::{DOUBLE_FAULT_IST_LOCATION, GP_IST_LOCATION, PAGE_FAULT_IST_LOCATION};

pub fn init() {
    log::debug!("tss: managed by per-cpu smp state");
}
