use crate::memory::utils::Mut;
use alloc::sync::Arc;

use crate::{
    filesystem::fusefs::FuseConnection,
    filesystem::info::LinuxStat,
    impl_cast_function,
    object::{
        FileFlags, Object,
        misc::ObjectResult,
        traits::{Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
};

#[derive(Debug)]
pub struct FuseDevice {
    flags: Mut<FileFlags>,
    pub connection: Arc<FuseConnection>,
}

impl FuseDevice {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            flags: Mut::new(FileFlags::empty()),
            connection: FuseConnection::new(),
        })
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
    crate::impl_cast_function_non_trait!("fuse_device", FuseDevice);
}

impl Readable for FuseDevice {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        self.read_with_flags(buffer, *self.flags.lock())
    }

    fn read_with_flags(&self, buffer: &mut [u8], flags: FileFlags) -> ObjectResult<usize> {
        self.connection.daemon_read(buffer, flags)
    }
}

impl Writable for FuseDevice {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        self.connection.daemon_write(buffer)
    }
}

impl Pollable for FuseDevice {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        match event {
            PollableEvent::CanBeRead => self.connection.is_request_pending(),
            PollableEvent::CanBeWritten => true,
            PollableEvent::Priority
            | PollableEvent::Error
            | PollableEvent::Closed
            | PollableEvent::ReadClosed
            | PollableEvent::Other(_) => false,
        }
    }
}

impl Statable for FuseDevice {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device_with_rdev(0o666, (10u64 << 8) | 229)
    }
}
