use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    misc::time::Time,
    object::{
        FileFlags, Object,
        misc::ObjectResult,
        traits::{Readable, Statable, Writable},
    },
    polling::{event::PollableEvent, object::Pollable},
};

#[derive(Debug)]
pub struct DevNull;

fn fill_pseudo_random(buffer: &mut [u8]) {
    let mut state = Time::since_boot().as_nanoseconds()
        ^ Time::current().as_nanoseconds()
        ^ (buffer.as_ptr() as u64).rotate_left(17)
        ^ (buffer.len() as u64).rotate_left(33);

    for byte in buffer {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        *byte = state as u8;
    }
}

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
    fn read(&self, _buffer: &mut [u8]) -> ObjectResult<usize> {
        Ok(0)
    }
}

impl Statable for DevNull {
    fn stat(&self) -> LinuxStat {
        LinuxStat::char_device(0o666)
    }
}

#[derive(Debug)]
pub struct DevRandom;

impl Object for DevRandom {
    impl_cast_function!("readable", Readable);
    impl_cast_function!("statable", Statable);
}

impl Readable for DevRandom {
    fn read(&self, buffer: &mut [u8]) -> ObjectResult<usize> {
        fill_pseudo_random(buffer);
        Ok(buffer.len())
    }
}

impl Statable for DevRandom {
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
