use alloc::sync::Arc;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function, impl_cast_function_non_trait,
    memory::utils::Mut,
    object::{
        FileFlags, Object,
        error::ObjectError,
        misc::ObjectResult,
        traits::{Readable, Statable},
    },
    polling::{event::PollableEvent, object::Pollable},
    thread::yielding::{
        BlockType, WakeType, cancel_block, finish_block_current, prepare_block_current,
    },
};

#[derive(Debug, Default)]
pub struct InotifyObject {
    flags: Mut<FileFlags>,
    next_watch: Mut<i32>,
}

impl InotifyObject {
    pub fn add_watch(&self) -> i32 {
        let mut next_watch = self.next_watch.lock();
        *next_watch += 1;
        *next_watch
    }
}

impl Object for InotifyObject {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("readable", Readable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    impl_cast_function_non_trait!("inotify", InotifyObject);
}

impl Pollable for InotifyObject {
    fn is_event_ready(&self, _event: PollableEvent) -> bool {
        false
    }
}

impl Readable for InotifyObject {
    fn read(&self, _buffer: &mut [u8]) -> ObjectResult<usize> {
        if self.flags.lock().contains(FileFlags::NONBLOCK) {
            return Err(ObjectError::TryAgain);
        }

        loop {
            let current = prepare_block_current(BlockType::WakeRequired {
                wake_type: WakeType::IO,
                deadline: None,
            });

            if self.flags.lock().contains(FileFlags::NONBLOCK) {
                cancel_block(&current);
                return Err(ObjectError::TryAgain);
            }

            finish_block_current();
        }
    }
}

impl Statable for InotifyObject {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o600)
    }
}
