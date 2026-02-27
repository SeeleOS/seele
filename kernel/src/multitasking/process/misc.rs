use core::sync::atomic::AtomicU64;

use alloc::{sync::Arc, vec::Vec};

use crate::{
    graphics::object::TtyObject, keyboard::object::KeyboardObject,
    multitasking::yielding::BlockType, object::Object,
};

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

pub fn init_objects(objects: &mut Vec<Arc<dyn Object>>) {
    objects.push(Arc::new(KeyboardObject {})); // stdin (unimpllemented)
    objects.push(Arc::new(TtyObject {})); // stdout
    objects.push(Arc::new(TtyObject {})); // stderr
}
