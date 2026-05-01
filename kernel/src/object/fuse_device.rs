use alloc::{collections::vec_deque::VecDeque, sync::Arc};
use spin::Mutex;

use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    object::{
        FileFlags, Object,
        error::ObjectError,
        misc::ObjectResult,
        queue_helpers::read_or_block_with_flags,
        traits::{Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
    thread::yielding::WakeType,
};

#[derive(Debug, Default)]
pub struct FuseDevice {
    flags: Mutex<FileFlags>,
    pending_requests: Mutex<VecDeque<u8>>,
}

impl FuseDevice {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

impl Object for FuseDevice {
    fn get_flags(self: Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("readable", Readable);
    impl_cast_function!("writable", Writable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
}

impl Readable for FuseDevice {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        self.read_with_flags(buffer, *self.flags.lock())
    }

    fn read_with_flags(&self, buffer: &mut [u8], flags: FileFlags) -> ObjectResult<usize> {
        read_or_block_with_flags(buffer, flags, WakeType::IO, |buffer| {
            let mut pending = self.pending_requests.lock();
            if pending.is_empty() {
                None
            } else {
                let mut read = 0usize;
                while read < buffer.len() {
                    let Some(byte) = pending.pop_front() else {
                        break;
                    };
                    buffer[read] = byte;
                    read += 1;
                }
                Some(read)
            }
        })
    }
}

impl Writable for FuseDevice {
    fn write(&self, _buffer: &[u8]) -> ObjectResult<usize> {
        Err(ObjectError::Unimplemented)
    }
}

impl Pollable for FuseDevice {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => !self.pending_requests.lock().is_empty(),
            PollableEvent::CanBeWritten => false,
            PollableEvent::Error | PollableEvent::Closed | PollableEvent::Other(_) => false,
        }
    }
}

impl Statable for FuseDevice {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device_with_rdev(0o666, (10u64 << 8) | 229)
    }
}
