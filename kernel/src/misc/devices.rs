use crate::{
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
pub struct DevNull;

impl Object for DevNull {
    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("statable", Statable);
}

impl Writable for DevNull {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        Ok(buffer.len())
    }
}

impl Readable for DevNull {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        buffer.fill(0);
        Ok(buffer.len())
    }
}

impl Statable for DevNull {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o666)
    }
}

#[derive(Debug, Default)]
pub struct DevKmsg {
    flags: spin::Mutex<FileFlags>,
}

impl Object for DevKmsg {
    impl_cast_function!("writable", Writable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);

    fn get_flags(self: alloc::sync::Arc<Self>) -> ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(self: alloc::sync::Arc<Self>, flags: FileFlags) -> ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }
}

impl Writable for DevKmsg {
    fn write(&self, buffer: &[u8]) -> ObjectResult<usize> {
        Ok(buffer.len())
    }
}

impl Readable for DevKmsg {
    fn read(&self, _buffer: &mut [u8]) -> ObjectResult<usize> {
        Err(crate::object::error::ObjectError::TryAgain)
    }
}

impl Pollable for DevKmsg {
    fn is_event_ready(&self, event: PollableEvent) -> bool {
        matches!(event, PollableEvent::CanBeWritten)
    }
}

impl Statable for DevKmsg {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o600)
    }
}
