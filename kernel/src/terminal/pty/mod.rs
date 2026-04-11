use acpi::sdt::fadt::ArmBootArchFlags;
use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    process::misc::with_current_process,
    terminal::pty::{master::PtyMaster, shared::PtyShared, slave::PtySlave},
};

pub mod master;
pub mod shared;
pub mod slave;

pub fn create_pty() -> (i32, i32) {
    let shared = Arc::new(Mutex::new(PtyShared::default()));

    let master = PtyMaster::new(shared.clone());
    let slave = PtySlave::new(shared.clone());

    with_current_process(|process| {
        (
            process.push_object(Arc::new(master)) as i32,
            process.push_object(Arc::new(slave)) as i32,
        )
    })
}
