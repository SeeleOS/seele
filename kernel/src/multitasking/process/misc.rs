use core::sync::atomic::AtomicU64;

use alloc::{sync::Arc, vec::Vec};

use crate::{graphics::object::TtyObject, multitasking::yielding::BlockType, object::Writable};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProcessID(pub u64);

impl Default for ProcessID {
    fn default() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        Self(NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
}

#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum State {
    #[default]
    Ready, // ready to run (in a queue)
    Running,
    Blocked(BlockType), // stuck, waiting for something (like keyboard input)
    Zombie,             // Exited process
}

pub fn init_objects(objects: &mut Vec<Arc<dyn Writable>>) {
    objects[1] = Arc::new(TtyObject {});
}
