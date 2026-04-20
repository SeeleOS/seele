use crate::{
    filesystem::info::LinuxStat,
    impl_cast_function,
    object::{
        Object,
        misc::ObjectResult,
        traits::{Readable, Statable, Writable},
    },
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
