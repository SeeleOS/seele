use alloc::{collections::btree_map::BTreeMap, sync::Arc, vec::Vec};
use spin::Mutex;

use crate::{
    object::misc::ObjectRef,
    process::misc::with_current_process,
    terminal::pty::{master::PtyMaster, shared::PtyShared, slave::PtySlave},
};

pub mod master;
pub mod shared;
pub mod slave;

struct PtyEntry {
    master: Arc<PtyMaster>,
    slave: Arc<PtySlave>,
    locked: bool,
}

#[derive(Default)]
struct PtyRegistry {
    next_number: u32,
    entries: BTreeMap<u32, PtyEntry>,
}

impl PtyRegistry {
    fn cleanup(&mut self) {
        self.entries.retain(|_, entry| {
            Arc::strong_count(&entry.master) > 1 || Arc::strong_count(&entry.slave) > 1
        });
    }

    fn create_entry(&mut self, locked: bool) -> (u32, ObjectRef, ObjectRef) {
        self.cleanup();

        let number = self.next_number;
        self.next_number += 1;

        let shared = Arc::new(Mutex::new(PtyShared::default()));
        let master = Arc::new(PtyMaster::new(number, shared.clone()));
        let slave = Arc::new(PtySlave::new(number, shared.clone()));
        let master_object: ObjectRef = master.clone();
        let slave_object: ObjectRef = slave.clone();

        {
            let mut shared = shared.lock();
            shared.master = Some(Arc::downgrade(&master_object));
            shared.slave = Some(Arc::downgrade(&slave_object));
        }

        self.entries.insert(
            number,
            PtyEntry {
                master,
                slave,
                locked,
            },
        );

        (number, master_object, slave_object)
    }
}

lazy_static::lazy_static! {
    static ref PTY_REGISTRY: Mutex<PtyRegistry> = Mutex::new(PtyRegistry::default());
}

fn create_registered_pty(locked: bool) -> (u32, ObjectRef, ObjectRef) {
    PTY_REGISTRY.lock().create_entry(locked)
}

pub fn create_pty() -> (i32, i32) {
    let (_, master_object, slave_object) = create_registered_pty(false);

    with_current_process(|process| {
        (
            process.push_object(master_object) as i32,
            process.push_object(slave_object) as i32,
        )
    })
}

pub fn open_ptmx() -> ObjectRef {
    let (_, master, _) = create_registered_pty(true);
    master
}

pub fn get_pty_slave(number: u32) -> Option<ObjectRef> {
    let mut registry = PTY_REGISTRY.lock();
    registry.cleanup();

    let entry = registry.entries.get(&number)?;
    if entry.locked {
        return None;
    }

    Some(entry.slave.clone())
}

pub fn list_ptys() -> Vec<u32> {
    let mut registry = PTY_REGISTRY.lock();
    registry.cleanup();
    registry.entries.keys().copied().collect()
}

pub fn set_pty_lock(number: u32, locked: bool) -> bool {
    let mut registry = PTY_REGISTRY.lock();
    registry.cleanup();

    let Some(entry) = registry.entries.get_mut(&number) else {
        return false;
    };
    entry.locked = locked;
    true
}
