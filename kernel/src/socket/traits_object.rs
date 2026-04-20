use crate::{
    impl_cast_function, impl_cast_function_non_trait,
    object::{
        FileFlags, Object,
        config::ConfigurateRequest,
        misc::ObjectResult,
        traits::{Configuratable, Readable, Statable, Writable},
    },
    polling::object::Pollable,
};

use super::{SocketLike, UnixSocketObject};

const FIONBIO: u64 = 0x5421;
const FIOCLEX: u64 = 0x5451;

impl Object for UnixSocketObject {
    fn get_flags(self: alloc::sync::Arc<Self>) -> crate::object::misc::ObjectResult<FileFlags> {
        Ok(*self.flags.lock())
    }

    fn set_flags(
        self: alloc::sync::Arc<Self>,
        flags: FileFlags,
    ) -> crate::object::misc::ObjectResult<()> {
        *self.flags.lock() = flags;
        Ok(())
    }

    impl_cast_function!("configuratable", Configuratable);
    impl_cast_function!("readable", Readable);
    impl_cast_function!("writable", Writable);
    impl_cast_function!("pollable", Pollable);
    impl_cast_function!("statable", Statable);
    impl_cast_function!("socket_like", SocketLike);
    impl_cast_function_non_trait!("unix_socket", UnixSocketObject);
}

impl Configuratable for UnixSocketObject {
    fn configure(&self, request: ConfigurateRequest) -> ObjectResult<isize> {
        match request {
            ConfigurateRequest::RawIoctl {
                request: FIOCLEX, ..
            } => Ok(0),
            ConfigurateRequest::RawIoctl {
                request: FIONBIO,
                arg,
            } => {
                let nonblocking = unsafe { *(arg as *const i32) };
                let mut flags = self.flags.lock();
                if nonblocking != 0 {
                    flags.insert(FileFlags::NONBLOCK);
                } else {
                    flags.remove(FileFlags::NONBLOCK);
                }
                Ok(0)
            }
            _ => Err(crate::object::error::ObjectError::InvalidRequest),
        }
    }
}
