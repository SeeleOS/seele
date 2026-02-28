use core::sync::atomic::AtomicU64;

use alloc::{sync::Arc, vec::Vec};

use crate::{
    graphics::object::TtyObject,
    keyboard::object::KeyboardObject,
    multitasking::thread::yielding::BlockType,
    object::{Object, tty_device::TtyDevice},
};

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct ProcessID(pub u64);

impl Default for ProcessID {
    fn default() -> Self {
        static NEXT_ID: AtomicU64 = AtomicU64::new(0);

        Self(NEXT_ID.fetch_add(1, core::sync::atomic::Ordering::Relaxed))
    }
}

pub fn init_objects(objects: &mut Vec<Arc<dyn Object>>) {
    objects.push(Arc::new(TtyDevice)); // stdin (unimpllemented)
    objects.push(Arc::new(TtyDevice)); // stdout
    objects.push(Arc::new(TtyDevice)); // stderr
}
