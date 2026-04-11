use alloc::sync::Arc;
use spin::Mutex;

use crate::{
    object::misc::ObjectRef,
    process::misc::with_current_process,
    terminal::pty::{master::PtyMaster, shared::PtyShared, slave::PtySlave},
};

pub mod master;
pub mod shared;
pub mod slave;

pub fn create_pty() -> (i32, i32) {
    let shared = Arc::new(Mutex::new(PtyShared::default()));

    let master = Arc::new(PtyMaster::new(shared.clone()));
    let slave = Arc::new(PtySlave::new(shared.clone()));
    let master_object: ObjectRef = master.clone();
    let slave_object: ObjectRef = slave.clone();

    {
        let mut shared = shared.lock();
        shared.master = Some(Arc::downgrade(&master_object));
        shared.slave = Some(Arc::downgrade(&slave_object));
    }

    with_current_process(|process| {
        (
            process.push_object(master_object) as i32,
            process.push_object(slave_object) as i32,
        )
    })
}
